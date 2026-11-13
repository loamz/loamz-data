//! The two doors of a write lease (proposal 0005).
//!
//! A run that writes tables has a sandbox with no network. It is given two private Unix sockets instead, and nothing else:
//!
//! * the **catalog door** forwards the REST-catalog requests for the tables the run declared to the Data server, adding the run's own
//!   token (which the sandbox never sees);
//! * the **storage door** takes the S3 requests DuckDB makes, checks method, folder and operation against the lease, signs them again
//!   with the lease's own credentials (the signature the sandbox made is ignored) and forwards them to the real store.
//!
//! Neither door passes a header through. Both refuse anything not named by the lease, and both stop working when the lease is closed.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::RwLock,
    task::JoinHandle,
};

type HmacSha256 = Hmac<Sha256>;

/// The largest request body a door accepts: one part of a multipart upload, or one catalog commit.
const MAX_BODY: usize = 128 << 20;
/// How long a door waits for a request on an open connection.
const IDLE: Duration = Duration::from_secs(30);
/// The longest a single request may take, end to end.
const REQUEST: Duration = Duration::from_mins(10);
const MAX_CONNECTIONS: usize = 64;

/// What the storage door may do: one store, one bucket, some folders, and the lease's own credentials.
#[derive(Clone)]
pub struct StorageGrant {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub key_id: String,
    pub secret: String,
    pub session_token: String,
    /// Folders the run may read and write, each ending in `/`.
    pub prefixes: Vec<String>,
}

/// What the catalog door may do: one server, one workspace, the run's token, and the tables and schemas the run declared.
#[derive(Clone)]
pub struct CatalogGrant {
    pub base: String,
    pub workspace: String,
    pub token: String,
    /// Tables the run may load and commit, as `catalog.schema.table`.
    pub tables: Vec<String>,
    /// Schemas (`catalog.schema`) the run may create tables in.
    pub schemas: Vec<String>,
}

/// The two sockets of one worker or job, and who may use them right now.
pub struct Doors {
    storage: Arc<RwLock<Option<Arc<StorageGrant>>>>,
    catalog: Arc<RwLock<Option<Arc<CatalogGrant>>>>,
    tasks: Vec<JoinHandle<()>>,
    storage_socket: PathBuf,
    catalog_socket: PathBuf,
}

impl Doors {
    /// Opens `storage.sock` and `catalog.sock` in `dir`. Both refuse everything until a lease is opened.
    ///
    /// # Errors
    /// The sockets could not be made.
    pub fn start(dir: &Path) -> std::io::Result<Self> {
        let storage: Arc<RwLock<Option<Arc<StorageGrant>>>> = Arc::new(RwLock::new(None));
        let catalog: Arc<RwLock<Option<Arc<CatalogGrant>>>> = Arc::new(RwLock::new(None));
        let storage_socket = dir.join("storage.sock");
        let catalog_socket = dir.join("catalog.sock");
        let tasks = vec![
            listen(&storage_socket, {
                let grant = Arc::clone(&storage);
                move |stream| {
                    let grant = Arc::clone(&grant);
                    Box::pin(async move { serve_storage(stream, grant).await })
                }
            })?,
            listen(&catalog_socket, {
                let grant = Arc::clone(&catalog);
                move |stream| {
                    let grant = Arc::clone(&grant);
                    Box::pin(async move { serve_catalog(stream, grant).await })
                }
            })?,
        ];
        Ok(Self {
            storage,
            catalog,
            tasks,
            storage_socket,
            catalog_socket,
        })
    }

    pub async fn open(&self, storage: StorageGrant, catalog: CatalogGrant) {
        *self.storage.write().await = Some(Arc::new(storage));
        *self.catalog.write().await = Some(Arc::new(catalog));
    }

    /// Ends the lease: from now on both doors refuse everything.
    pub async fn close(&self) {
        *self.storage.write().await = None;
        *self.catalog.write().await = None;
    }

    #[must_use]
    pub fn sockets(&self) -> (&Path, &Path) {
        (&self.storage_socket, &self.catalog_socket)
    }
}

impl Drop for Doors {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        let _ = std::fs::remove_file(&self.storage_socket);
        let _ = std::fs::remove_file(&self.catalog_socket);
    }
}

type Served = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

