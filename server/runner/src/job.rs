//! One job, run on a compute machine. The machine never holds a storage credential: it talks to the Data API with the
//! caller's short-lived token, so every read and write is checked and audited by governance like any other. The token
//! stays in this process; the code in the sandbox has no network and never sees it.

use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{
    check::verify,
    exec::{Gvisor, prepare_workspace, run},
    spec::{Sandbox, oci_config},
};

/// What to run and what it may have.
#[derive(Deserialize)]
pub struct Job {
    /// Where the Data API is, as seen from this machine (a tunnel to the laptop now, a Cloudflare address later).
    pub data_url: String,
    pub token: String,
    /// Volumes the code may read, each mounted read-only at `/inputs/<name>`.
    #[serde(default)]
    pub inputs: Vec<Input>,
    /// The volume that receives what the code leaves in `/workspace`, under `runs/<run id>/`.
    pub output_volume: String,
    /// Python source, run as `python3 /inputs/job/main.py`.
    pub script: String,
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u64,
    #[serde(default = "default_cpu")]
    pub cpu_millis: u64,
    #[serde(default = "default_pids")]
    pub pids: u64,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
}

impl std::fmt::Debug for Job {
    // Never print the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("data_url", &self.data_url)
            .field("token", &"<hidden>")
            .field("inputs", &self.inputs.len())
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
pub struct Input {
    pub volume: String,
    pub name: String,
}

fn default_memory_mb() -> u64 {
    2048
}
fn default_cpu() -> u64 {
    1000
}
fn default_pids() -> u64 {
    256
}
fn default_timeout() -> u64 {
    120
}

/// What the machine has: the sandbox runtime and a place to work.
pub struct Machine {
    pub gvisor: Gvisor,
    pub rootfs: PathBuf,
    pub work: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub run_id: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub downloaded: Vec<String>,
    pub uploaded: Vec<(String, u64)>,
    pub memory_mb: u64,
    pub cpu_millis: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("the job is not valid: {0}")]
    Invalid(String),
    #[error("the Data API refused or failed: {0}")]
    Api(String),
    #[error("{0}")]
    Local(String),
}

impl From<std::io::Error> for JobError {
    fn from(e: std::io::Error) -> Self {
        Self::Local(e.to_string())
    }
}

/// A path from the server, made safe to put under a folder of ours: plain components only.
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, JobError> {
    let mut out = base.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(part) => out.push(part),
            _ => {
                return Err(JobError::Api(format!(
                    "the server sent an unsafe path {rel:?}"
                )));
            }
        }
    }
    if out == base {
        return Err(JobError::Api("the server sent an empty path".into()));
    }
    Ok(out)
}

/// The most that is fetched or sent, so a mistake cannot fill the disk.
const MAX_FILE: u64 = 256 << 20;
const MAX_FILES: usize = 200;

