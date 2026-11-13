//! One piece of work, from the lease to the report: fetch what it declared through the Data API with the run's own token, run it
//! in a sandbox, and say how it ended. The token is the person's rights narrowed to what the work declared, so this machine can
//! read and write nothing else, and the code in the sandbox never sees it (it has no network and no credential).

use std::{
    future::Future,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    check::verify,
    exec::{Gvisor, prepare_workspace, private_dir, run},
    spec::{Sandbox, User, oci_config},
};

/// What the machine has: the sandbox runtime, a root filesystem with Python (and `DuckDB`), and a place to work.
#[derive(Clone)]
pub struct Machine {
    pub gvisor: Gvisor,
    pub rootfs: PathBuf,
    pub work: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct Lease {
    pub run: RunInfo,
    pub token: String,
    pub limits: Limits,
    /// How to read the run's tables: credentials limited to their folders, and where each one's current metadata is.
    #[serde(default)]
    pub storage: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct RunInfo {
    pub id: String,
    pub kind: String,
    pub org_id: String,
    pub workspace_id: String,
    pub spec: Value,
}

#[derive(Debug, Deserialize)]
pub struct Limits {
    pub memory_mb: u64,
    pub cpu_millis: u64,
    pub timeout_s: u64,
}

/// How a run ended, as the Data server wants to hear it.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub state: String,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub result: String,
    pub error: String,
}

impl Report {
    fn failed(error: impl Into<String>) -> Self {
        Self {
            state: "failed".into(),
            exit_code: -1,
            error: error.into(),
            ..Self::default()
        }
    }
}

/// A path from the server, made safe to put under a folder of ours: plain components only.
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let mut out = base.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(part) => out.push(part),
            _ => return Err(format!("the server sent an unsafe path {rel:?}")),
        }
    }
    if out == base {
        return Err("the server sent an empty path".into());
    }
    Ok(out)
}

/// The most that is fetched or sent, so a mistake cannot fill the disk.
const MAX_FILE: u64 = 256 << 20;
const MAX_FILES: usize = 200;
/// The most one table may weigh on this machine.
const MAX_TABLE: u64 = 1 << 30;
const MAX_RESULT: u64 = 4 << 20;

/// Bound listing as it is read, rather than collecting an arbitrarily large bucket response first.
async fn table_objects(
    store: &dyn object_store::ObjectStore,
    prefix: &object_store::path::Path,
    id: &str,
) -> Result<Vec<object_store::ObjectMeta>, String> {
    use futures::StreamExt;
    let mut list = store.list(Some(prefix));
    let mut objects = Vec::new();
    let mut bytes = 0_u64;
    while let Some(item) = list.next().await {
        let object = item.map_err(|e| format!("listing the table {id}: {e}"))?;
        if objects.len() == MAX_FILES {
            return Err(format!("{id} has too many files for one run"));
        }
        bytes = bytes
            .checked_add(object.size)
            .ok_or_else(|| format!("{id} is too large to read on this machine"))?;
        if bytes > MAX_TABLE {
            return Err(format!("{id} is too large to read on this machine"));
        }
        objects.push(object);
    }
    Ok(objects)
}

/// The Data API, as one run's token sees it.
struct Api {
    http: reqwest::Client,
    base: String,
    workspace: String,
    token: String,
}