fn listen(
    socket: &Path,
    serve: impl Fn(UnixStream) -> Served + Send + Sync + 'static,
) -> std::io::Result<JoinHandle<()>> {
    let listener = UnixListener::bind(socket)?;
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let serve = Arc::new(serve);
    Ok(tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                let _ = refuse(&mut stream, 503, "Busy").await;
                continue;
            };
            let serve = Arc::clone(&serve);
            tokio::spawn(async move {
                let _permit = permit;
                serve(stream).await;
            });
        }
    }))
}

async fn refuse(stream: &mut UnixStream, code: u16, reason: &str) -> std::io::Result<()> {
    stream
        .write_all(
            format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
}

/// One request as the sandbox sent it.
struct Asked {
    method: String,
    /// The path as sent, still percent-encoded, without the query.
    path: String,
    /// The query as sent, without the `?`.
    query: String,
    content_type: Option<String>,
    range: Option<String>,
    body: Vec<u8>,
    close: bool,
}

/// Reads one request off a connection: the line, the few headers a door cares about, and the body (by `Content-Length` only).
async fn read_request(stream: &mut UnixStream) -> Result<Option<Asked>, u16> {
    let mut raw = Vec::new();
    let mut block = [0_u8; 4096];
    let end = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if raw.len() > 32_768 {
            return Err(431);
        }
        let n = match tokio::time::timeout(IDLE, stream.read(&mut block)).await {
            Ok(Ok(n)) => n,
            _ => return Ok(None),
        };
        if n == 0 {
            return Ok(None);
        }
        raw.extend_from_slice(&block[..n]);
    };
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| 400_u16)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (Some(method), Some(target), Some("HTTP/1.1"), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        return Err(400);
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (mut length, mut content_type, mut range, mut close) = (0_usize, None, None, false);
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(400_u16)?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().map_err(|_| 400_u16)?;
        } else if name.eq_ignore_ascii_case("content-type") {
            content_type = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("range") {
            range = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("connection") {
            close = value.eq_ignore_ascii_case("close");
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("expect")
        {
            // Chunked uploads and `Expect: 100-continue` are not supported: the sender can use Content-Length.
            return Err(411);
        }
    }
    if length > MAX_BODY {
        return Err(413);
    }
    let mut body = raw[end..].to_vec();
    if body.len() > length {
        return Err(400);
    }
    while body.len() < length {
        let want = (length - body.len()).min(block.len());
        let n = match tokio::time::timeout(IDLE, stream.read(&mut block[..want])).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => return Err(400),
        };
        body.extend_from_slice(&block[..n]);
    }
    Ok(Some(Asked {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        content_type,
        range,
        body,
        close,
    }))
}

/// What a door sends back: the status, a few headers, a body.
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// For a reply to `HEAD`: the length of the object, which the reply states without sending it.
    head_length: Option<usize>,
}

impl Answer {
    fn plain(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            head_length: None,
        }
    }
}

async fn write_answer(
    stream: &mut UnixStream,
    answer: &Answer,
    close: bool,
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: {}\r\n",
        answer.status,
        reason(answer.status),
        answer.head_length.unwrap_or(answer.body.len()),
        if close { "close" } else { "keep-alive" }
    );
    for (name, value) in &answer.headers {
        if !value.contains(['\r', '\n']) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&answer.body).await
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        503 => "Busy",
        _ => "Status",
    }
}

/// The loop of one connection: read a request, answer it, until the sender closes or asks to.
async fn serve_connection<G, F, Fut>(
    mut stream: UnixStream,
    grant: Arc<RwLock<Option<Arc<G>>>>,
    answer: F,
) where
    F: Fn(Arc<G>, Asked) -> Fut,
    Fut: std::future::Future<Output = Answer>,
{
    loop {
        let asked = match read_request(&mut stream).await {
            Ok(Some(a)) => a,
            Ok(None) => return,
            Err(code) => {
                let _ = refuse(&mut stream, code, reason(code)).await;
                return;
            }
        };
        let close = asked.close;
        let Some(current) = grant.read().await.clone() else {
            let _ = refuse(&mut stream, 403, "Forbidden").await;
            return;
        };
        let reply = tokio::time::timeout(REQUEST, answer(current, asked))
            .await
            .unwrap_or_else(|_| Answer::plain(502));
        if write_answer(&mut stream, &reply, close).await.is_err() || close {
            return;
        }
    }
}

// ---- the catalog door ---------------------------------------------------------------------------------------------