struct Api {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Api {
    fn url(&self, volume: &str, path: Option<&str>) -> Result<reqwest::Url, JobError> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/api/v1/volumes/{volume}/files",
            self.base.trim_end_matches('/')
        ))
        .map_err(|e| JobError::Invalid(format!("data_url: {e}")))?;
        if let Some(p) = path {
            let mut segs = url
                .path_segments_mut()
                .map_err(|()| JobError::Invalid("data_url cannot be a base".into()))?;
            segs.extend(p.split('/'));
        }
        Ok(url)
    }

    async fn list(&self, volume: &str) -> Result<Vec<(String, u64)>, JobError> {
        let r = self
            .http
            .get(self.url(volume, None)?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| JobError::Api(e.to_string()))?;
        if !r.status().is_success() {
            return Err(JobError::Api(format!("listing {volume}: {}", r.status())));
        }
        let v: serde_json::Value = r.json().await.map_err(|e| JobError::Api(e.to_string()))?;
        Ok(v["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|i| Some((i["path"].as_str()?.to_owned(), i["size"].as_u64()?)))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn download(&self, volume: &str, path: &str, to: &Path) -> Result<(), JobError> {
        let mut r = self
            .http
            .get(self.url(volume, Some(path))?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| JobError::Api(e.to_string()))?;
        if !r.status().is_success() {
            return Err(JobError::Api(format!("reading {path}: {}", r.status())));
        }
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(to).await?;
        let mut total = 0u64;
        while let Some(chunk) = r.chunk().await.map_err(|e| JobError::Api(e.to_string()))? {
            total += chunk.len() as u64;
            if total > MAX_FILE {
                return Err(JobError::Api(format!("{path} is larger than the limit")));
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        }
        Ok(())
    }

    async fn upload(&self, volume: &str, path: &str, bytes: Vec<u8>) -> Result<(), JobError> {
        let r = self
            .http
            .put(self.url(volume, Some(path))?)
            .bearer_auth(&self.token)
            .body(bytes)
            .send()
            .await
            .map_err(|e| JobError::Api(e.to_string()))?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(JobError::Api(format!("writing {path}: {}", r.status())))
        }
    }
}

/// Every regular file under `dir` (links are skipped), as paths relative to it.
async fn files_under(dir: &Path) -> Result<Vec<String>, JobError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut rd = tokio::fs::read_dir(&d).await?;
        while let Some(e) = rd.next_entry().await? {
            let t = e.file_type().await?;
            if t.is_dir() {
                stack.push(e.path());
            } else if let (true, Ok(rel)) = (t.is_file(), e.path().strip_prefix(dir)) {
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Runs one job to the end. Whatever happens, the working folder is removed afterwards.
///
/// # Errors
/// The job is not valid, the Data API refused something, or the machine could not do its part. A program that fails
/// or is killed is not an error: that is an [`Outcome`].
pub async fn execute(job: Job, machine: &Machine) -> Result<Outcome, JobError> {
    let run_id = format!(
        "{:x}-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
        std::process::id()
    );
    let dir = machine.work.join(&run_id);
    let result = execute_in(&job, machine, &run_id, &dir).await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
    result
}

async fn execute_in(
    job: &Job,
    machine: &Machine,
    run_id: &str,
    dir: &Path,
) -> Result<Outcome, JobError> {
    let api = Api {
        http: reqwest::Client::builder()
            .timeout(Duration::from_mins(5))
            .build()
            .map_err(|e| JobError::Local(e.to_string()))?,
        base: job.data_url.clone(),
        token: job.token.clone(),
    };

    // 1. Inputs, through the API (so governance checks each read).
    let mut downloaded = Vec::new();
    let mut mounts = Vec::new();
    for input in &job.inputs {
        let folder = dir.join("inputs").join(&input.name);
        let listing = api.list(&input.volume).await?;
        if listing.len() > MAX_FILES {
            return Err(JobError::Api(format!(
                "{} has too many files for one job",
                input.name
            )));
        }
        for (path, _) in &listing {
            api.download(&input.volume, path, &safe_join(&folder, path)?)
                .await?;
            downloaded.push(format!("{}/{path}", input.name));
        }
        tokio::fs::create_dir_all(&folder).await?;
        mounts.push((input.name.clone(), folder));
    }
    let code_dir = dir.join("inputs").join("job");
    tokio::fs::create_dir_all(&code_dir).await?;
    tokio::fs::write(code_dir.join("main.py"), &job.script).await?;
    mounts.push(("job".to_owned(), code_dir));

    // 2. The sandbox, with its limits written into its own config and checked before anything runs.
    let workspace = dir.join("workspace");
    prepare_workspace(&workspace)?;
    let sandbox = Sandbox {
        rootfs: machine.rootfs.clone(),
        inputs: mounts,
        workspace: workspace.clone(),
        args: vec!["python3".into(), "/inputs/job/main.py".into()],
        memory_bytes: job.memory_mb << 20,
        pids_max: job.pids,
        cpu_millis: job.cpu_millis,
    };
    let mut config = oci_config(&sandbox).map_err(JobError::Invalid)?;
    verify(&config).map_err(|why| {
        JobError::Invalid(format!(
            "the sandbox config is missing promises: {}",
            why.join("; ")
        ))
    })?;
    // Rootless gVisor cannot map the sandbox user; it runs as root of its own empty user namespace instead.
    if machine.gvisor.rootless {
        config["process"]["user"] = serde_json::json!({"uid": 0, "gid": 0});
    }
    let bundle = dir.join("bundle");
    tokio::fs::create_dir_all(&bundle).await?;
    tokio::fs::write(bundle.join("config.json"), config.to_string()).await?;

    // 3. Run, with a hard stop.
    let (cmd, clean) = machine.gvisor.commands(&bundle, run_id);
    let out = run(cmd, Duration::from_secs(job.timeout_s), Some(clean)).await?;

    // 4. Outputs, through the API, under runs/<run id>/.
    let mut uploaded = Vec::new();
    for rel in files_under(&workspace).await?.into_iter().take(MAX_FILES) {
        let path = workspace.join(&rel);
        if tokio::fs::metadata(&path).await?.len() > MAX_FILE {
            continue;
        }
        let bytes = tokio::fs::read(&path).await?;
        let size = bytes.len() as u64;
        api.upload(&job.output_volume, &format!("runs/{run_id}/{rel}"), bytes)
            .await?;
        uploaded.push((format!("runs/{run_id}/{rel}"), size));
    }

    Ok(Outcome {
        run_id: run_id.to_owned(),
        exit_code: out.exit_code,
        timed_out: out.timed_out,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        downloaded,
        uploaded,
        memory_mb: job.memory_mb,
        cpu_millis: job.cpu_millis,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
    fn a_job_hides_its_token_when_printed() {
        let job: Job = serde_json::from_str(
            r#"{"data_url":"http://x","token":"SECRET","output_volume":"o","script":"print(1)"}"#,
        )
        .unwrap();
        assert!(!format!("{job:?}").contains("SECRET"));
        assert_eq!((job.memory_mb, job.cpu_millis), (2048, 1000));
    }

    #[test]
    fn urls_encode_each_part_of_a_path() {
        let api = Api {
            http: reqwest::Client::new(),
            base: "http://h:1/".into(),
            token: String::new(),
        };
        assert_eq!(
            api.url("v1", Some("a b/c#d.csv")).unwrap().as_str(),
            "http://h:1/api/v1/volumes/v1/files/a%20b/c%23d.csv"
        );
    }
}
