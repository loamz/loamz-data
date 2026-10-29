//! Starts a process, waits for it, and kills it hard when time is up. The sandbox backend is only a different command:
//! `runsc` for gVisor, or `bwrap` for trusted internal use.

use std::{process::Stdio, time::Duration};

use tokio::{io::AsyncReadExt, process::Command};

#[derive(Debug)]
pub struct Outcome {
    /// `None` if it was killed.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// How much of each output stream is kept. A program that prints forever cannot fill the server's memory.
pub const OUTPUT_LIMIT: u64 = 1 << 20;

/// Runs `cmd`. After `timeout` it is killed (SIGKILL, no chance to ignore it). `cleanup` runs after a kill, for a
/// runtime that must be told to remove its container (`runsc delete --force`).
///
/// # Errors
/// The process could not be started.
pub async fn run(
    mut cmd: Command,
    timeout: Duration,
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
    if let Ok(done) = tokio::time::timeout(timeout, work).await {
        let (status, stdout, stderr) = done?;
        return Ok(Outcome {
            exit_code: status.code(),
            timed_out: false,
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
        timed_out: true,
        stdout: Vec::new(),
        stderr: Vec::new(),
    })
}

/// The command line that runs a bundle under gVisor. `id` names the container so it can be removed after a kill.
#[must_use]
pub fn gvisor(runsc: &std::path::Path, bundle: &std::path::Path, id: &str) -> (Command, Command) {
    let mut run = Command::new(runsc);
    run.args(["--network=none", "--rootless", "run", "--bundle"])
        .arg(bundle)
        .arg(id);
    let mut clean = Command::new(runsc);
    clean.args(["delete", "--force"]).arg(id);
    (run, clean)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_finished_program_returns_its_code_and_output() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo hello; echo oops >&2; exit 3"]);
        let o = run(c, Duration::from_secs(5), None).await.unwrap();
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
        let o = run(c, Duration::from_millis(300), None).await.unwrap();
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
        run(main, Duration::from_millis(200), Some(clean))
            .await
            .unwrap();
        assert!(marker.exists());
        let _ = std::fs::remove_file(&marker);
    }

    #[tokio::test]
    async fn a_program_that_prints_forever_is_cut_off_at_the_limit() {
        let mut c = Command::new("yes");
        c.arg("spam");
        let o = run(c, Duration::from_millis(500), None).await.unwrap();
        assert!(
            o.stdout.len() as u64 <= OUTPUT_LIMIT,
            "kept {} bytes",
            o.stdout.len()
        );
    }

    #[test]
    fn the_gvisor_command_has_no_network_and_a_named_container() {
        let (run, clean) = gvisor("/usr/bin/runsc".as_ref(), "/b".as_ref(), "job-1");
        let args: Vec<_> = run
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--network=none",
                "--rootless",
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
        assert_eq!(cleanup, ["delete", "--force", "job-1"]);
    }
}