async fn serve_catalog(stream: UnixStream, grant: Arc<RwLock<Option<Arc<CatalogGrant>>>>) {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST)
        .build();
    let Ok(http) = http else {
        return;
    };
    serve_connection(stream, grant, move |g, asked| {
        let http = http.clone();
        async move { forward_catalog(&http, &g, asked).await }
    })
    .await;
}

/// Whether the catalog door may pass this request on: only the REST-catalog routes of this workspace, loading anything the server
/// will authorize, but committing and creating only for what the run declared. Nothing is ever dropped.
#[must_use]
pub fn catalog_allows(grant: &CatalogGrant, method: &str, path: &str) -> bool {
    let prefix = format!("/api/v1/workspaces/{}/iceberg/v1/", grant.workspace);
    let Some(rest) = path.strip_prefix(&prefix) else {
        return false;
    };
    if rest.contains("..") || rest.contains('\\') {
        return false;
    }
    let parts: Vec<String> = rest.split('/').map(pct_decode).collect();
    // `config`, or `<catalog>/...`
    let reading = matches!(method, "GET" | "HEAD");
    if parts.first().is_some_and(|p| p == "config") {
        return reading;
    }
    let (Some(catalog), Some(kind)) = (parts.first(), parts.get(1)) else {
        return false;
    };
    match (kind.as_str(), parts.len()) {
        // namespaces: list, get one, and the tables of one
        ("namespaces", 2 | 3) => reading,
        ("namespaces", 4) if parts[3] == "tables" => {
            if reading {
                return true;
            }
            // Creating a table in a schema the run declared.
            method == "POST" && grant.schemas.contains(&format!("{catalog}.{}", parts[2]))
        }
        ("namespaces", 5) if parts[3] == "tables" => {
            let table = format!("{catalog}.{}.{}", parts[2], parts[4]);
            match method {
                "GET" | "HEAD" => true,
                // A commit to a table the run declared.
                "POST" => grant.tables.contains(&table),
                _ => false,
            }
        }
        _ => false,
    }
}

async fn forward_catalog(http: &reqwest::Client, grant: &CatalogGrant, asked: Asked) -> Answer {
    let path = pct_decode_path_for_check(&asked.path);
    if !catalog_allows(grant, &asked.method, &path) {
        tracing::warn!(
            method = asked.method,
            path,
            "the catalog door refused a request"
        );
        return Answer::plain(403);
    }
    if asked.query.contains(['\r', '\n']) {
        return Answer::plain(400);
    }
    let url = if asked.query.is_empty() {
        format!("{}{}", grant.base.trim_end_matches('/'), asked.path)
    } else {
        format!(
            "{}{}?{}",
            grant.base.trim_end_matches('/'),
            asked.path,
            asked.query
        )
    };
    let Ok(method) = reqwest::Method::from_bytes(asked.method.as_bytes()) else {
        return Answer::plain(400);
    };
    let mut request = http.request(method, url).bearer_auth(&grant.token);
    if let Some(kind) = &asked.content_type {
        request = request.header("Content-Type", kind);
    }
    if !asked.body.is_empty() {
        request = request.body(asked.body);
    }
    let Ok(response) = request.send().await else {
        return Answer::plain(502);
    };
    let status = response.status().as_u16();
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let Ok(body) = response.bytes().await else {
        return Answer::plain(502);
    };
    if body.len() > MAX_BODY {
        return Answer::plain(502);
    }
    Answer {
        status,
        headers: kind
            .map(|k| ("Content-Type".to_owned(), k))
            .into_iter()
            .collect(),
        body: body.to_vec(),
        head_length: None,
    }
}

// ---- the storage door ---------------------------------------------------------------------------------------------

async fn serve_storage(stream: UnixStream, grant: Arc<RwLock<Option<Arc<StorageGrant>>>>) {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST)
        .build();
    let Ok(http) = http else {
        return;
    };
    serve_connection(stream, grant, move |g, asked| {
        let http = http.clone();
        async move { forward_storage(&http, &g, asked).await }
    })
    .await;
}