impl Api {
    fn url(&self, holder: &str, id: &str, path: Option<&str>) -> Result<reqwest::Url, String> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/api/v1/workspaces/{}/{holder}/{id}/files",
            self.base.trim_end_matches('/'),
            self.workspace
        ))
        .map_err(|e| format!("the Data address is not valid: {e}"))?;
        if let Some(p) = path {
            let mut segs = url
                .path_segments_mut()
                .map_err(|()| "the Data address cannot be a base".to_owned())?;
            segs.extend(p.split('/'));
        }
        Ok(url)
    }

    async fn list(&self, holder: &str, id: &str) -> Result<Vec<String>, String> {
        let r = self
            .http
            .get(self.url(holder, id, None)?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("listing the files of {id}: {}", r.status()));
        }
        let v: Value = r.json().await.map_err(|e| e.to_string())?;
        Ok(v["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|i| i["path"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn download(&self, holder: &str, id: &str, path: &str, to: &Path) -> Result<(), String> {
        let mut r = self
            .http
            .get(self.url(holder, id, Some(path))?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("reading {path}: {}", r.status()));
        }
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        let mut file = tokio::fs::File::create(to)
            .await
            .map_err(|e| e.to_string())?;
        let mut total = 0u64;
        while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
            total += chunk.len() as u64;
            if total > MAX_FILE {
                return Err(format!("{path} is larger than the limit"));
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    async fn upload(
        &self,
        holder: &str,
        id: &str,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        let r = self
            .http
            .put(self.url(holder, id, Some(path))?)
            .bearer_auth(&self.token)
            .body(bytes)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(format!("writing {path}: {}", r.status()))
        }
    }

    /// Fetches every file of a table or volume into `folder`.
    async fn fetch(&self, holder: &str, id: &str, folder: &Path) -> Result<(), String> {
        tokio::fs::create_dir_all(folder)
            .await
            .map_err(|e| e.to_string())?;
        let files = self.list(holder, id).await?;
        if files.len() > MAX_FILES {
            return Err(format!("{id} has too many files for one run"));
        }
        for path in &files {
            self.download(holder, id, path, &safe_join(folder, path)?)
                .await?;
        }
        Ok(())
    }
}

