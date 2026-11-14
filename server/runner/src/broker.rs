//! A read-only object broker reached through one private Unix socket: the only way a SQL worker reaches storage.
//!
//! It serves `GET` and `HEAD` for exactly the folders the current lease names, and nothing else. Connections are kept open, and what
//! it reads is kept in a small cache: Iceberg metadata, manifests and Parquet files are never changed once written (a new version is
//! a new file), so the same bytes can be served again without asking the store. The cache is in the broker, outside the sandbox, and
//! every request is checked against the lease before the cache is looked at, so it can never serve a folder the lease does not name.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures::StreamExt;
use object_store::{
    GetRange, ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, path::Path as ObjectPath,
};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::RwLock,
    task::JoinHandle,
};

struct Grant {
    bucket: String,
    prefixes: Vec<String>,
    store: Arc<dyn ObjectStore>,
    generation: u64,
    /// Every object under the prefixes never changes (an Iceberg table's files): all of it may be remembered. Otherwise only files
    /// in a `results` folder, which are written once and never replaced, are.
    immutable: bool,
}

impl Grant {
    fn may_cache(&self, key: &str) -> bool {
        self.immutable || key.contains("/results/")
    }
}

/// Objects up to this size are fetched whole, once: a footer, a manifest or a small Parquet file is then served from memory.
const SMALL_OBJECT: u64 = 16 << 20;
/// How much the cache may hold.
const CACHE_BUDGET: usize = 384 << 20;
/// A connection that sends nothing for this long is closed.
const IDLE: Duration = Duration::from_secs(20);
/// Open connections at once. `DuckDB` keeps one per open file and thread, and keeps them open, so this is far above one query's need.
const MAX_CONNECTIONS: usize = 128;

#[derive(Clone)]
struct Meta {
    size: u64,
    etag: Option<String>,
}

#[derive(Default)]
struct CacheInner {
    tick: u64,
    used: usize,
    heads: HashMap<String, Meta>,
    /// (object, start, end) to its bytes and when it was last used.
    blobs: HashMap<(String, u64, u64), (Bytes, u64)>,
}

/// What the broker has already read. Kept for the life of the worker, or, for the jobs of a machine, for the life of the agent.
#[derive(Default)]
pub struct Cache {
    inner: Mutex<CacheInner>,
    hits: AtomicU64,
}

/// The cache that every job's broker on this machine shares: a result file read by one run is not read from storage again by the
/// next. Safe to share because a request is checked against its own lease before the cache is looked at.
#[must_use]
pub fn shared_cache() -> Arc<Cache> {
    static SHARED: std::sync::OnceLock<Arc<Cache>> = std::sync::OnceLock::new();
    Arc::clone(SHARED.get_or_init(|| Arc::new(Cache::default())))
}

impl Cache {
    fn head(&self, key: &str) -> Option<Meta> {
        self.inner.lock().ok()?.heads.get(key).cloned()
    }

    fn put_head(&self, key: &str, meta: Meta) {
        if let Ok(mut c) = self.inner.lock() {
            if c.heads.len() > 100_000 {
                c.heads.clear();
            }
            c.heads.insert(key.to_owned(), meta);
        }
    }

    /// The bytes of `start..end` of an object: from the whole object if it is held, or from exactly that range.
    fn get(&self, key: &str, size: u64, start: u64, end: u64) -> Option<Bytes> {
        let mut c = self.inner.lock().ok()?;
        c.tick += 1;
        let tick = c.tick;
        if let Some((whole, used)) = c.blobs.get_mut(&(key.to_owned(), 0, size)) {
            *used = tick;
            let (a, b) = (usize::try_from(start).ok()?, usize::try_from(end).ok()?);
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(whole.slice(a..b.min(whole.len())));
        }
        let (bytes, used) = c.blobs.get_mut(&(key.to_owned(), start, end))?;
        *used = tick;
        self.hits.fetch_add(1, Ordering::Relaxed);
        Some(bytes.clone())
    }

