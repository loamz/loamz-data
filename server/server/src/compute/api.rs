use std::time::Duration;

use axum::{
    Json,
    body::Bytes,
    extract::{FromRequestParts, Path, Query, State},
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    model::{IDLE_STOP_MS, Run, RunFilter, Target, is_terminal},
    service::{self, Ctx, ImportRequest, JobRequest, Report, SqlRequest},
};
use crate::{
    governance::context::WsCtx,
    kernel::{
        error::{Error, Result},
        http::{Routes, json as body_json},
        now_ms,
        state::Shared,
    },
};

fn ctx(c: &WsCtx) -> Ctx<'_> {
    Ctx {
        identity: &c.identity,
        workspace: &c.workspace,
        standing: c.standing,
        sides: &c.sides,
        schedule_task_id: None,
    }
}

fn param(params: &std::collections::HashMap<String, String>, name: &str) -> Result<String> {
    params
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

/// A run as people see it. A list leaves out what can be large; the one run has everything. Who it runs as is never shown.
fn run_view(r: &Run, detail: bool) -> Value {
    let mut v = json!({
        "id": r.id, "kind": r.kind, "state": r.state, "title": r.title,
        "requested_by": r.requested_by, "on_behalf_of": r.on_behalf_of, "target_id": r.target_id,
        "created_ms": r.created_ms, "started_ms": r.started_ms, "finished_ms": r.finished_ms,
        "error": if detail { &r.error } else { "" }, "exit_code": r.exit_code, "cancelling": r.cancel && !is_terminal(&r.state),
        "queued_ms": waited(r, true), "run_ms": waited(r, false),
    });
    if detail {
        let mut spec = json_of(&r.spec);
        if let Some(o) = spec.as_object_mut() {
            o.remove("as");
        }
        v["spec"] = spec;
        v["stdout"] = json!(r.stdout);
        v["stderr"] = json!(r.stderr);
        v["result"] = json_of(&r.result);
        v["result_access"] = json!("available");
    }
    v
}

/// A history record survives loss of input access, but never includes results or process output.
/// Only the person or service that directly submitted a SQL statement may see its full text here.
fn restricted_view(r: &Run, c: &WsCtx) -> Value {
    let mut view = run_view(r, false);
    view["result_access"] = json!("restricted");
    let direct_owner = c.identity.actor.is_none()
        && r.on_behalf_of.is_empty()
        && r.requested_by == c.identity.subject;
    if direct_owner && r.kind == "sql" {
        if let Some(sql) = json_of(&r.spec)["sql"].as_str() {
            view["spec"] = json!({"sql": sql});
        }
    } else {
        view["title"] = json!(if r.kind == "sql" { "Query" } else { "Job" });
    }
    view
}

/// How long a run waited before it started (`queued`), or how long it ran (once it has ended); `-1` while that is not known yet. These
/// are differences of wall-clock times, and a clock that is stepped back (WSL2 does) must not make one negative.
fn waited(r: &Run, queued: bool) -> i64 {
    if queued {
        if r.started_ms > 0 {
            (r.started_ms - r.created_ms).max(0)
        } else {
            -1
        }
    } else if r.started_ms > 0 && r.finished_ms > 0 {
        (r.finished_ms - r.started_ms).max(0)
    } else {
        -1
    }
}

fn target_view(t: &Target, now: i64, last_run_ms: i64, unfinished_since: i64) -> Value {
    // A serverless target's `last_seen_ms` is when it was last started (positive) or stopped (negative) by hand; it is in use from
    // then or from its last run, whichever is later.
    let (started, stopped) = if t.serverless() {
        (t.last_seen_ms.max(0), (-t.last_seen_ms).max(0))
    } else {
        (0, 0)
    };
    // Serverless compute is starting when something is waiting on it and it had been idle (stopped or past its idle minute) when
    // that was asked for: the first statement after a stop pays for the start.
    let last_used = last_run_ms.max(started);
    let was_cold = unfinished_since > 0
        && (last_used == 0 || unfinished_since - last_used > IDLE_STOP_MS || stopped >= last_used);
    json!({"starting": t.serverless() && was_cold, "id": t.id, "name": t.name, "kind": t.kind, "online": t.online(now), "max_memory_mb": t.max_memory_mb, "max_cpu_millis": t.max_cpu_millis, "last_seen_ms": t.last_seen_ms.max(0), "last_used_ms": last_run_ms.max(started), "stopped_ms": stopped, "created_ms": t.created_ms})
}

async fn listed(
    app: &Shared,
    identity: &crate::kernel::auth::Identity,
    workspace: &str,
) -> Result<Vec<Value>> {
    let now = now_ms();
    let used = service::last_used(app, workspace).await?;
    let waiting = service::unfinished_since(app, workspace).await?;
    Ok(service::targets(app, identity, workspace)
        .await?
        .iter()
        .map(|t| {
            target_view(
                t,
                now,
                used.get(&t.id).copied().unwrap_or(0),
                waiting.get(&t.id).copied().unwrap_or(0),
            )
        })
        .collect())
}

fn provider_view(p: &service::Provider) -> Value {
    json!({"id": p.id, "name": p.name, "description": p.description, "available": p.available, "max_memory_mb": p.max_memory_mb, "max_cpu_millis": p.max_cpu_millis, "idle_stop_s": p.idle_stop_s})
}

async fn providers(State(app): State<Shared>, _c: WsCtx) -> Json<Value> {
    Json(json!({"items": service::providers(&app).iter().map(provider_view).collect::<Vec<_>>()}))
}

// ---- people ------------------------------------------------------------------------------------------------------

async fn list_targets(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    Ok(Json(
        json!({"items": listed(&app, &c.identity, &c.workspace.id).await?}),
    ))
}

/// The workspace's own cap on active runs (null: none).
async fn get_quota(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    Ok(Json(
        json!({"max_active_runs": service::quota(&app, &ctx(&c)).await?}),
    ))
}

#[derive(Deserialize)]
struct Quota {
    max_active_runs: Option<i64>,
}

async fn set_quota(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<StatusCode> {
    let req: Quota = body_json(&body)?;
    service::set_quota(&app, &ctx(&c), req.max_active_runs).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Which compute may read a catalog (an empty list: any compute of the workspace).
async fn catalog_policy(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let ids = service::catalog_policy(&app, &ctx(&c), &param(&c.params, "catalog")?).await?;
    Ok(Json(json!({"compute": ids})))
}

#[derive(Deserialize)]
struct CatalogPolicy {
    compute: Vec<String>,
}

async fn set_catalog_policy(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<StatusCode> {
    let req: CatalogPolicy = body_json(&body)?;
    service::set_catalog_policy(&app, &ctx(&c), &param(&c.params, "catalog")?, &req.compute)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct StatementBody {
    sql: String,
    limit: Option<i64>,
    target: Option<String>,
    /// How long to wait for the answer, in seconds, before getting the run back to ask about later.
    wait_s: Option<u64>,
}

/// Runs a statement and waits a while for its rows. It comes back finished (200), or still running (202) with the id to ask about.
async fn statement(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<Response> {
    let req: StatementBody = body_json(&body)?;
    let k = ctx(&c);
    let run = service::submit_sql(
        &app,
        &k,
        SqlRequest {
            sql: req.sql,
            limit: req.limit,
            target: req.target,
        },
    )
    .await?;
    let run = service::wait(
        &app,
        &k,
        run,
        Duration::from_secs(req.wait_s.unwrap_or(30).min(60)),
    )
    .await?;
    let code = if is_terminal(&run.state) {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    let view = if service::require_detail(&app, &k, &run).await.is_ok() {
        run_view(&run, true)
    } else {
        restricted_view(&run, &c)
    };
    Ok((code, Json(view)).into_response())
}

async fn submit_import(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: ImportRequest = body_json(&body)?;
    let run = service::submit_import(&app, &ctx(&c), req).await?;
    Ok((StatusCode::ACCEPTED, Json(run_view(&run, false))))
}

async fn submit_job(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: JobRequest = body_json(&body)?;
    let run = service::submit_job(&app, &ctx(&c), req).await?;
    Ok((StatusCode::ACCEPTED, Json(run_view(&run, false))))
}

#[derive(Deserialize)]
struct ListParams {
    kind: Option<String>,
    limit: Option<i64>,
    /// Only what this person asked for (an admin's narrowing; everyone else sees only their own anyway).
    by: Option<String>,
    target: Option<String>,
    state: Option<String>,
    since_ms: Option<i64>,
    /// The page cursor: the `created_ms` of the oldest run of the page before.
    before_ms: Option<i64>,
}

async fn list_runs(
    State(app): State<Shared>,
    c: WsCtx,
    Query(p): Query<ListParams>,
) -> Result<Json<Value>> {
    if p.kind.as_deref().is_some_and(|k| k != "sql" && k != "job") {
        return Err(Error::Invalid("kind must be sql or job".into()));
    }
    if p.state
        .as_deref()
        .is_some_and(|s| !crate::compute::model::STATES.contains(&s))
    {
        return Err(Error::Invalid("unknown state".into()));
    }
    let filter = RunFilter {
        by: p.by,
        kind: p.kind,
        target: p.target,
        state: p.state,
        since_ms: p.since_ms,
        before_ms: p.before_ms,
    };
    let runs = service::list(&app, &ctx(&c), filter, p.limit.unwrap_or(50)).await?;
    Ok(Json(
        json!({"items": runs.iter().map(|r| run_view(r, false)).collect::<Vec<_>>()}),
    ))
}

async fn get_run(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let context = ctx(&c);
    let run = service::get(&app, &context, &param(&c.params, "id")?).await?;
    let view = if service::require_detail(&app, &context, &run).await.is_ok() {
        run_view(&run, true)
    } else {
        restricted_view(&run, &c)
    };
    Ok(Json(view))
}

async fn cancel_run(State(app): State<Shared>, c: WsCtx) -> Result<(StatusCode, Json<Value>)> {
    let run = service::cancel(&app, &ctx(&c), &param(&c.params, "id")?).await?;
    Ok((StatusCode::ACCEPTED, Json(run_view(&run, false))))
}

// ---- the workspace admins' ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct NewTarget {
    name: String,
    /// `agent` (default) or `serverless`.
    kind: Option<String>,
    /// For serverless: the https address of the provider's endpoint.
    endpoint: Option<String>,
    max_memory_mb: Option<i64>,
    max_cpu_millis: Option<i64>,
}

/// The secret is in this answer and nowhere else, ever again.
async fn attach(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: NewTarget = body_json(&body)?;
    if req.kind.as_deref() == Some(super::model::SERVERLESS) {
        let t = service::add_serverless(
            &app,
            &c.identity,
            &c.workspace.id,
            &req.name,
            req.endpoint.as_deref(),
            req.max_memory_mb.unwrap_or(2048),
            req.max_cpu_millis.unwrap_or(1000),
        )
        .await?;
        return Ok((StatusCode::CREATED, Json(target_view(&t, now_ms(), 0, 0))));
    }
    let e = service::add_target(
        &app,
        &c.identity,
        &c.workspace.id,
        &req.name,
        req.max_memory_mb.unwrap_or(2048),
        req.max_cpu_millis.unwrap_or(1000),
    )
    .await?;
    let mut v = target_view(&e.target, now_ms(), 0, 0);
    v["secret"] = json!(e.secret);
    Ok((StatusCode::CREATED, Json(v)))
}

async fn rotate(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let e =
        service::new_secret(&app, &c.identity, &c.workspace.id, &param(&c.params, "id")?).await?;
    let mut v = target_view(&e.target, now_ms(), 0, 0);
    v["secret"] = json!(e.secret);
    Ok(Json(v))
}

async fn start(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    service::power(
        &app,
        &c.identity,
        &c.workspace.id,
        &param(&c.params, "id")?,
        true,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn stop(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    service::power(
        &app,
        &c.identity,
        &c.workspace.id,
        &param(&c.params, "id")?,
        false,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    service::remove_target(&app, &c.identity, &c.workspace.id, &param(&c.params, "id")?).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- the agent's -------------------------------------------------------------------------------------------------

/// A compute agent, known by its secret.
pub struct Agent(pub Target);

impl FromRequestParts<Shared> for Agent {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self> {
        let secret = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(Error::Unauthorized)?;
        service::authenticate_agent(app, secret).await.map(Agent)
    }
}

#[derive(Deserialize)]
struct LeaseParams {
    wait_s: Option<u64>,
}

/// Waits up to twenty seconds for work (or as long as `wait_s` says, at most twenty-five), so an idle agent costs one request every
/// twenty seconds and hears of new work at once.
async fn lease(
    State(app): State<Shared>,
    Agent(target): Agent,
    Query(p): Query<LeaseParams>,
) -> Result<Response> {
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(p.wait_s.unwrap_or(20).min(25));
    loop {
        // Ask to be woken before looking, so work queued between the look and the wait is not missed.
        let queued = app.queued.notified();
        tokio::pin!(queued);
        queued.as_mut().enable();
        if let Some(l) = service::lease(&app, &target).await? {
            let mut spec = json_of(&l.run.spec);
            if let Some(o) = spec.as_object_mut() {
                o.remove("as");
            }
            let body = json!({
                "run": {"id": l.run.id, "kind": l.run.kind, "org_id": l.run.org_id, "target_id": l.run.target_id, "workspace_id": l.run.workspace_id, "spec": spec},
                "token": l.token,
                "limits": {"memory_mb": l.limits.memory_mb, "cpu_millis": l.limits.cpu_millis, "timeout_s": l.limits.timeout_s},
                "storage": l.storage,
            });
            return Ok(Json(body).into_response());
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        // New work wakes this at once; the short sleep is only a safety net (a lease that frees up, a missed wake).
        tokio::select! {
            () = &mut queued => {}
            () = tokio::time::sleep(Duration::from_millis(400)) => {}
        }
    }
}

async fn heartbeat(
    State(app): State<Shared>,
    Agent(target): Agent,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    match service::heartbeat(&app, &target, &id).await? {
        Some(cancel) => Ok(Json(json!({"cancel": cancel}))),
        None => Err(Error::NotFound("this run is not yours any more".into())),
    }
}

async fn complete(
    State(app): State<Shared>,
    Agent(target): Agent,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<StatusCode> {
    let report: Report = body_json(&body)?;
    service::complete(&app, &target, &id, report).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/workspaces/{ws}/compute/targets", list_targets)
        .get("/api/v1/workspaces/{ws}/compute/providers", providers)
        .get("/api/v1/workspaces/{ws}/quota", get_quota)
        .put("/api/v1/workspaces/{ws}/quota", set_quota)
        .get(
            "/api/v1/workspaces/{ws}/catalogs/{catalog}/compute",
            catalog_policy,
        )
        .put(
            "/api/v1/workspaces/{ws}/catalogs/{catalog}/compute",
            set_catalog_policy,
        )
        .post("/api/v1/workspaces/{ws}/sql/statements", statement)
        .post("/api/v1/workspaces/{ws}/runs", submit_job)
        .post("/api/v1/workspaces/{ws}/imports", submit_import)
        .get("/api/v1/workspaces/{ws}/runs", list_runs)
        .get("/api/v1/workspaces/{ws}/runs/{id}", get_run)
        .post("/api/v1/workspaces/{ws}/runs/{id}/cancel", cancel_run)
        .post("/api/v1/workspaces/{ws}/compute/targets", attach)
        .post(
            "/api/v1/workspaces/{ws}/compute/targets/{id}/secret",
            rotate,
        )
        .post("/api/v1/workspaces/{ws}/compute/targets/{id}/start", start)
        .post("/api/v1/workspaces/{ws}/compute/targets/{id}/stop", stop)
        .delete("/api/v1/workspaces/{ws}/compute/targets/{id}", detach)
        .post("/api/v1/agent/lease", lease)
        .post("/api/v1/agent/runs/{id}/heartbeat", heartbeat)
        .post("/api/v1/agent/runs/{id}/complete", complete)
}
