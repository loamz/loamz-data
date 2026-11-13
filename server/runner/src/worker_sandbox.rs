//! Starts a persistent SQL worker in gVisor with a private, read-only broker mount.

use std::{
    os::unix::fs::{PermissionsExt, chown},
    path::PathBuf,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    broker::Broker,
    check::verify,
    exec::{prepare_workspace, private_dir},
    pool::Worker,
    spec::{Sandbox, User, oci_config},
    work::{Limits, Machine},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Start a runtime process. Its storage broker has no access until a lease is installed.
pub async fn start(
    machine: &Machine,
    limits: Limits,
    binary: &std::path::Path,
) -> std::io::Result<Worker> {
    private_dir(&machine.work)?;
    let id = format!(
        "warm-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let dir = machine.work.join(&id);
    private_dir(&dir)?;
    let result = start_in(machine, limits, binary, &id, &dir).await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    result
}

async fn start_in(
    machine: &Machine,
    limits: Limits,
    binary: &std::path::Path,
    id: &str,
    dir: &std::path::Path,
) -> std::io::Result<Worker> {
    let user = if machine.gvisor.rootless {
        User::MappedRoot
    } else {
        User::Nobody
    };
    let binary_dir = dir.join("binary");
    let broker_dir = dir.join("broker");
    let workspace = dir.join("workspace");
    let bundle = dir.join("bundle");
    for path in [&binary_dir, &broker_dir, &bundle] {
        std::fs::create_dir_all(path)?;
    }
    prepare_workspace(&workspace, user)?;
    std::fs::copy(binary, binary_dir.join("loamz-worker"))?;
    std::fs::set_permissions(
        binary_dir.join("loamz-worker"),
        std::fs::Permissions::from_mode(0o555),
    )?;
    // DuckDB's library is a file beside the worker; the worker finds it there (its rpath is its own folder). It is linked, not
    // copied, when it can be: it is large and the same for every worker.
    let library = binary.with_file_name("libduckdb.so");
    if library.is_file() {
        let target = binary_dir.join("libduckdb.so");
        if std::fs::hard_link(&library, &target).is_err() {
            std::fs::copy(&library, &target)?;
        }
    }
    if user == User::Nobody {
        chown(&broker_dir, Some(user.uid()), Some(user.uid()))?;
    }
    std::fs::set_permissions(&broker_dir, std::fs::Permissions::from_mode(0o700))?;
    let broker = Broker::start(&broker_dir)?;
    let socket = broker_dir.join("socket");
    if user == User::Nobody {
        chown(&socket, Some(user.uid()), Some(user.uid()))?;
    }
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    let sandbox = Sandbox {
        rootfs: machine.rootfs.clone(),
        inputs: vec![("worker".into(), binary_dir), ("broker".into(), broker_dir)],
        workspace,
        args: vec!["/inputs/worker/loamz-worker".into()],
        memory_bytes: limits.memory_mb << 20,
        pids_max: 256,
        cpu_millis: limits.cpu_millis,
        user,
    };
    let config = oci_config(&sandbox).map_err(std::io::Error::other)?;
    verify(&config, user).map_err(|e| std::io::Error::other(e.join("; ")))?;
    tokio::fs::write(bundle.join("config.json"), config.to_string()).await?;
    let (mut command, cleanup) = machine.gvisor.worker_commands(&bundle, id);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = command.spawn()?;
    Ok(Worker::new(child, Some(cleanup))
        .with_broker(broker, PathBuf::from(dir))
        .with_limits(limits.memory_mb, limits.cpu_millis))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{
        exec::{Executor, Gvisor, WarmExecutor},
        pool::{Pool, WorkerKey},
        work::Lease,
    };

    #[tokio::test]
    #[ignore = "requires LOAMZ_TEST_WORKER_BIN, LOAMZ_RUNSC and LOAMZ_ROOTFS"]
    async fn persistent_worker_runs_in_gvisor() {
        let dir = std::env::temp_dir().join(format!("loamz-warm-test-{}", std::process::id()));
        let machine = Machine {
            gvisor: Gvisor {
                runsc: std::env::var("LOAMZ_RUNSC").unwrap().into(),
                state: dir.join("state"),
                rootless: true,
                platform: "ptrace".into(),
            },
            rootfs: std::env::var("LOAMZ_ROOTFS").unwrap().into(),
            work: dir.clone(),
        };
        let binary: PathBuf = std::env::var("LOAMZ_TEST_WORKER_BIN").unwrap().into();
        let executor = WarmExecutor {
            pool: Pool::new(10, std::time::Duration::from_mins(1)),
            start: move |_key: WorkerKey, limits: Limits| {
                let (machine, binary) = (machine.clone(), binary.clone());
                async move { start(&machine, limits, &binary).await }
            },
        };
        let lease: Lease = serde_json::from_str(r#"{"run":{"id":"r","kind":"sql","org_id":"o","target_id":"t","workspace_id":"w","spec":{"sql":"select 42 as answer","limit":2,"tables":[]}},"token":"","limits":{"memory_mb":512,"cpu_millis":1000,"timeout_s":30}}"#).unwrap();
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
            .invalidate(&WorkerKey {
                org_id: "o".into(),
                target_id: "t".into(),
            })
            .await;
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
