//! Starts a process, waits for it, and kills it hard when time is up. The sandbox backend is only a different command:
//! `runsc` for gVisor, or `bwrap` for trusted internal use.

use std::{process::Stdio, time::Duration};

use tokio::{io::AsyncReadExt, process::Command};

/// A duration in whole milliseconds, for logs.
fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// A cancellation signal for one leased run.
pub type StopSignal = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// Both executors preserve the same lease and report boundary.
pub trait Executor {
    fn execute(
        &self,
        lease: &crate::work::Lease,
        stop: StopSignal,
    ) -> impl std::future::Future<Output = crate::work::Report> + Send;
}

/// Runs jobs in their existing one-shot gVisor sandbox.
pub struct SandboxExecutor {
    pub machine: crate::work::Machine,
    pub data_url: String,
}

impl Executor for SandboxExecutor {
    async fn execute(&self, lease: &crate::work::Lease, stop: StopSignal) -> crate::work::Report {
        crate::work::execute(lease, &self.machine, &self.data_url, stop).await
    }
}

/// Sends each statement to a persistent sandbox process; a protocol failure removes that process.
pub struct WarmExecutor<F> {
    pub pool: crate::pool::Pool,
    pub start: F,
}

impl<F, Fut> Executor for WarmExecutor<F>
where
    F: Fn(crate::pool::WorkerKey, crate::work::Limits) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = std::io::Result<crate::pool::Worker>> + Send,
{
    #[allow(clippy::too_many_lines)]
    async fn execute(&self, lease: &crate::work::Lease, stop: StopSignal) -> crate::work::Report {
        if lease.run.kind != "sql" {
            return crate::work::Report::failed("the warm worker only runs SQL");
        }
        let key = crate::pool::WorkerKey {
            org_id: lease.run.org_id.clone(),
            target_id: lease.run.target_id.clone(),
        };
        let Ok(mut worker) = self
            .pool
            .acquire(
                key.clone(),
                Some((lease.limits.memory_mb, lease.limits.cpu_millis)),
                || (self.start)(key.clone(), lease.limits),
            )
            .await
        else {
            return crate::work::Report::failed("could not start the SQL sandbox");
        };
        let began = std::time::Instant::now();
        if let Some(broker) = &worker.broker {
            if broker
                .authorize(lease.storage.as_ref(), &lease.run.spec)
                .await
                .is_err()
            {
                drop(worker);
                self.pool.invalidate(&key).await;
                return crate::work::Report::failed(
                    "could not prepare the SQL sandbox's storage access",
                );
            }
        } else if lease.run.spec["tables"]
            .as_array()
            .is_none_or(|tables| !tables.is_empty())
        {
            drop(worker);
            self.pool.invalidate(&key).await;
            return crate::work::Report::failed("the SQL sandbox has no storage broker");
        }
        let request = serde_json::json!({
            "spec": lease.run.spec,
            "storage": lease.storage,
            "limits": {"memory_mb": lease.limits.memory_mb, "cpu_millis": lease.limits.cpu_millis},
        });
        let work = async {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let mut line = serde_json::to_vec(&request).map_err(|_| ())?;
            line.push(b'\n');
            let input = worker.child.stdin.as_mut().ok_or(())?;
            input.write_all(&line).await.map_err(|_| ())?;
            input.flush().await.map_err(|_| ())?;
            let output = worker.child.stdout.as_mut().ok_or(())?;
            let mut reader = BufReader::new(output.take(4_194_305));
            let mut response = Vec::new();
            reader
                .read_until(b'\n', &mut response)
                .await
                .map_err(|_| ())?;
            if response.len() > 4_194_304 || !response.ends_with(b"\n") {
                return Err(());
            }
            serde_json::from_slice::<crate::work::Report>(&response).map_err(|_| ())
        };
        let authorized = began.elapsed();
        let outcome = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(lease.limits.timeout_s), work) => result.map_err(|_| "the SQL run timed out"),
            () = stop => Err("the SQL run was cancelled"),
        };
        tracing::info!(
            run = lease.run.id,
            authorize_ms = millis(authorized),
            worker_ms = millis(began.elapsed().saturating_sub(authorized)),
            "warm statement"
        );
        if let Some(broker) = &worker.broker {
            broker.clear().await;
        }
        match outcome {
            Ok(Ok(report)) => {
                let report = with_export(&worker, lease, report).await;
                worker.completed();
                report
            }
            outcome => {
                drop(worker);
                self.pool.invalidate(&key).await;
                let mut report = crate::work::Report::failed(match outcome {
                    Err(reason) => reason,
                    Ok(Err(())) => "the SQL sandbox failed",
                    Ok(Ok(_)) => unreachable!(),
                });
                if report.error == "the SQL run was cancelled" {
                    report.state = "cancelled".into();
                } else if report.error == "the SQL run timed out" {
                    report.state = "timed_out".into();
                }
                report
            }
        }
    }
}