/// Fetches a table's whole folder (its Iceberg metadata and data files) straight from the bucket, with the run's own read-only
/// credentials, and writes where its current metadata is beside it. The sandbox then reads the folder from disk.
async fn fetch_table(storage: &Value, id: &str, folder: &Path) -> Result<(), String> {
    use futures::StreamExt;
    use object_store::{ObjectStoreExt, aws::AmazonS3Builder, path::Path as Key};
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let text = |k: &str| {
        storage[k]
            .as_str()
            .ok_or_else(|| format!("the lease has no storage {k}"))
    };
    let endpoint = text("endpoint")?;
    let s3 = AmazonS3Builder::new()
        .with_endpoint(endpoint)
        // A local store (RustFS in development) is plain http.
        .with_allow_http(endpoint.starts_with("http://"))
        .with_bucket_name(text("bucket")?)
        .with_access_key_id(text("key_id")?)
        .with_secret_access_key(text("secret")?)
        .with_token(text("session_token")?)
        .with_region("auto")
        .build()
        .map_err(|e| e.to_string())?;
    let at = &storage["tables"][id];
    let metadata = at["metadata"]
        .as_str()
        .ok_or_else(|| format!("the lease does not say where {id} is"))?;
    // The table's folder in the bucket: `<workspace>/<catalog>/<id>`.
    let remote = at["folder"]
        .as_str()
        .ok_or_else(|| format!("the lease does not say which folder {id} is in"))?;
    let prefix = Key::from(remote);
    let objects = table_objects(&s3, &prefix, id).await?;
    tokio::fs::create_dir_all(folder)
        .await
        .map_err(|e| e.to_string())?;
    let mut downloaded = 0_u64;
    for o in objects {
        let rel = o
            .location
            .as_ref()
            .strip_prefix(&format!("{remote}/"))
            .ok_or("the bucket listed something outside the table")?;
        let to = safe_join(folder, rel)?;
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        let mut stream = s3
            .get(&o.location)
            .await
            .map_err(|e| format!("reading {rel}: {e}"))?
            .into_stream();
        let mut file = tokio::fs::File::create(&to)
            .await
            .map_err(|e| e.to_string())?;
        let mut object_bytes = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("reading {rel}: {e}"))?;
            let n = u64::try_from(chunk.len()).map_err(|e| e.to_string())?;
            downloaded = downloaded.checked_add(n).ok_or("table size overflow")?;
            object_bytes = object_bytes.checked_add(n).ok_or("file size overflow")?;
            if downloaded > MAX_TABLE || object_bytes > o.size {
                return Err(format!(
                    "{id} changed or exceeded its size limit while downloading"
                ));
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| e.to_string())?;
        }
        if object_bytes != o.size {
            return Err(format!("{id} changed while downloading"));
        }
    }
    tokio::fs::write(
        folder.join("loamz-table.json"),
        serde_json::json!({"metadata": metadata}).to_string(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Every regular file under `dir` (links are skipped), as paths relative to it.
async fn files_under(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut rd = tokio::fs::read_dir(&d).await.map_err(|e| e.to_string())?;
        while let Some(e) = rd.next_entry().await.map_err(|e| e.to_string())? {
            let t = e.file_type().await.map_err(|e| e.to_string())?;
            if t.is_dir() {
                stack.push(e.path());
            } else if let (true, Ok(rel)) = (t.is_file(), e.path().strip_prefix(dir)) {
                if out.len() == MAX_FILES {
                    return Err("the job produced too many files".into());
                }
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// What runs inside the sandbox to answer a SQL statement. It is part of this program, not something a person supplies.
pub const RUN_SQL: &str = include_str!("sandbox/run_sql.py");
/// What `RUN_SQL` imports: how rows become JSON, shared with serverless endpoints.
pub const LOAMZ_SQL: &str = include_str!("sandbox/loamz_sql.py");

/// Does the work, whatever happens to it, and says how it ended. The run's folder is removed afterwards.
pub async fn execute(
    lease: &Lease,
    machine: &Machine,
    data_url: &str,
    stop: impl Future<Output = ()>,
) -> Report {
    let dir = machine.work.join(&lease.run.id);
    let report = match go(lease, machine, data_url, &dir, stop).await {
        Ok(r) => r,
        Err(e) => Report::failed(e),
    };
    let _ = tokio::fs::remove_dir_all(&dir).await;
    report
}

/// What staging leaves ready to run: the folders to mount, and which script in `/inputs/job` to run.
struct Staged {
    mounts: Vec<(String, PathBuf)>,
    script: &'static str,
}

async fn go(
    lease: &Lease,
    machine: &Machine,
    data_url: &str,
    dir: &Path,
    stop: impl Future<Output = ()>,
) -> Result<Report, String> {
    let io = |e: std::io::Error| e.to_string();
    private_dir(&machine.work).map_err(io)?;
    private_dir(dir).map_err(io)?;
    let api = Api {
        http: reqwest::Client::builder()
            .timeout(Duration::from_mins(5))
            .build()
            .map_err(|e| e.to_string())?,
        base: data_url.to_owned(),
        workspace: lease.run.workspace_id.clone(),
        token: lease.token.clone(),
    };
    let staged = stage(lease, &api, dir).await?;
    let workspace = dir.join("workspace");
    let report = sandboxed(lease, machine, dir, &workspace, staged, stop).await?;
    // Only a run that ended well has anything to collect.
    if report.state == "running" {
        collect(lease, &api, &workspace, report).await
    } else {
        Ok(report)
    }
}

/// Fetches what the work declared, through the API with its own token, and writes the code to run beside it.
async fn stage(lease: &Lease, api: &Api, dir: &Path) -> Result<Staged, String> {
    let io = |e: std::io::Error| e.to_string();
    let spec = &lease.run.spec;
    let code = dir.join("job");
    tokio::fs::create_dir_all(&code).await.map_err(io)?;
    let mut mounts = vec![("job".to_owned(), code.clone())];
    if lease.run.kind == "sql" {
        let tables = dir.join("tables");
        tokio::fs::create_dir_all(&tables).await.map_err(io)?;
        for t in spec["tables"].as_array().into_iter().flatten() {
            let id = t["id"].as_str().ok_or("a table has no id")?;
            let storage = lease.storage.as_ref().ok_or(
                "this server cannot give this machine access to data: its storage is not a bucket",
            )?;
            fetch_table(storage, id, &safe_join(&tables, id)?).await?;
        }
        let run = serde_json::json!({"sql": spec["sql"], "limit": spec["limit"], "tables": spec["tables"], "memory_mb": lease.limits.memory_mb});
        tokio::fs::write(code.join("spec.json"), run.to_string())
            .await
            .map_err(io)?;
        tokio::fs::write(code.join("run_sql.py"), RUN_SQL)
            .await
            .map_err(io)?;
        tokio::fs::write(code.join("loamz_sql.py"), LOAMZ_SQL)
            .await
            .map_err(io)?;
        mounts.push(("tables".to_owned(), tables));
        return Ok(Staged {
            mounts,
            script: "run_sql.py",
        });
    }
    for i in spec["inputs"].as_array().into_iter().flatten() {
        let (id, name) = (
            i["id"].as_str().ok_or("an input has no id")?,
            i["name"].as_str().ok_or("an input has no name")?,
        );
        let folder = dir.join("inputs").join(name);
        if i["kind"] == "table" {
            let storage = lease.storage.as_ref().ok_or(
                "this server cannot give this machine access to data: its storage is not a bucket",
            )?;
            fetch_table(storage, id, &folder).await?;
        } else {
            api.fetch("volumes", id, &folder).await?;
        }
        mounts.push((name.to_owned(), folder));
    }
    tokio::fs::write(
        code.join("main.py"),
        spec["script"].as_str().ok_or("the job has no script")?,
    )
    .await
    .map_err(io)?;
    Ok(Staged {
        mounts,
        script: "main.py",
    })
}

/// Runs the staged work in a sandbox whose limits are written into its own config and checked before anything runs, with a hard
/// stop on time and on being asked to. A run that exited cleanly comes back as `running`, meaning "its results are still to collect".
async fn sandboxed(
    lease: &Lease,
    machine: &Machine,
    dir: &Path,
    workspace: &Path,
    staged: Staged,
    stop: impl Future<Output = ()>,
) -> Result<Report, String> {
    let io = |e: std::io::Error| e.to_string();
    let user = if machine.gvisor.rootless {
        User::MappedRoot
    } else {
        User::Nobody
    };
    prepare_workspace(workspace, user).map_err(io)?;
    let sandbox = Sandbox {
        rootfs: machine.rootfs.clone(),
        inputs: staged.mounts,
        workspace: workspace.to_path_buf(),
        args: vec!["python3".into(), format!("/inputs/job/{}", staged.script)],
        memory_bytes: lease.limits.memory_mb << 20,
        pids_max: 256,
        cpu_millis: lease.limits.cpu_millis,
        user,
    };
    let config = oci_config(&sandbox)?;
    verify(&config, user)
        .map_err(|why| format!("the sandbox config is missing promises: {}", why.join("; ")))?;
    let bundle = dir.join("bundle");
    tokio::fs::create_dir_all(&bundle).await.map_err(io)?;
    tokio::fs::write(bundle.join("config.json"), config.to_string())
        .await
        .map_err(io)?;

    let (cmd, clean) = machine.gvisor.commands(&bundle, &lease.run.id);
    let out = run(
        cmd,
        Duration::from_secs(lease.limits.timeout_s),
        stop,
        Some(clean),
    )
    .await
    .map_err(|e| format!("could not start the sandbox: {e}"))?;
    let mut report = Report {
        state: "failed".into(),
        exit_code: out.exit_code.map_or(-1, i64::from),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        ..Report::default()
    };
    if out.cancelled {
        "cancelled".clone_into(&mut report.state);
    } else if out.timed_out {
        "timed_out".clone_into(&mut report.state);
        report.error = format!("it did not finish in {} seconds", lease.limits.timeout_s);
    } else if out.exit_code == Some(0) {
        "running".clone_into(&mut report.state);
    } else {
        report.error = format!(
            "the program exited with {}",
            out.exit_code.map_or("a signal".into(), |c| c.to_string())
        );
    }
    Ok(report)
}

/// A statement's rows come back in the report; a job's files go to its output volume.
async fn collect(
    lease: &Lease,
    api: &Api,
    workspace: &Path,
    mut report: Report,
) -> Result<Report, String> {
    let io = |e: std::io::Error| e.to_string();
    if lease.run.kind == "sql" {
        let path = workspace.join("result.json");
        let size = tokio::fs::metadata(&path)
            .await
            .map_err(|_| "the statement produced no result".to_owned())?
            .len();
        if size > MAX_RESULT {
            "failed".clone_into(&mut report.state);
            "the result is too large".clone_into(&mut report.error);
            return Ok(report);
        }
        let text = tokio::fs::read_to_string(&path).await.map_err(io)?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| format!("the result is not JSON: {e}"))?;
        if let Some(e) = v["error"].as_str() {
            "failed".clone_into(&mut report.state);
            e.clone_into(&mut report.error);
        } else {
            "succeeded".clone_into(&mut report.state);
            report.result = text;
        }
        return Ok(report);
    }
    let output = lease.run.spec["output"]
        .as_str()
        .ok_or("the job has no output volume")?;
    let files = files_under(workspace).await?;
    let mut total = 0_u64;
    for rel in &files {
        let path = workspace.join(rel);
        let size = tokio::fs::metadata(&path).await.map_err(io)?.len();
        if size > MAX_FILE {
            return Err(format!("an output file exceeds the {MAX_FILE} byte limit"));
        }
        total = total.checked_add(size).ok_or("job output size overflow")?;
        if total > MAX_TABLE {
            return Err(format!("the job output exceeds the {MAX_TABLE} byte limit"));
        }
    }
    for rel in files {
        let path = workspace.join(&rel);
        api.upload(
            "volumes",
            output,
            &format!("runs/{}/{rel}", lease.run.id),
            tokio::fs::read(&path).await.map_err(io)?,
        )
        .await?;
    }
    "succeeded".clone_into(&mut report.state);
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn table_listing_stops_when_it_exceeds_the_file_limit() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as Key};
        let store = InMemory::new();
        let prefix = Key::from("table");
        for i in 0..MAX_FILES {
            store
                .put(&Key::from(format!("table/{i}")), "x".into())
                .await
                .unwrap();
        }
        assert_eq!(
            table_objects(&store, &prefix, "table").await.unwrap().len(),
            MAX_FILES
        );
        store
            .put(&Key::from("table/extra"), "x".into())
            .await
            .unwrap();
        assert!(
            table_objects(&store, &prefix, "table")
                .await
                .unwrap_err()
                .contains("too many files")
        );
    }

    #[tokio::test]
    async fn too_many_outputs_fail_instead_of_silently_dropping_files() {
        let dir = std::env::temp_dir().join(format!(
            "loamz-output-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        for i in 0..=MAX_FILES {
            tokio::fs::write(dir.join(format!("{i}")), b"x")
                .await
                .unwrap();
        }
        let result = files_under(&dir).await;
        tokio::fs::remove_dir_all(&dir).await.unwrap();
        assert!(result.unwrap_err().contains("too many files"));
    }

    #[test]
    fn paths_from_the_server_stay_under_the_folder() {
        let base = Path::new("/w/inputs/sales");
        assert_eq!(
            safe_join(base, "a/b.csv").unwrap(),
            Path::new("/w/inputs/sales/a/b.csv")
        );
        for bad in ["../x", "a/../../x", "/etc/passwd", "", "."] {
            assert!(safe_join(base, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn urls_encode_each_part_of_a_path() {
        let api = Api {
            http: reqwest::Client::new(),
            base: "http://h:1/".into(),
            workspace: "w".into(),
            token: String::new(),
        };
        assert_eq!(
            api.url("tables", "t1", Some("a b/c#d.csv"))
                .unwrap()
                .as_str(),
            "http://h:1/api/v1/workspaces/w/tables/t1/files/a%20b/c%23d.csv"
        );
    }

    #[test]
    fn a_lease_is_read_as_the_server_writes_it() {
        let lease: Lease = serde_json::from_str(r#"{"run":{"id":"r","kind":"sql","org_id":"o","workspace_id":"w","spec":{"sql":"select 1"}},"token":"jt_x","limits":{"memory_mb":512,"cpu_millis":1000,"timeout_s":30}}"#).unwrap();
        assert_eq!(
            (lease.run.kind.as_str(), lease.limits.timeout_s),
            ("sql", 30)
        );
        assert_eq!(lease.run.org_id, "o");
    }
}