/// The S3 operations a write lease allows, and where. `path` is `/<bucket>/<key>` as sent, `query` as sent.
///
/// Allowed, always inside the leased folders: get and head an object, put an object, delete an object, the multipart calls (start,
/// upload a part, complete, abort) and a listing whose `prefix` is inside a leased folder.
///
/// # Errors
/// Why the request is refused.
pub fn storage_allows(
    grant: &StorageGrant,
    method: &str,
    path: &str,
    query: &str,
) -> Result<(), &'static str> {
    let Some(rest) = path.strip_prefix('/') else {
        return Err("not a path");
    };
    let (bucket, key) = rest.split_once('/').unwrap_or((rest, ""));
    if bucket != grant.bucket {
        return Err("another bucket");
    }
    let pairs = parse_query(query);
    let has = |name: &str| pairs.iter().any(|(k, _)| k == name);
    let key = pct_decode(key);
    let inside = |k: &str| {
        !k.is_empty()
            && !k.split('/').any(|part| part == ".." || part == ".")
            && grant.prefixes.iter().any(|p| k.starts_with(p.as_str()))
    };
    // Anything but the known sub-resources is refused: `acl`, `tagging`, `versions`, `policy`, `copy` and the rest.
    let known = [
        "uploads",
        "uploadId",
        "partNumber",
        "list-type",
        "prefix",
        "delimiter",
        "max-keys",
        "continuation-token",
        "start-after",
        "encoding-type",
    ];
    if pairs.iter().any(|(k, _)| !known.contains(&k.as_str())) {
        return Err("a sub-resource that is not allowed");
    }
    if key.is_empty() {
        // The bucket itself: only a listing, and only of a leased folder.
        let prefix = pairs
            .iter()
            .find(|(k, _)| k == "prefix")
            .map(|(_, v)| v.as_str());
        return match (method, prefix) {
            ("GET", Some(p)) if grant.prefixes.iter().any(|g| p.starts_with(g.as_str())) => Ok(()),
            _ => Err("a listing outside the leased folders"),
        };
    }
    if !inside(&key) {
        return Err("outside the leased folders");
    }
    match method {
        "GET" | "HEAD" => Ok(()),
        "PUT" => Ok(()),
        "DELETE" => Ok(()),
        "POST" if has("uploads") || has("uploadId") => Ok(()),
        _ => Err("a method that is not allowed"),
    }
}

async fn forward_storage(http: &reqwest::Client, grant: &StorageGrant, asked: Asked) -> Answer {
    if let Err(why) = storage_allows(grant, &asked.method, &asked.path, &asked.query) {
        tracing::warn!(
            method = asked.method,
            path = asked.path,
            why,
            "the storage door refused a request"
        );
        return Answer::plain(403);
    }
    let endpoint = grant.endpoint.trim_end_matches('/');
    let host = endpoint
        .split_once("://")
        .map_or(endpoint, |(_, h)| h)
        .to_owned();
    let signed = sign_request(
        grant,
        &asked.method,
        &host,
        &asked.path,
        &asked.query,
        SystemTime::now(),
    );
    let url = if asked.query.is_empty() {
        format!("{endpoint}{}", asked.path)
    } else {
        format!("{endpoint}{}?{}", asked.path, asked.query)
    };
    let Ok(method) = reqwest::Method::from_bytes(asked.method.as_bytes()) else {
        return Answer::plain(400);
    };
    let mut request = http.request(method, url);
    for (name, value) in signed {
        request = request.header(name, value);
    }
    if let Some(kind) = &asked.content_type {
        request = request.header("Content-Type", kind);
    }
    if let Some(range) = &asked.range {
        request = request.header("Range", range);
    }
    if !asked.body.is_empty() {
        request = request.body(asked.body);
    }
    let Ok(response) = request.send().await else {
        return Answer::plain(502);
    };
    let status = response.status().as_u16();
    let headers: Vec<(String, String)> = [
        "etag",
        "content-type",
        "content-range",
        "accept-ranges",
        "last-modified",
    ]
    .iter()
    .filter_map(|name| {
        response
            .headers()
            .get(*name)
            .and_then(|v| v.to_str().ok())
            .map(|v| ((*name).to_owned(), v.to_owned()))
    })
    .collect();
    if asked.method == "HEAD" {
        // A reply to HEAD states the object's length and sends no body.
        let length = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        return Answer {
            status,
            headers,
            body: Vec::new(),
            head_length: Some(length),
        };
    }
    let Ok(body) = response.bytes().await else {
        return Answer::plain(502);
    };
    if body.len() > MAX_BODY {
        return Answer::plain(502);
    }
    Answer {
        status,
        headers,
        body: body.to_vec(),
        head_length: None,
    }
}