/// A whole result asked for as a file is moved out of the sandbox's folder now, while the worker is still ours.
async fn with_export(
    worker: &crate::pool::Worker,
    lease: &crate::work::Lease,
    mut report: crate::work::Report,
) -> crate::work::Report {
    if report.state == "succeeded" && lease.run.spec["export"] == true {
        let extension = if lease.run.spec["export_format"] == "parquet" {
            "parquet"
        } else {
            "arrow"
        };
        match worker.take_export(&lease.run.id, extension).await {
            Some(path) => report.file = Some(path),
            None => {
                report = crate::work::Report::failed("the statement produced no result file");
            }
        }
    }
    report
}

#[derive(Debug)]
pub struct Outcome {
    /// `None` if it was killed.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Stopped because the caller asked (the person cancelled the run).
    pub cancelled: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// How much of each output stream is kept. A program that prints forever cannot fill the server's memory.
pub const OUTPUT_LIMIT: u64 = 1 << 20;

/// Runs `cmd`. It is killed (SIGKILL, no chance to ignore it) after `timeout`, or as soon as `stop` finishes. `cleanup` runs after a
/// kill, for a runtime that must be told to remove its container (`runsc delete --force`).
///
/// # Errors
/// The process could not be started.
pub async fn run(
    mut cmd: Command,
    timeout: Duration,
    stop: impl std::future::Future<Output = ()>,
    cleanup: Option<Command>,
) -> std::io::Result<Outcome> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let read = async {
        let mut a = Vec::new();
        let mut b = Vec::new();
        let _ = tokio::join!(
            async {
                if let Some(o) = out.as_mut() {
                    let _ = o.take(OUTPUT_LIMIT).read_to_end(&mut a).await;
                }
            },
            async {
                if let Some(e) = err.as_mut() {
                    let _ = e.take(OUTPUT_LIMIT).read_to_end(&mut b).await;
                }
            }
        );
        (a, b)
    };
    let work = async {
        let (a, b) = read.await;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((status, a, b))
    };
    let mut cancelled = false;
    let finished = tokio::select! {
        done = tokio::time::timeout(timeout, work) => done.ok(),
        () = stop => { cancelled = true; None }
    };
    if let Some(done) = finished {
        let (status, stdout, stderr) = done?;
        return Ok(Outcome {
            exit_code: status.code(),
            timed_out: false,
            cancelled: false,
            stdout,
            stderr,
        });
    }
    // `work` borrowed the child; it is dropped now, so the child can be killed and reaped.
    let _ = child.kill().await;
    if let Some(mut c) = cleanup {
        let _ = c
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    Ok(Outcome {
        exit_code: None,
        timed_out: !cancelled,
        cancelled,
        stdout: Vec::new(),
        stderr: Vec::new(),
    })
}

/// How to start gVisor. `state` is where `runsc` keeps its container records.
#[derive(Clone, Debug)]
pub struct Gvisor {
    pub runsc: std::path::PathBuf,
    pub state: std::path::PathBuf,
    /// Without root (a laptop, CI): runsc cannot make its own cgroup, so memory and process limits are not enforced.
    /// The runner reports that to the caller instead of pretending.
    pub rootless: bool,
    /// `systrap` (the default, fastest without KVM) or `ptrace`, for kernels where systrap cannot start (WSL2).
    pub platform: String,
}

impl Gvisor {
    fn base(&self) -> Command {
        let mut c = Command::new(&self.runsc);
        c.arg("--network=none")
            .arg(format!("--platform={}", self.platform))
            .arg("--root")
            .arg(&self.state);
        if self.rootless {
            c.args(["--rootless", "--ignore-cgroups"]);
        }
        c
    }

