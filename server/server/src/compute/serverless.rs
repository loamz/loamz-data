//! Serverless compute: a provider's endpoint (such as Modal) that starts when a statement arrives and stops when idle.
//!
//! The server calls the endpoint over https (outbound only). The endpoint never reaches back and holds no storage credential: it is
//! given a short-lived signed address for each file of each table the statement declared, and returns the rows in the answer.

use std::time::Duration;

use serde_json::{Value, json};

use super::{
    model::{MAX_RESULT, Target},
    service::{self, Report},
};
use crate::kernel::{
    error::{Error, Result},
    state::App,
};

/// Files one statement may read through signed addresses.
const MAX_FILES: usize = 200;

/// Serves the queued statements of every serverless target, forever.
pub async fn serve_forever(app: crate::kernel::state::Shared) {
    loop {
        match service::serverless_targets(&app).await {
            Ok(targets) => {
                for t in targets {
                    while let Ok(Some(lease)) = service::lease(&app, &t).await {
                        let (app, t) = (app.clone(), t.clone());
                        tokio::spawn(async move { run(&app, &t, lease).await });
                    }
                }
            }
            Err(e) => tracing::error!(error = %e, "serverless: could not list targets"),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn run(app: &App, target: &Target, lease: service::Lease) {
    let id = lease.run.id.clone();
    let report = match call(app, target, &lease).await {
        Ok(r) => r,
        Err(e) => failed(e.to_string()),
    };
    if let Err(e) = service::complete(app, target, &id, report).await {
        tracing::warn!(run = %id, error = %e, "serverless: could not record the result");
    }
}

fn failed(error: String) -> Report {
    Report {
        state: "failed".into(),
        exit_code: 1,
        stdout: String::new(),
        stderr: String::new(),
        result: String::new(),
        error,
    }
}

async fn call(app: &App, target: &Target, lease: &service::Lease) -> Result<Report> {
    let token = app
        .serverless_token
        .as_deref()
        .ok_or_else(|| Error::Internal("no serverless token".into()))?;
    let spec: Value = serde_json::from_str(&lease.run.spec)
        .map_err(|e| Error::Internal(format!("a run has a spec that is not JSON: {e}")))?;
    let valid = Duration::from_secs(u64::try_from(lease.limits.timeout_s).unwrap_or(120) + 120);
    let mut tables = Vec::new();
    let mut total = 0;
    for t in spec["tables"].as_array().into_iter().flatten() {
        let (id, name) = (t["id"].as_str().unwrap_or_default(), &t["name"]);
        let mut files = Vec::new();
        for f in app.stores.list(id).await? {
            total += 1;
            if total > MAX_FILES {
                return Ok(failed(format!("that reads more than {MAX_FILES} files")));
            }
            files.push(
                json!({"path": f.path, "url": app.stores.sign_get(id, &f.path, valid).await?}),
            );
        }
        tables.push(json!({"name": name, "files": files}));
    }
    let body = json!({
        "sql": spec["sql"], "limit": spec["limit"], "memory_mb": lease.limits.memory_mb, "tables": tables,
    });
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(
            u64::try_from(lease.limits.timeout_s).unwrap_or(120) + 30,
        ))
        .build()
        .map_err(|e| Error::Internal(e.to_string()))?;
    let call = http
        .post(&target.endpoint)
        .bearer_auth(token)
        .json(&body)
        .send();
    tokio::pin!(call);
    let mut beat = tokio::time::interval(Duration::from_secs(20));
    let reply = loop {
        tokio::select! {
            r = &mut call => break r,
            _ = beat.tick() => {
                if service::heartbeat(app, target, &lease.run.id).await? == Some(true) {
                    let mut r = failed("cancelled".into());
                    "cancelled".clone_into(&mut r.state);
                    return Ok(r);
                }
            }
        }
    };
    let reply = match reply {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            let mut r = failed("the compute did not answer in time".into());
            "timed_out".clone_into(&mut r.state);
            return Ok(r);
        }
        Err(_) => return Ok(failed(format!("{} could not be reached", target.name))),
    };
    let status = reply.status();
    let text = reply.text().await.unwrap_or_default();
    if !status.is_success() {
        return Ok(failed(format!("{} answered {status}", target.name)));
    }
    if text.len() > MAX_RESULT {
        return Ok(failed("the result is too large".into()));
    }
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Internal(format!("the answer is not JSON: {e}")))?;
    Ok(match v["error"].as_str() {
        Some(e) => failed(e.to_owned()),
        None => Report {
            state: "succeeded".into(),
            exit_code: 0,
            stdout: format!("ran on {} (serverless)", target.name),
            stderr: String::new(),
            result: text,
            error: String::new(),
        },
    })
}
