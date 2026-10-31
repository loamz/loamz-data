//! `loamz-runner run-job`: reads one job (JSON) on standard input, runs it, prints the outcome (JSON) on standard output.
//! The compute machine is told where gVisor and the root filesystem are by environment variables:
//! `LOAMZ_RUNSC`, `LOAMZ_ROOTFS`, optionally `LOAMZ_PLATFORM` (default `systrap`), `LOAMZ_WORK`, `LOAMZ_ROOTLESS`.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::{io::Read, path::PathBuf, process::ExitCode};

use loamz_runner::{
    exec::Gvisor,
    job::{Job, Machine, execute},
};

fn need(name: &str) -> Result<PathBuf, String> {
    std::env::var(name)
        .map(PathBuf::from)
        .map_err(|_| format!("{name} is not set"))
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() != Some("run-job") {
        eprintln!("usage: loamz-runner run-job < job.json");
        return ExitCode::from(2);
    }
    match go().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("loamz-runner: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn go() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    let job: Job =
        serde_json::from_str(&input).map_err(|e| format!("the job is not valid JSON: {e}"))?;
    let work = std::env::var("LOAMZ_WORK")
        .map_or_else(|_| PathBuf::from("/var/tmp/loamz-compute"), PathBuf::from);
    let machine = Machine {
        gvisor: Gvisor {
            runsc: need("LOAMZ_RUNSC")?,
            state: work.join("runsc-state"),
            rootless: std::env::var("LOAMZ_ROOTLESS").is_ok(),
            platform: std::env::var("LOAMZ_PLATFORM").unwrap_or_else(|_| "systrap".into()),
        },
        rootfs: need("LOAMZ_ROOTFS")?,
        work,
    };
    let outcome = execute(job, &machine).await.map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&outcome).map_err(|e| e.to_string())?
    );
    Ok(())
}