    fn put(&self, key: &str, start: u64, end: u64, bytes: Bytes) {
        if bytes.len() > CACHE_BUDGET / 4 {
            return;
        }
        let Ok(mut c) = self.inner.lock() else {
            return;
        };
        while c.used + bytes.len() > CACHE_BUDGET {
            let Some(oldest) = c
                .blobs
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some((gone, _)) = c.blobs.remove(&oldest) {
                c.used = c.used.saturating_sub(gone.len());
            }
        }
        c.tick += 1;
        let tick = c.tick;
        c.used += bytes.len();
        if let Some((old, _)) = c.blobs.insert((key.to_owned(), start, end), (bytes, tick)) {
            c.used = c.used.saturating_sub(old.len());
        }
    }
}

/// The broker never accepts a caller-supplied endpoint or credentials from a socket request.
pub struct Broker {
    access: Arc<RwLock<Option<Arc<Grant>>>>,
    generation: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
    cache: Arc<Cache>,
    /// Clients to the store, by the credentials they use: a statement with the same credentials as the last reuses its client.
    clients: Mutex<HashMap<String, Arc<dyn ObjectStore>>>,
    task: JoinHandle<()>,
    requests: Arc<Mutex<Vec<tokio::task::AbortHandle>>>,
    socket: PathBuf,
}

impl Broker {
    pub fn start(dir: &Path) -> std::io::Result<Self> {
        Self::start_with_cache(dir, Arc::new(Cache::default()))
    }

