//! The compute agent: a machine's connection to a Data server. It connects out (it never accepts connections), asks for work, runs it
//! one piece at a time, and tells the server how it went. It is given a secret when the machine is attached; it holds nothing else.

use std::time::Duration;

use serde_json::Value;
use tokio::sync::watch;

use crate::{
    exec::{Executor, WarmExecutor},
    pool::Pool,
    work::{Lease, Machine, Report, execute},
};

pub struct Agent {
    pub data_url: String,
    pub secret: String,
    pub machine: Machine,
}

impl Agent {
    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/agent{path}", self.data_url.trim_end_matches('/'))
    }

    /// Asks for work, and does it, until the process is stopped. Trouble reaching the server is waited out, never fatal.
    pub async fn serve(&self) {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap_or_default();
        let machine = self.machine.clone();
        let binary = std::env::var_os("LOAMZ_WORKER_BIN")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .map(|p| p.with_file_name("loamz-worker"))
            });
        let warm = WarmExecutor {
            pool: Pool::new(100, Duration::from_mins(5)),
            start: move |_key: crate::pool::WorkerKey, limits: crate::work::Limits| {
                let machine = machine.clone();
                let binary = binary.clone();
                async move {
                    let binary = binary.ok_or_else(|| {
                        std::io::Error::other("the SQL worker path is unavailable")
                    })?;
                    crate::worker_sandbox::start(&machine, limits, &binary).await
                }
            },
        };
        loop {
            match self.lease(&http).await {
                Ok(Some(lease)) => self.run_one(&http, lease, &warm).await,
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(error = e, "could not ask for work");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn lease(&self, http: &reqwest::Client) -> Result<Option<Lease>, String> {
        let r = http
            .post(self.url("/lease"))
            .bearer_auth(&self.secret)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        match r.status().as_u16() {
            200 => Ok(Some(r.json().await.map_err(|e| format!("the server's work is not readable: {e}"))?)),
            204 => Ok(None),
            401 => Err("the server does not know this agent's secret (was the compute removed, or given a new one?)".into()),
            s => Err(format!("the server answered {s}")),
        }
    }

    async fn run_one<E: Executor>(&self, http: &reqwest::Client, lease: Lease, warm: &E) {
        let id = lease.run.id.clone();
        tracing::info!(run = id, kind = lease.run.kind, "starting");
        // While it works, keep telling the server so; the answer says whether the person has cancelled it.
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let beat = {
            let (http, url, secret) = (
                http.clone(),
                self.url(&format!("/runs/{id}/heartbeat")),
                self.secret.clone(),
            );
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    let stopped = match http.post(&url).bearer_auth(&secret).send().await {
                        Ok(r) if r.status().is_success() => {
                            r.json::<Value>().await.is_ok_and(|v| v["cancel"] == true)
                        }
                        // Not ours any more (the server gave up on us): stop.
                        Ok(r) => r.status().as_u16() == 404,
                        Err(_) => false,
                    };
                    if stopped {
                        let _ = stop_tx.send(true);
                    }
                }
            })
        };
        let stop = async move {
            let _ = stop_rx.wait_for(|stopped| *stopped).await;
        };
        let report = if lease.run.kind == "sql" {
            warm.execute(&lease, Box::pin(stop)).await
        } else {
            execute(&lease, &self.machine, &self.data_url, stop).await
        };
        beat.abort();
        tracing::info!(run = id, state = report.state, "finished");
        self.report(http, &id, &report).await;
    }

    /// Says how a run ended. Tries a few times: the server is waiting for it.
    async fn report(&self, http: &reqwest::Client, id: &str, report: &Report) {
        for attempt in 1..=5 {
            match http
                .post(self.url(&format!("/runs/{id}/complete")))
                .bearer_auth(&self.secret)
                .json(report)
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => return,
                // The server has already ended it (we were too slow): there is nothing more to say.
                Ok(r) if r.status().as_u16() == 409 || r.status().as_u16() == 404 => return,
                Ok(r) => tracing::warn!(
                    run = id,
                    attempt,
                    status = r.status().as_u16(),
                    "could not report"
                ),
                Err(e) => tracing::warn!(run = id, attempt, error = %e, "could not report"),
            }
            tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
        }
    }
}
