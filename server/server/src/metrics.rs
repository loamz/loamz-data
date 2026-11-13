//! What the server can say about itself for a monitoring system: how many requests it answered and how long they took, and how much
//! work is waiting. `GET /metrics` in the Prometheus text format, behind a scrape token kept in the vault (not set: the route does not
//! exist). The numbers are counts and sums, never a name, an id or any content.

use std::{collections::BTreeMap, fmt::Write as _, sync::Mutex, time::Instant};

use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{
    compute::service as compute,
    kernel::{
        error::{Error, Result},
        http::Routes,
        state::Shared,
    },
};

/// Requests by method and status class, with the seconds they took. Kept in memory: a restart starts it again, as counters do.
#[derive(Default)]
pub struct Metrics {
    requests: Mutex<BTreeMap<(String, u16), (u64, f64)>>,
}

impl Metrics {
    fn record(&self, method: &str, status: u16, seconds: f64) {
        if let Ok(mut all) = self.requests.lock() {
            let entry = all.entry((method.to_owned(), status)).or_default();
            entry.0 += 1;
            entry.1 += seconds;
        }
    }

    /// The Prometheus text for what was counted.
    fn render(&self) -> String {
        let mut out = String::from(
            "# TYPE loamz_http_requests_total counter\n# TYPE loamz_http_request_seconds_sum counter\n",
        );
        if let Ok(all) = self.requests.lock() {
            for ((method, status), (n, seconds)) in all.iter() {
                let _ = writeln!(
                    out,
                    "loamz_http_requests_total{{method=\"{method}\",status=\"{status}\"}} {n}"
                );
                let _ = writeln!(
                    out,
                    "loamz_http_request_seconds_sum{{method=\"{method}\",status=\"{status}\"}} {seconds:.6}"
                );
            }
        }
        out
    }
}

/// Counts every request that passes through, by method and the status it got.
pub async fn count(State(app): State<Shared>, req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_owned();
    let started = Instant::now();
    let reply = next.run(req).await;
    app.metrics.record(
        &method,
        reply.status().as_u16(),
        started.elapsed().as_secs_f64(),
    );
    reply
}

async fn scrape(State(app): State<Shared>, headers: axum::http::HeaderMap) -> Result<Response> {
    let Some(want) = app.metrics_token.as_deref() else {
        return Err(Error::NotFound("no such route".into()));
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if given != Some(want) {
        return Err(Error::Unauthorized);
    }
    let mut body = app.metrics.render();
    body.push_str("# TYPE loamz_runs gauge\n");
    for (state, n) in compute::queue_depths(&app).await? {
        let _ = writeln!(body, "loamz_runs{{state=\"{state}\"}} {n}");
    }
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response())
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new().get("/metrics", scrape)
}