    /// As [`Broker::start`], remembering what it reads in `cache`, which other brokers may share.
    pub fn start_with_cache(dir: &Path, cache: Arc<Cache>) -> std::io::Result<Self> {
        let socket = dir.join("socket");
        let listener = UnixListener::bind(&socket)?;
        let access = Arc::new(RwLock::new(None));
        let generation = Arc::new(AtomicU64::new(0));
        let bytes = Arc::new(AtomicU64::new(0));
        let requests = Arc::new(Mutex::new(Vec::<tokio::task::AbortHandle>::new()));
        let task = tokio::spawn({
            let (access, generation, bytes, cache) = (
                Arc::clone(&access),
                Arc::clone(&generation),
                Arc::clone(&bytes),
                Arc::clone(&cache),
            );
            let requests = Arc::clone(&requests);
            async move {
                let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
                while let Ok((stream, _)) = listener.accept().await {
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                        tracing::warn!("the storage broker is at its connection limit");
                        let mut stream = stream;
                        let _ = error(&mut stream, 503, "Busy").await;
                        continue;
                    };
                    let (access, generation, bytes, cache) = (
                        Arc::clone(&access),
                        Arc::clone(&generation),
                        Arc::clone(&bytes),
                        Arc::clone(&cache),
                    );
                    if let Ok(mut requests) = requests.lock() {
                        let handle = tokio::spawn(async move {
                            let _permit = permit;
                            let _ = tokio::time::timeout(
                                Duration::from_mins(5),
                                serve(stream, access, generation, bytes, cache),
                            )
                            .await;
                        });
                        requests.retain(|handle| !handle.is_finished());
                        requests.push(handle.abort_handle());
                    }
                }
            }
        });
        Ok(Self {
            access,
            generation,
            bytes,
            cache,
            clients: Mutex::default(),
            task,
            requests,
            socket,
        })
    }

    /// Install access for precisely the tables in this lease, before giving it to the worker.
    pub async fn authorize(&self, storage: Option<&Value>, spec: &Value) -> Result<(), String> {
        self.clear().await;
        let tables = spec["tables"]
            .as_array()
            .ok_or("the SQL lease has no table list")?;
        if tables.is_empty() {
            return Ok(());
        }
        let storage = storage.ok_or("the lease has no storage access")?;
        let mut prefixes = Vec::new();
        for table in tables {
            let id = table["id"].as_str().ok_or("a table has no identity")?;
            let folder = storage["tables"][id]["folder"]
                .as_str()
                .ok_or("a table has no folder")?;
            let parsed = ObjectPath::parse(folder).map_err(|_| "a table folder is invalid")?;
            if parsed.as_ref().is_empty() {
                return Err("a table folder is empty".into());
            }
            prefixes.push(format!("{}/", parsed.as_ref().trim_end_matches('/')));
        }
        self.grant_for(storage, prefixes, true).await
    }

    /// Install access for exactly the folders a job reads (`read_prefixes` of the lease, each ending in `/`), before giving it to the
    /// sandbox. Only what is under a `results` folder is remembered: the rest may be replaced.
    pub async fn authorize_reads(&self, storage: &Value) -> Result<(), String> {
        self.clear().await;
        let prefixes: Vec<String> = storage["read_prefixes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect();
        if prefixes.is_empty() || prefixes.iter().any(|p| p.is_empty() || !p.ends_with('/')) {
            return Err("the lease names no folder to read".into());
        }
        self.grant_for(storage, prefixes, false).await
    }

    async fn grant_for(
        &self,
        storage: &Value,
        prefixes: Vec<String>,
        immutable: bool,
    ) -> Result<(), String> {
        let field = |key: &str| {
            storage[key]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or("the storage access is incomplete")
        };
        let bucket = field("bucket")?.to_owned();
        let endpoint = field("endpoint")?;
        // A store's own long-lived key has no session token; a vended credential has one.
        let (key_id, secret) = (field("key_id")?, field("secret")?);
        let token = storage["session_token"].as_str().unwrap_or_default();
        let client_key = format!("{endpoint}|{bucket}|{key_id}|{token}");
        let reused = self
            .clients
            .lock()
            .ok()
            .and_then(|c| c.get(&client_key).cloned());
        let store: Arc<dyn ObjectStore> = if let Some(client) = reused {
            client
        } else {
            let mut builder = AmazonS3Builder::new()
                .with_endpoint(endpoint)
                .with_bucket_name(&bucket)
                .with_access_key_id(key_id)
                .with_secret_access_key(secret)
                .with_region(storage["region"].as_str().unwrap_or("auto"))
                .with_allow_http(endpoint.starts_with("http://"));
            if !token.is_empty() {
                builder = builder.with_token(token);
            }
            let built: Arc<dyn ObjectStore> = Arc::new(
                builder
                    .build()
                    .map_err(|_| "the storage access could not be prepared")?,
            );
            if let Ok(mut c) = self.clients.lock() {
                if c.len() >= 8 {
                    c.clear();
                }
                c.insert(client_key, Arc::clone(&built));
            }
            built
        };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.access.write().await = Some(Arc::new(Grant {
            bucket,
            prefixes,
            store,
            generation,
            immutable,
        }));
        Ok(())
    }

    pub async fn clear(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.access.write().await = None;
        if let Ok(mut requests) = self.requests.lock() {
            for request in requests.drain(..) {
                request.abort();
            }
        }
    }

    #[must_use]
    pub fn bytes_read(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// How many reads were answered from the cache.
    #[must_use]
    pub fn cache_hits(&self) -> u64 {
        self.cache.hits.load(Ordering::Relaxed)
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.task.abort();
        if let Ok(mut requests) = self.requests.lock() {
            for request in requests.drain(..) {
                request.abort();
            }
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

async fn error(stream: &mut UnixStream, code: u16, reason: &str) -> std::io::Result<()> {
    if code != 403 {
        // A refusal for a folder outside the lease is expected; anything else is worth knowing about.
        tracing::warn!(code, reason, "the storage broker refused a request");
    }
    stream
        .write_all(
            format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
}

fn parse_range(value: &str) -> Option<GetRange> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    if end.contains(',') {
        return None;
    }
    if start.is_empty() {
        return Some(GetRange::Suffix(end.parse().ok()?));
    }
    let start: u64 = start.parse().ok()?;
    if end.is_empty() {
        return Some(GetRange::Offset(start));
    }
    let end: u64 = end.parse::<u64>().ok()?.checked_add(1)?;
    (end > start).then_some(GetRange::Bounded(start..end))
}

async fn read_header(stream: &mut UnixStream) -> std::io::Result<Vec<u8>> {
    let mut raw = Vec::new();
    let mut block = [0_u8; 2048];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut block).await?;
        if read == 0 || raw.len().saturating_add(read) > 16_384 {
            return Err(std::io::Error::other("invalid request header"));
        }
        raw.extend_from_slice(&block[..read]);
    }
    Ok(raw)
}

fn parse_headers<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<(Option<GetRange>, Option<String>, bool), u16> {
    let (mut range, mut if_match, mut close) = (None, None, false);
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(400_u16)?;
        if name.eq_ignore_ascii_case("range") {
            if range.is_some() {
                return Err(400);
            }
            range = Some(parse_range(value.trim()).ok_or(416_u16)?);
        } else if name.eq_ignore_ascii_case("if-match") {
            if if_match.is_some() {
                return Err(400);
            }
            if_match = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("connection") {
            close = value.trim().eq_ignore_ascii_case("close");
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return Err(400);
        }
    }
    Ok((range, if_match, close))
}

async fn serve(
    mut stream: UnixStream,
    access: Arc<RwLock<Option<Arc<Grant>>>>,
    generation: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
    cache: Arc<Cache>,
) -> std::io::Result<()> {
    // One connection serves many requests, until the client closes it, asks to, or says nothing for a minute.
    loop {
        let Ok(Ok(raw)) = tokio::time::timeout(IDLE, read_header(&mut stream)).await else {
            return Ok(());
        };
        match handle(&mut stream, &raw, &access, &generation, &bytes, &cache).await {
            Ok(true) => {}
            Ok(false) => return Ok(()),
            Err(e) => {
                tracing::warn!(error = %e, "the storage broker lost a connection");
                return Err(e);
            }
        }
    }
}

/// Answers one request. `false` means the connection should now be closed.
#[allow(clippy::too_many_lines)]
async fn handle(
    stream: &mut UnixStream,
    raw: &[u8],
    access: &Arc<RwLock<Option<Arc<Grant>>>>,
    generation: &Arc<AtomicU64>,
    bytes: &Arc<AtomicU64>,
    cache: &Arc<Cache>,
) -> std::io::Result<bool> {
    let Ok(header) = std::str::from_utf8(raw) else {
        error(stream, 400, "Bad Request").await?;
        return Ok(false);
    };
    let mut lines = header.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (Some(method), Some(target), Some("HTTP/1.1"), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        error(stream, 400, "Bad Request").await?;
        return Ok(false);
    };
    if !matches!(method, "GET" | "HEAD") || target.contains('?') {
        error(stream, 403, "Forbidden").await?;
        return Ok(false);
    }
    let Some(grant) = access.read().await.clone() else {
        error(stream, 403, "Forbidden").await?;
        return Ok(false);
    };
    let Some((bucket, key)) = target.strip_prefix('/').and_then(|v| v.split_once('/')) else {
        error(stream, 400, "Bad Request").await?;
        return Ok(false);
    };
    let Ok(path) = ObjectPath::from_url_path(key) else {
        error(stream, 400, "Bad Request").await?;
        return Ok(false);
    };
    // The lease is checked first, every time: what is in the cache is never served to a folder the lease does not name.
    if bucket != grant.bucket
        || !grant
            .prefixes
            .iter()
            .any(|prefix| path.as_ref().starts_with(prefix))
    {
        error(stream, 403, "Forbidden").await?;
        return Ok(false);
    }
    let (range, if_match, close) = match parse_headers(lines) {
        Ok(parsed) => parsed,
        Err(416) => {
            error(stream, 416, "Range Not Satisfiable").await?;
            return Ok(false);
        }
        Err(_) => {
            error(stream, 400, "Bad Request").await?;
            return Ok(false);
        }
    };
    let connection = if close { "close" } else { "keep-alive" };
    let name = format!("{bucket}/{}", path.as_ref());
    let keep = grant.may_cache(path.as_ref());
    let meta = if let Some(m) = cache.head(&name).filter(|_| keep) {
        m
    } else {
        let Ok(found) = grant.store.head(&path).await else {
            error(stream, 404, "Not Found").await?;
            return Ok(false);
        };
        let m = Meta {
            size: found.size,
            etag: found.e_tag.clone(),
        };
        if keep {
            cache.put_head(&name, m.clone());
        }
        m
    };
    if generation.load(Ordering::SeqCst) != grant.generation {
        error(stream, 403, "Forbidden").await?;
        return Ok(false);
    }
    let etag = etag_header(meta.etag.as_deref());
    if method == "HEAD" {
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{etag}Accept-Ranges: bytes\r\nConnection: {connection}\r\n\r\n", meta.size).as_bytes()).await?;
        return Ok(!close);
    }
    if if_match.is_some_and(|wanted| {
        meta.etag
            .as_deref()
            .is_some_and(|have| have != wanted && wanted != "*")
    }) {
        error(stream, 412, "Precondition Failed").await?;
        return Ok(false);
    }
    let ranged = range.is_some();
    let size = meta.size;
    let (start, end) = match range {
        None => (0, size),
        Some(GetRange::Offset(o)) => (o, size),
        Some(GetRange::Bounded(r)) => (r.start, r.end.min(size)),
        Some(GetRange::Suffix(n)) => (size.saturating_sub(n), size),
    };
    if start > end || (start >= size && size > 0) {
        error(stream, 416, "Range Not Satisfiable").await?;
        return Ok(false);
    }
    let status = if ranged {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let content_range = if ranged {
        format!(
            "Content-Range: bytes {start}-{}/{size}\r\n",
            end.saturating_sub(1)
        )
    } else {
        String::new()
    };
    let response_head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{content_range}{etag}Accept-Ranges: bytes\r\nConnection: {connection}\r\n\r\n",
        end - start
    );

    // From memory if it has been read before; a small object is read whole, once; a range of a big one is read as asked.
    let remembered = if keep {
        cache.get(&name, size, start, end)
    } else {
        None
    };
    let body = if let Some(b) = remembered {
        Some(b)
    } else if size <= SMALL_OBJECT {
        let Ok(whole) = grant.store.get(&path).await else {
            error(stream, 404, "Not Found").await?;
            return Ok(false);
        };
        let Ok(all) = whole.bytes().await else {
            error(stream, 502, "Bad Gateway").await?;
            return Ok(false);
        };
        if keep {
            cache.put(&name, 0, size, all.clone());
        }
        let (a, b) = (
            usize::try_from(start).unwrap_or(0),
            usize::try_from(end).unwrap_or(0),
        );
        Some(all.slice(a..b.min(all.len())))
    } else if ranged {
        let Ok(part) = grant.store.get_range(&path, start..end).await else {
            error(stream, 404, "Not Found").await?;
            return Ok(false);
        };
        if keep {
            cache.put(&name, start, end, part.clone());
        }
        Some(part)
    } else {
        None
    };
    if generation.load(Ordering::SeqCst) != grant.generation {
        error(stream, 403, "Forbidden").await?;
        return Ok(false);
    }
    stream.write_all(response_head.as_bytes()).await?;
    if let Some(body) = body {
        stream.write_all(&body).await?;
        bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
        return Ok(!close);
    }
    // A whole big object, asked for without a range: streamed, not held.
    let Ok(result) = grant.store.get(&path).await else {
        tracing::warn!(object = %path, "the store would not give a whole object");
        return Ok(false);
    };
    let mut body = result.into_stream();
    while let Some(chunk) = body.next().await {
        if generation.load(Ordering::SeqCst) != grant.generation {
            tracing::warn!(object = %path, "the lease ended while an object was being sent");
            return Ok(false);
        }
        let chunk = chunk.map_err(std::io::Error::other)?;
        stream.write_all(&chunk).await?;
        bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    Ok(!close)
}

fn etag_header(tag: Option<&str>) -> String {
    tag.filter(|s| !s.contains(['\r', '\n']))
        .map_or_else(String::new, |s| format!("ETag: {s}\r\n"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use object_store::memory::InMemory;

    async fn request(socket: &Path, request: &str) -> Vec<u8> {
        let mut stream = UnixStream::connect(socket).await.unwrap();
        // One request, then the broker closes: without this a kept-open connection would never end the read.
        let request = request.replacen("\r\n\r\n", "\r\nConnection: close\r\n\r\n", 1);
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn broker_enforces_prefix_and_returns_only_the_requested_range() {
        let dir = std::env::temp_dir().join(format!("loamz-broker-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let broker = Broker::start(&dir).unwrap();
        let store = Arc::new(InMemory::new());
        store
            .put(&ObjectPath::from("allowed/file"), "abcdefghij".into())
            .await
            .unwrap();
        store
            .put(&ObjectPath::from("other/file"), "private".into())
            .await
            .unwrap();
        let generation = broker.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *broker.access.write().await = Some(Arc::new(Grant {
            immutable: true,
            bucket: "bucket".into(),
            prefixes: vec!["allowed/".into()],
            store,
            generation,
        }));
        let socket = dir.join("socket");
        let response = request(
            &socket,
            "GET /bucket/allowed/file HTTP/1.1\r\nHost: ignored\r\nRange: bytes=2-4\r\n\r\n",
        )
        .await;
        let text = String::from_utf8(response).unwrap();
        assert!(text.starts_with("HTTP/1.1 206"));
        assert!(text.ends_with("cde"));
        assert_eq!(broker.bytes_read(), 3);
        for target in [
            "/bucket/other/file",
            "/bucket/allowed%2F..%2Fother/file",
            "/bucket/allowedness/file",
            "/wrong/allowed/file",
        ] {
            let response = request(&socket, &format!("GET {target} HTTP/1.1\r\n\r\n")).await;
            assert!(
                !response.starts_with(b"HTTP/1.1 200") && !response.starts_with(b"HTTP/1.1 206")
            );
        }
        let response = request(&socket, "PUT /bucket/allowed/file HTTP/1.1\r\n\r\n").await;
        assert!(response.starts_with(b"HTTP/1.1 403"));
        broker.clear().await;
        let response = request(&socket, "GET /bucket/allowed/file HTTP/1.1\r\n\r\n").await;
        assert!(response.starts_with(b"HTTP/1.1 403"));
        drop(broker);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    /// Reads one response off an open connection: the header, then exactly Content-Length bytes.
    async fn one_response(stream: &mut UnixStream) -> (String, Vec<u8>) {
        let mut head = Vec::new();
        let mut byte = [0_u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut body = vec![0_u8; length];
        stream.read_exact(&mut body).await.unwrap();
        (head, body)
    }

    #[tokio::test]
    async fn repeat_reads_come_from_memory_and_one_connection_serves_many_requests() {
        let dir = std::env::temp_dir().join(format!("loamz-broker-cache-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let broker = Broker::start(&dir).unwrap();
        let store = Arc::new(InMemory::new());
        store
            .put(&ObjectPath::from("allowed/file"), "abcdefghij".into())
            .await
            .unwrap();
        let generation = broker.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *broker.access.write().await = Some(Arc::new(Grant {
            immutable: true,
            bucket: "bucket".into(),
            prefixes: vec!["allowed/".into()],
            store: store.clone(),
            generation,
        }));
        let mut stream = UnixStream::connect(dir.join("socket")).await.unwrap();
        let ask = "GET /bucket/allowed/file HTTP/1.1\r\nRange: bytes=2-4\r\n\r\n";
        stream.write_all(ask.as_bytes()).await.unwrap();
        let (head, body) = one_response(&mut stream).await;
        assert!(head.starts_with("HTTP/1.1 206") && head.contains("Connection: keep-alive"));
        assert_eq!(body, b"cde");
        assert_eq!(broker.cache_hits(), 0);

        // The object is gone from the store, and the same connection still gets the answer: it came from memory.
        store
            .delete(&ObjectPath::from("allowed/file"))
            .await
            .unwrap();
        stream.write_all(ask.as_bytes()).await.unwrap();
        let (_, again) = one_response(&mut stream).await;
        assert_eq!(again, b"cde");
        // A different range of the same small object is also served from memory.
        let other = "GET /bucket/allowed/file HTTP/1.1\r\nRange: bytes=0-1\r\n\r\n";
        stream.write_all(other.as_bytes()).await.unwrap();
        let (_, first) = one_response(&mut stream).await;
        assert_eq!(first, b"ab");
        assert_eq!(broker.cache_hits(), 2);

        // Memory never widens the lease: a folder it does not name is refused, cached or not.
        let mut outside = UnixStream::connect(dir.join("socket")).await.unwrap();
        outside
            .write_all(b"GET /bucket/other/file HTTP/1.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut refused = Vec::new();
        outside.read_to_end(&mut refused).await.unwrap();
        assert!(refused.starts_with(b"HTTP/1.1 403"));
        drop(stream);
        drop(broker);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
