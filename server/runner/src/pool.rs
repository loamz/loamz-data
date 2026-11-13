//! Owns persistent sandbox processes separately for each organization and compute target.

use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{
    process::{Child, Command},
    sync::{Mutex, OwnedMutexGuard},
};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct WorkerKey {
    pub org_id: String,
    pub target_id: String,
}

/// An owned sandbox process. Dropping it kills the runtime process.
pub struct Worker {
    pub child: Child,
    cleanup: Option<Command>,
    runs: usize,
    last_used: Instant,
}

impl Worker {
    pub fn new(child: Child, cleanup: Option<Command>) -> Self {
        Self {
            child,
            cleanup,
            runs: 0,
            last_used: Instant::now(),
        }
    }

    pub fn completed(&mut self) {
        self.runs = self.runs.saturating_add(1);
        self.last_used = Instant::now();
    }

    async fn kill(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        if let Some(mut cleanup) = self.cleanup.take() {
            match cleanup.status().await {
                Ok(status) if status.success() => {}
                Ok(status) => tracing::warn!(%status, "sandbox cleanup failed"),
                Err(error) => tracing::warn!(%error, "sandbox cleanup could not start"),
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

type Slot = Arc<Mutex<Worker>>;

/// A pool owns at most one worker per key and serializes use through the slot mutex.
pub struct Pool {
    workers: Mutex<HashMap<WorkerKey, Slot>>,
    max_runs: usize,
    max_idle: Duration,
}

impl Pool {
    #[must_use]
    pub fn new(max_runs: usize, max_idle: Duration) -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
            max_runs: max_runs.max(1),
            max_idle,
        }
    }

    /// Return the existing worker or start one under the caller's sandbox policy.
    pub async fn acquire<F, Fut>(
        &self,
        key: WorkerKey,
        start: F,
    ) -> std::io::Result<OwnedMutexGuard<Worker>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = std::io::Result<Worker>>,
    {
        let mut workers = self.workers.lock().await;
        if let Some(slot) = workers.get(&key) {
            let mut worker = Arc::clone(slot).lock_owned().await;
            let dead = worker.child.try_wait()?.is_some();
            if !dead && worker.runs < self.max_runs && worker.last_used.elapsed() < self.max_idle {
                return Ok(worker);
            }
            worker.kill().await;
        }
        workers.remove(&key);
        let slot = Arc::new(Mutex::new(start().await?));
        workers.insert(key, Arc::clone(&slot));
        Ok(slot.lock_owned().await)
    }

    /// Remove a failed sandbox and wait until its runtime process has exited.
    pub async fn invalidate(&self, key: &WorkerKey) {
        if let Some(slot) = self.workers.lock().await.remove(key) {
            slot.lock().await.kill().await;
        }
    }

    pub async fn reap_idle(&self) {
        let mut workers = self.workers.lock().await;
        let keys: Vec<_> = workers.keys().cloned().collect();
        for key in keys {
            if let Some(slot) = workers.get(&key) {
                // Never reap a worker while a run holds its slot.
                if let Ok(mut worker) = slot.try_lock() {
                    if worker.last_used.elapsed() < self.max_idle && worker.runs < self.max_runs {
                        continue;
                    }
                    worker.kill().await;
                } else {
                    continue;
                }
            }
            workers.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tokio::process::Command;

    fn key(org: &str) -> WorkerKey {
        WorkerKey {
            org_id: org.into(),
            target_id: "target".into(),
        }
    }

    async fn child() -> std::io::Result<Worker> {
        Ok(Worker::new(
            Command::new("sleep").arg("60").kill_on_drop(true).spawn()?,
            None,
        ))
    }

    #[tokio::test]
    async fn organizations_never_share_workers_and_failures_kill_them() {
        let pool = Pool::new(10, Duration::from_mins(1));
        let a = pool.acquire(key("a"), child).await.unwrap();
        let a_id = a.child.id();
        drop(a);
        let b = pool.acquire(key("b"), child).await.unwrap();
        assert_ne!(a_id, b.child.id());
        drop(b);
        let a = pool.acquire(key("a"), child).await.unwrap();
        assert_eq!(a_id, a.child.id());
        drop(a);
        let a = Arc::clone(pool.workers.lock().await.get(&key("a")).unwrap());
        pool.invalidate(&key("a")).await;
        assert!(a.lock().await.child.try_wait().unwrap().is_some());
        pool.invalidate(&key("b")).await;
    }

    #[tokio::test]
    async fn workers_recycle_after_the_run_limit_and_idle() {
        let pool = Pool::new(1, Duration::from_mins(1));
        let mut first = pool.acquire(key("a"), child).await.unwrap();
        let first_id = first.child.id();
        first.completed();
        drop(first);
        let first = Arc::clone(pool.workers.lock().await.get(&key("a")).unwrap());
        let second = pool.acquire(key("a"), child).await.unwrap();
        assert_ne!(first_id, second.child.id());
        drop(second);
        assert!(first.lock().await.child.try_wait().unwrap().is_some());
        pool.invalidate(&key("a")).await;
        let idle = Pool::new(10, Duration::ZERO);
        let guard = idle.acquire(key("a"), child).await.unwrap();
        drop(guard);
        let slot = Arc::clone(idle.workers.lock().await.get(&key("a")).unwrap());
        idle.reap_idle().await;
        assert!(slot.lock().await.child.try_wait().unwrap().is_some());
    }

    #[tokio::test]
    async fn invalidation_runs_sandbox_cleanup() {
        let marker =
            std::env::temp_dir().join(format!("loamz-pool-cleanup-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let pool = Pool::new(2, Duration::from_mins(1));
        let guard = pool
            .acquire(key("a"), || async {
                let process = Command::new("sleep").arg("60").kill_on_drop(true).spawn()?;
                let mut cleanup = Command::new("touch");
                cleanup.arg(&marker);
                Ok(Worker::new(process, Some(cleanup)))
            })
            .await
            .unwrap();
        drop(guard);
        pool.invalidate(&key("a")).await;
        assert!(marker.exists());
        std::fs::remove_file(marker).unwrap();
    }
}
