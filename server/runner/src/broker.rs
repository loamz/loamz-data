//! A read-only, per-run object broker reached through one private Unix socket.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures::StreamExt;
use object_store::{
    GetOptions, GetRange, ObjectStore, ObjectStoreExt, aws::AmazonS3Builder,
    path::Path as ObjectPath,
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
}

/// The broker never accepts a caller-supplied endpoint or credentials from a socket request.
pub struct Broker {
    access: Arc<RwLock<Option<Arc<Grant>>>>,
    generation: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
    task: JoinHandle<()>,
    requests: Arc<Mutex<Vec<tokio::task::AbortHandle>>>,
    socket: PathBuf,
}

impl Broker {
    pub fn start(dir: &Path) -> std::io::Result<Self> {
        let socket = dir.join("socket");
        let listener = UnixListener::bind(&socket)?;
        let access = Arc::new(RwLock::new(None));
        let generation = Arc::new(AtomicU64::new(0));
        let bytes = Arc::new(AtomicU64::new(0));
        let requests = Arc::new(Mutex::new(Vec::<tokio::task::AbortHandle>::new()));
        let task = tokio::spawn({
            let (access, generation, bytes) = (
                Arc::clone(&access),
                Arc::clone(&generation),
                Arc::clone(&bytes),
            );
            let requests = Arc::clone(&requests);
            async move {
                let permits = Arc::new(tokio::sync::Semaphore::new(32));
                while let Ok((stream, _)) = listener.accept().await {
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                        continue;
                    };
                    let (access, generation, bytes) = (
                        Arc::clone(&access),
                        Arc::clone(&generation),
                        Arc::clone(&bytes),
                    );
                    if let Ok(mut requests) = requests.lock() {
                        let handle = tokio::spawn(async move {
                            let _permit = permit;
                            let _ = tokio::time::timeout(
                                Duration::from_mins(5),
                                serve(stream, access, generation, bytes),
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
        let field = |key: &str| {
            storage[key]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or("the storage access is incomplete")
        };
        let bucket = field("bucket")?.to_owned();
        let endpoint = field("endpoint")?;
        let store = AmazonS3Builder::new()
            .with_endpoint(endpoint)
            .with_bucket_name(&bucket)
            .with_access_key_id(field("key_id")?)
            .with_secret_access_key(field("secret")?)
            .with_token(field("session_token")?)
            .with_region("auto")
            .with_allow_http(endpoint.starts_with("http://"))
            .build()
            .map_err(|_| "the storage access could not be prepared")?;
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
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.access.write().await = Some(Arc::new(Grant {
            bucket,
            prefixes,
            store: Arc::new(store),
            generation,
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
) -> Result<(Option<GetRange>, Option<String>), u16> {
    let (mut range, mut if_match) = (None, None);
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
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return Err(400);
        }
    }
    Ok((range, if_match))
}

async fn serve(
    mut stream: UnixStream,
    access: Arc<RwLock<Option<Arc<Grant>>>>,
    generation: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
) -> std::io::Result<()> {
    let Ok(raw) = read_header(&mut stream).await else {
        return error(&mut stream, 400, "Bad Request").await;
    };
    let Ok(header) = std::str::from_utf8(&raw) else {
        return error(&mut stream, 400, "Bad Request").await;
    };
    let mut lines = header.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (Some(method), Some(target), Some("HTTP/1.1"), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        return error(&mut stream, 400, "Bad Request").await;
    };
    if !matches!(method, "GET" | "HEAD") || target.contains('?') {
        return error(&mut stream, 403, "Forbidden").await;
    }
    let Some(grant) = access.read().await.clone() else {
        return error(&mut stream, 403, "Forbidden").await;
    };
    let Some((bucket, key)) = target.strip_prefix('/').and_then(|v| v.split_once('/')) else {
        return error(&mut stream, 400, "Bad Request").await;
    };
    let Ok(path) = ObjectPath::from_url_path(key) else {
        return error(&mut stream, 400, "Bad Request").await;
    };
    if bucket != grant.bucket
        || !grant
            .prefixes
            .iter()
            .any(|prefix| path.as_ref().starts_with(prefix))
    {
        return error(&mut stream, 403, "Forbidden").await;
    }
    let (range, if_match) = match parse_headers(lines) {
        Ok(parsed) => parsed,
        Err(416) => return error(&mut stream, 416, "Range Not Satisfiable").await,
        Err(_) => return error(&mut stream, 400, "Bad Request").await,
    };
    if method == "HEAD" {
        let Ok(meta) = grant.store.head(&path).await else {
            return error(&mut stream, 404, "Not Found").await;
        };
        if generation.load(Ordering::SeqCst) != grant.generation {
            return error(&mut stream, 403, "Forbidden").await;
        }
        let etag = etag_header(meta.e_tag.as_deref());
        return stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{etag}Accept-Ranges: bytes\r\nConnection: close\r\n\r\n", meta.size).as_bytes()).await;
    }
    let ranged = range.is_some();
    let opts = GetOptions {
        range,
        if_match,
        ..Default::default()
    };
    let Ok(result) = grant.store.get_opts(&path, opts).await else {
        return error(&mut stream, 404, "Not Found").await;
    };
    if generation.load(Ordering::SeqCst) != grant.generation {
        return error(&mut stream, 403, "Forbidden").await;
    }
    let length = result.range.end.saturating_sub(result.range.start);
    let content_range = if ranged {
        format!(
            "Content-Range: bytes {}-{}/{}\r\n",
            result.range.start,
            result.range.end.saturating_sub(1),
            result.meta.size
        )
    } else {
        String::new()
    };
    let status = if ranged {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let etag = etag_header(result.meta.e_tag.as_deref());
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {length}\r\n{content_range}{etag}Accept-Ranges: bytes\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut body = result.into_stream();
    while let Some(chunk) = body.next().await {
        if generation.load(Ordering::SeqCst) != grant.generation {
            break;
        }
        let chunk = chunk.map_err(std::io::Error::other)?;
        stream.write_all(&chunk).await?;
        bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    Ok(())
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
}
