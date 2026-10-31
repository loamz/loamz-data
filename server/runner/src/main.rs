//! `loamz-runner agent`: a compute agent. It connects out to a Data server and runs the work it is given in gVisor sandboxes.
//!
//! Where things are is told by the environment (the agent's own deployment, not anyone's permissions):
//! `LOAMZ_DATA_URL` the Data server, `LOAMZ_AGENT_SECRET_FILE` the file holding the secret the account console showed once,
//! `LOAMZ_RUNSC` and `LOAMZ_ROOTFS` the sandbox runtime and its root filesystem, `LOAMZ_WORK` a folder to work in, and
//! `LOAMZ_PLATFORM` (default `systrap`; `ptrace` where that cannot start, as on WSL2).
#![allow(clippy::print_stderr)]

use std::{path::PathBuf, process::ExitCode};

use loamz_runner::{
    agent::Agent,
    exec::{Gvisor, is_root},
    work::Machine,
};

fn need(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("{name} is not set"))
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() != Some("agent") {
        eprintln!("usage: loamz-runner agent");
        return ExitCode::from(2);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    match setup() {
        Ok(agent) => {
            tracing::info!(data = agent.data_url, "ready for work");
            agent.serve().await;
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("loamz-runner: {e}");
            ExitCode::FAILURE
        }
    }
}

fn setup() -> Result<Agent, String> {
    let secret = std::fs::read_to_string(need("LOAMZ_AGENT_SECRET_FILE")?)
        .map_err(|e| format!("cannot read the agent secret: {e}"))?
        .trim()
        .to_owned();
    if !secret.starts_with("agt_") {
        return Err("the agent secret file does not hold an agent secret (agt_…)".into());
    }
    let runsc = PathBuf::from(need("LOAMZ_RUNSC")?);
    let rootfs = PathBuf::from(need("LOAMZ_ROOTFS")?);
    if !runsc.is_file() || !rootfs.join("usr/local/bin/python3").exists() {
        return Err(format!(
            "{} is not runsc, or {} is not a Python root filesystem",
            runsc.display(),
            rootfs.display()
        ));
    }
    let work = std::env::var("LOAMZ_WORK")
        .map_or_else(|_| PathBuf::from("/var/tmp/loamz-compute"), PathBuf::from);
    let rootless = !is_root();
    if rootless {
        tracing::warn!(
            "not running as root: gVisor cannot enforce the memory and process limits here (the network, file and time limits still hold)"
        );
    }
    Ok(Agent {
        data_url: need("LOAMZ_DATA_URL")?,
        secret,
        machine: Machine {
            gvisor: Gvisor {
                runsc,
                state: work.join("runsc-state"),
                rootless,
                platform: std::env::var("LOAMZ_PLATFORM").unwrap_or_else(|_| "systrap".into()),
            },
            rootfs,
            work,
        },
    })
}