    /// The command that runs a bundle, and the one that removes its container after a kill. `id` names the container.
    #[must_use]
    pub fn commands(&self, bundle: &std::path::Path, id: &str) -> (Command, Command) {
        let mut run = self.base();
        run.arg("run").arg("--bundle").arg(bundle).arg(id);
        let mut clean = self.base();
        clean.args(["delete", "--force"]).arg(id);
        (run, clean)
    }

    /// Worker sandboxes have no network, but may open their one mounted broker socket.
    #[must_use]
    pub fn worker_commands(&self, bundle: &std::path::Path, id: &str) -> (Command, Command) {
        let mut run = self.base();
        run.arg("--host-uds=open")
            .arg("run")
            .arg("--bundle")
            .arg(bundle)
            .arg(id);
        let mut clean = self.base();
        clean.args(["delete", "--force"]).arg(id);
        (run, clean)
    }
}

/// Makes the workspace somewhere the sandbox's user can write its output, and nothing else can: owned by that user, and closed to
/// everyone else. `Nobody` needs the agent to be root (to hand the folder over); `MappedRoot` is the agent's own user already.
///
/// # Errors
/// The folder could not be created or prepared.
pub fn prepare_workspace(path: &std::path::Path, user: crate::spec::User) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path)?;
    if user == crate::spec::User::Nobody {
        std::os::unix::fs::chown(path, Some(user.uid()), Some(user.uid()))?;
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// A folder only the current user can enter.
///
/// # Errors
/// The folder could not be created.
pub fn private_dir(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Whether this process is root (the only way gVisor can hold a run to its memory and process limits).
#[must_use]
pub fn is_root() -> bool {
    std::fs::metadata("/proc/self").is_ok_and(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn warm_executor_reuses_a_worker_for_a_target() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let starts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&starts);
        let executor = WarmExecutor {
            pool: crate::pool::Pool::new(10, Duration::from_mins(1)),
            start: move |_key: crate::pool::WorkerKey, limits: crate::work::Limits| {
                counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    let mut command = Command::new("sh");
                    command.args(["-c", r#"while IFS= read -r line; do printf '%s\n' '{"state":"succeeded","exit_code":0,"stdout":"","stderr":"","result":"{}","error":""}'; done"#])
                        .stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true);
                    Ok(crate::pool::Worker::new(command.spawn()?, None)
                        .with_limits(limits.memory_mb, limits.cpu_millis))
                }
            },
        };
        let lease: crate::work::Lease = serde_json::from_str(r#"{"run":{"id":"r","kind":"sql","org_id":"o","target_id":"t","workspace_id":"w","spec":{"sql":"select 1","tables":[]}},"token":"","limits":{"memory_mb":512,"cpu_millis":1000,"timeout_s":3}}"#).unwrap();
        for _ in 0..2 {
            let report = executor
                .execute(&lease, Box::pin(std::future::pending()))
                .await;
            assert_eq!(report.state, "succeeded");
        }
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        executor
            .pool
            .invalidate(&crate::pool::WorkerKey {
                org_id: "o".into(),
                target_id: "t".into(),
            })
            .await;
    }

    #[tokio::test]
    #[ignore = "requires LOAMZ_TEST_WORKER_BIN with a built worker"]
    async fn warm_executor_runs_real_duckdb_requests() {
        let binary = std::env::var("LOAMZ_TEST_WORKER_BIN").unwrap();
        let executor = WarmExecutor {
            pool: crate::pool::Pool::new(10, Duration::from_mins(1)),
            start: move |_key: crate::pool::WorkerKey, limits: crate::work::Limits| {
                let binary = binary.clone();
                async move {
                    let mut command = Command::new(binary);
                    command
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .kill_on_drop(true);
                    Ok(crate::pool::Worker::new(command.spawn()?, None)
                        .with_limits(limits.memory_mb, limits.cpu_millis))
                }
            },
        };
        let lease: crate::work::Lease = serde_json::from_str(r#"{"run":{"id":"r","kind":"sql","org_id":"o","target_id":"t","workspace_id":"w","spec":{"sql":"select 42 as answer","limit":2,"tables":[]}},"token":"","limits":{"memory_mb":512,"cpu_millis":1000,"timeout_s":30}}"#).unwrap();
        for _ in 0..2 {
            let report = executor
                .execute(&lease, Box::pin(std::future::pending()))
                .await;
            assert_eq!(report.state, "succeeded", "{}", report.error);
            let answer: serde_json::Value = serde_json::from_str(&report.result).unwrap();
            assert_eq!(answer["rows"], serde_json::json!([[42]]));
        }
        executor
            .pool
            .invalidate(&crate::pool::WorkerKey {
                org_id: "o".into(),
                target_id: "t".into(),
            })
            .await;
    }

    #[tokio::test]
    async fn a_finished_program_returns_its_code_and_output() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo hello; echo oops >&2; exit 3"]);
        let o = run(c, Duration::from_secs(5), std::future::pending(), None)
            .await
            .unwrap();
        assert_eq!((o.exit_code, o.timed_out), (Some(3), false));
        assert_eq!(o.stdout, b"hello\n");
        assert_eq!(o.stderr, b"oops\n");
    }

    #[tokio::test]
    async fn a_program_that_will_not_stop_is_killed_on_time() {
        let mut c = Command::new("sh");
        // Ignores the polite signal, and would run for a minute.
        c.args(["-c", "trap '' TERM INT HUP; sleep 60"]);
        let started = std::time::Instant::now();
        let o = run(c, Duration::from_millis(300), std::future::pending(), None)
            .await
            .unwrap();
        assert!(o.timed_out && o.exit_code.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "it did not wait for the program"
        );
    }

    #[tokio::test]
    async fn the_cleanup_command_runs_after_a_kill() {
        let marker =
            std::env::temp_dir().join(format!("loamz-runner-cleanup-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let mut main = Command::new("sleep");
        main.arg("60");
        let mut clean = Command::new("touch");
        clean.arg(&marker);
        run(
            main,
            Duration::from_millis(200),
            std::future::pending(),
            Some(clean),
        )
        .await
        .unwrap();
        assert!(marker.exists());
        let _ = std::fs::remove_file(&marker);
    }

    #[tokio::test]
    async fn a_program_that_prints_forever_is_cut_off_at_the_limit() {
        let mut c = Command::new("yes");
        c.arg("spam");
        let o = run(c, Duration::from_millis(500), std::future::pending(), None)
            .await
            .unwrap();
        assert!(
            o.stdout.len() as u64 <= OUTPUT_LIMIT,
            "kept {} bytes",
            o.stdout.len()
        );
    }

    #[test]
    fn the_gvisor_command_has_no_network_and_a_named_container() {
        let g = Gvisor {
            runsc: "/usr/bin/runsc".into(),
            state: "/st".into(),
            rootless: false,
            platform: "systrap".into(),
        };
        let (run, clean) = g.commands("/b".as_ref(), "job-1");
        let args: Vec<_> = run
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--network=none",
                "--platform=systrap",
                "--root",
                "/st",
                "run",
                "--bundle",
                "/b",
                "job-1"
            ]
        );
        let cleanup: Vec<_> = clean
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            &cleanup[cleanup.len() - 3..],
            ["delete", "--force", "job-1"]
        );
    }
}
