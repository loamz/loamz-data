//! Runs real code in a real gVisor sandbox. Skipped unless both are given:
//!   `LOAMZ_RUNSC=/path/to/runsc` and `LOAMZ_ROOTFS=/path/to/an/unpacked/python/image`.
//! Set `LOAMZ_PLATFORM=ptrace` where systrap cannot start (WSL2). Set `LOAMZ_ROOTLESS=1` when not root (then memory and process limits cannot be enforced and those tests are skipped).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::{path::PathBuf, time::Duration};

use loamz_runner::{
    check::verify,
    exec::{Gvisor, Outcome, run},
    spec::{Sandbox, oci_config},
};

struct Lab {
    gv: Gvisor,
    rootfs: PathBuf,
    dir: PathBuf,
    rootless: bool,
}

fn lab() -> Option<Lab> {
    let runsc = PathBuf::from(std::env::var("LOAMZ_RUNSC").ok()?);
    let rootfs = PathBuf::from(std::env::var("LOAMZ_ROOTFS").ok()?);
    let rootless = std::env::var("LOAMZ_ROOTLESS").is_ok();
    let dir = std::env::temp_dir().join(format!("loamz-runner-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    Some(Lab {
        gv: Gvisor {
            runsc,
            state: dir.join("state"),
            rootless,
            platform: std::env::var("LOAMZ_PLATFORM").unwrap_or_else(|_| "systrap".into()),
        },
        rootfs,
        dir,
        rootless,
    })
}

impl Lab {
    /// Runs python code with the given limits; returns the outcome and the workspace folder.
    async fn python(
        &self,
        name: &str,
        code: &str,
        memory: u64,
        pids: u64,
        seconds: u64,
    ) -> (Outcome, PathBuf) {
        let ws = self.dir.join(format!("{name}-ws"));
        std::fs::create_dir_all(&ws).unwrap();
        let sandbox = Sandbox {
            rootfs: self.rootfs.clone(),
            inputs: vec![],
            workspace: ws.clone(),
            args: vec!["python3".into(), "-c".into(), code.into()],
            memory_bytes: memory,
            pids_max: pids,
            cpu_millis: 1000,
        };
        let mut config = oci_config(&sandbox).unwrap();
        verify(&config).expect("the config passes the check before anything runs");
        if self.rootless {
            // runsc makes its own user namespace; inside it the code is root of nothing.
            config["process"]["user"] = serde_json::json!({"uid": 0, "gid": 0});
        }
        let bundle = self.dir.join(format!("{name}-bundle"));
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("config.json"), config.to_string()).unwrap();
        if std::env::var("LOAMZ_KEEP").is_ok() {
            std::fs::write(
                std::env::temp_dir().join(format!("{name}-config.json")),
                config.to_string(),
            )
            .unwrap();
        }
        let (cmd, clean) = self.gv.commands(&bundle, name);
        (
            run(cmd, Duration::from_secs(seconds), Some(clean))
                .await
                .unwrap(),
            ws,
        )
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn code_runs_and_writes_only_to_its_workspace() {
    let Some(lab) = lab() else {
        return println!("skipped: LOAMZ_RUNSC / LOAMZ_ROOTFS not set");
    };
    let code = "open('/workspace/out.txt','w').write('chart'); print(6*7)\ntry:\n open('/etc/evil','w')\nexcept OSError as e: print('root is read-only')";
    let (o, ws) = lab.python("write", code, 512 << 20, 64, 60).await;
    let text =
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.exit_code, Some(0), "{text}");
    assert!(
        text.contains("42") && text.contains("root is read-only"),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("out.txt")).unwrap(),
        "chart",
        "the workspace is how output leaves"
    );
}

#[tokio::test]
async fn code_has_no_network() {
    let Some(lab) = lab() else { return };
    let code = "import socket\ns=socket.socket(); s.settimeout(3)\ntry:\n s.connect(('1.1.1.1',53)); print('REACHED')\nexcept OSError as e: print('no network')";
    let (o, _) = lab.python("net", code, 512 << 20, 64, 60).await;
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        text.contains("no network") && !text.contains("REACHED"),
        "{text}"
    );
}

#[tokio::test]
async fn a_program_that_will_not_stop_is_killed_and_its_container_removed() {
    let Some(lab) = lab() else { return };
    let started = std::time::Instant::now();
    let (o, _) = lab
        .python("loop", "while True: pass", 512 << 20, 64, 4)
        .await;
    assert!(o.timed_out);
    assert!(started.elapsed() < Duration::from_secs(20));
    let left = std::process::Command::new(&lab.gv.runsc)
        .arg("--root")
        .arg(&lab.gv.state)
        .arg("list")
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&left.stdout).contains("loop"),
        "no container left behind"
    );
}

#[tokio::test]
async fn the_memory_limit_stops_a_program_that_asks_for_too_much() {
    let Some(lab) = lab() else { return };
    if lab.rootless {
        return println!("skipped: memory limits need root (runsc makes its own cgroup)");
    }
    let (o, _) = lab
        .python(
            "mem",
            "x = bytearray(900*1024*1024); print('GOT IT')",
            256 << 20,
            64,
            60,
        )
        .await;
    assert!(!String::from_utf8_lossy(&o.stdout).contains("GOT IT") && o.exit_code != Some(0));
}

#[tokio::test]
async fn the_process_limit_stops_a_fork_bomb() {
    let Some(lab) = lab() else { return };
    if lab.rootless {
        return println!("skipped: process limits need root");
    }
    let code = "import os,sys\nn=0\ntry:\n while n<500:\n  if os.fork()==0: import time; time.sleep(5); os._exit(0)\n  n+=1\nexcept OSError: pass\nprint('forks', n)";
    let (o, _) = lab.python("pids", code, 512 << 20, 16, 60).await;
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let forks: u32 = text
        .split_whitespace()
        .last()
        .and_then(|n| n.parse().ok())
        .unwrap_or(9999);
    assert!(forks <= 16, "{text}");
}