// ---- SigV4 --------------------------------------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    // HMAC takes a key of any length, so this does not fail.
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else {
        return Vec::new();
    };
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// The `AWS4-HMAC-SHA256` signature of a canonical request.
fn signature(
    canonical_request: &str,
    secret: &str,
    date: &str,
    region: &str,
    service: &str,
) -> (String, String) {
    let day = &date[..8];
    let scope = format!("{day}/{region}/{service}/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), day);
    let k_region = hmac(&k_date, region);
    let k_service = hmac(&k_region, service);
    let k_signing = hmac(&k_service, "aws4_request");
    (hex(&hmac(&k_signing, &to_sign)), scope)
}

/// `YYYYMMDDTHHMMSSZ` for a moment in time.
fn amz_date(now: SystemTime) -> String {
    let secs =
        i64::try_from(now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Percent-encodes everything but the unreserved characters (and `/` when `keep_slash`), as SigV4 asks.
fn uri_encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~')
            || (keep_slash && b == b'/')
        {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn pct_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The path with escapes undone, for the allow-list checks only (the request is forwarded as sent).
fn pct_decode_path_for_check(path: &str) -> String {
    pct_decode(path)
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (pct_decode(k), pct_decode(v))
        })
        .collect()
}

/// The headers that sign one request to the store with the lease's credentials. The body is not hashed (`UNSIGNED-PAYLOAD`), so a
/// large part can be forwarded as it is.
fn sign_request(
    grant: &StorageGrant,
    method: &str,
    host: &str,
    path: &str,
    query: &str,
    now: SystemTime,
) -> Vec<(String, String)> {
    let date = amz_date(now);
    let mut pairs = parse_query(query);
    pairs.sort();
    let canonical_query = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k, false), uri_encode(v, false)))
        .collect::<Vec<_>>()
        .join("&");
    // The path is signed as the store will read it: encoded once, exactly as sent.
    let canonical_path = if path.is_empty() { "/" } else { path };
    let payload = "UNSIGNED-PAYLOAD";
    let mut headers = vec![
        ("host", host.to_owned()),
        ("x-amz-content-sha256", payload.to_owned()),
        ("x-amz-date", date.clone()),
    ];
    if !grant.session_token.is_empty() {
        headers.push(("x-amz-security-token", grant.session_token.clone()));
    }
    headers.sort_by(|a, b| a.0.cmp(b.0));
    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();
    let signed_names = headers
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "{method}\n{canonical_path}\n{canonical_query}\n{canonical_headers}\n{signed_names}\n{payload}"
    );
    let (sig, scope) = signature(&canonical, &grant.secret, &date, &grant.region, "s3");
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_names}, Signature={sig}",
        grant.key_id
    );
    let mut out: Vec<(String, String)> = headers
        .into_iter()
        .filter(|(k, _)| *k != "host")
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    out.push(("authorization".to_owned(), authorization));
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn storage() -> StorageGrant {
        StorageGrant {
            endpoint: "http://127.0.0.1:9000".into(),
            region: "us-east-1".into(),
            bucket: "lake".into(),
            key_id: "AKID".into(),
            secret: "secret".into(),
            session_token: "token".into(),
            prefixes: vec!["ws/cat/tbl/".into()],
        }
    }

    fn catalog() -> CatalogGrant {
        CatalogGrant {
            base: "http://127.0.0.1:8020".into(),
            workspace: "w1".into(),
            token: "jt_x".into(),
            tables: vec!["sales.raw.orders".into()],
            schemas: vec!["sales.raw".into()],
        }
    }

    #[test]
    fn the_signature_matches_the_published_aws_example() {
        // The "get-vanilla" example from AWS's own signature documentation.
        let canonical = "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let (sig, scope) = signature(
            canonical,
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830T123600Z",
            "us-east-1",
            "service",
        );
        assert_eq!(scope, "20150830/us-east-1/service/aws4_request");
        assert_eq!(
            sig,
            "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn dates_are_written_the_way_signatures_expect() {
        assert_eq!(amz_date(UNIX_EPOCH), "19700101T000000Z");
        assert_eq!(
            amz_date(UNIX_EPOCH + Duration::from_secs(1_440_938_160)),
            "20150830T123600Z"
        );
        // A leap day.
        assert_eq!(
            amz_date(UNIX_EPOCH + Duration::from_secs(1_709_208_000)),
            "20240229T120000Z"
        );
    }

    #[test]
    fn the_storage_door_allows_only_the_leased_folders_and_known_operations() {
        let g = storage();
        let ok = |m: &str, p: &str, q: &str| storage_allows(&g, m, p, q).is_ok();
        assert!(ok("GET", "/lake/ws/cat/tbl/data/a.parquet", ""));
        assert!(ok(
            "PUT",
            "/lake/ws/cat/tbl/data/a.parquet",
            "partNumber=1&uploadId=abc"
        ));
        assert!(ok("POST", "/lake/ws/cat/tbl/data/a.parquet", "uploads"));
        assert!(ok(
            "POST",
            "/lake/ws/cat/tbl/data/a.parquet",
            "uploadId=abc"
        ));
        assert!(ok(
            "DELETE",
            "/lake/ws/cat/tbl/data/a.parquet",
            "uploadId=abc"
        ));
        assert!(ok(
            "GET",
            "/lake",
            "list-type=2&prefix=ws%2Fcat%2Ftbl%2Fdata"
        ));
        // Everything else.
        for (m, p, q) in [
            ("GET", "/other/ws/cat/tbl/x", ""),
            ("GET", "/lake/ws/cat/other/x", ""),
            ("GET", "/lake/ws/cat/tbl/../other/x", ""),
            ("GET", "/lake/ws/cat/tbl/%2e%2e/other/x", ""),
            ("GET", "/lake/ws/cat/tbl", ""),
            ("PUT", "/lake/ws/cat/tbl/x", "acl"),
            ("GET", "/lake/ws/cat/tbl/x", "versions"),
            ("PUT", "/lake/ws/cat/tbl/x", "tagging"),
            ("POST", "/lake/ws/cat/tbl/x", "delete"),
            ("POST", "/lake/ws/cat/tbl/x", ""),
            ("PATCH", "/lake/ws/cat/tbl/x", ""),
            ("GET", "/lake", "list-type=2&prefix=other%2F"),
            ("GET", "/lake", "list-type=2"),
            ("PUT", "/lake", ""),
        ] {
            assert!(!ok(m, p, q), "{m} {p}?{q}");
        }
    }

    #[test]
    fn the_catalog_door_passes_only_the_run_s_tables() {
        let g = catalog();
        let base = "/api/v1/workspaces/w1/iceberg/v1";
        let yes = |m: &str, p: &str| catalog_allows(&g, m, p);
        assert!(yes("GET", &format!("{base}/config")));
        assert!(yes(
            "GET",
            &format!("{base}/sales/namespaces/raw/tables/orders")
        ));
        assert!(yes(
            "POST",
            &format!("{base}/sales/namespaces/raw/tables/orders")
        ));
        assert!(yes("POST", &format!("{base}/sales/namespaces/raw/tables")));
        assert!(yes(
            "HEAD",
            &format!("{base}/sales/namespaces/raw/tables/other")
        ));
        for (m, p) in [
            ("POST", format!("{base}/sales/namespaces/raw/tables/other")),
            ("POST", format!("{base}/sales/namespaces/clean/tables")),
            (
                "DELETE",
                format!("{base}/sales/namespaces/raw/tables/orders"),
            ),
            ("POST", format!("{base}/config")),
            ("GET", "/api/v1/workspaces/w2/iceberg/v1/config".into()),
            ("GET", "/api/v1/workspaces/w1/securables".into()),
            ("GET", format!("{base}/sales/namespaces/raw/tables/../../x")),
            ("POST", format!("{base}/transactions/commit")),
        ] {
            assert!(!yes(m, &p), "{m} {p}");
        }
    }

    #[test]
    fn a_request_is_signed_with_the_lease_s_key_and_names_only_a_few_headers() {
        let headers = sign_request(
            &storage(),
            "PUT",
            "127.0.0.1:9000",
            "/lake/ws/cat/tbl/a",
            "partNumber=2&uploadId=x",
            UNIX_EPOCH,
        );
        let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        let auth = get("authorization").unwrap();
        assert!(
            auth.starts_with("AWS4-HMAC-SHA256 Credential=AKID/19700101/us-east-1/s3/aws4_request")
        );
        assert!(
            auth.contains(
                "SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token"
            )
        );
        assert_eq!(get("x-amz-security-token").unwrap(), "token");
        assert!(get("host").is_none(), "the client adds the host itself");
    }
}
