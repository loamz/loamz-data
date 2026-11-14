use axum::{Json, body::Bytes, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::service;
use crate::{
    governance::context::OrgCtx,
    kernel::{
        error::{Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
};

#[derive(Deserialize)]
struct Erase {
    subject: String,
}

async fn erase(
    State(app): State<Shared>,
    c: OrgCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: Erase = body_json(&body)?;
    let task = service::request_erase(&app, &c.identity, &c.org, &req.subject).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"task_id": task.id}))))
}

fn id(params: &std::collections::HashMap<String, String>) -> Result<String> {
    params
        .get("id")
        .cloned()
        .ok_or_else(|| Error::Internal("the route has no id".into()))
}

async fn schedules(
    State(app): State<Shared>,
    c: crate::governance::context::WsCtx,
) -> Result<Json<Value>> {
    Ok(Json(json!({"items": service::schedules(&app, &c).await?})))
}

async fn create_schedule(
    State(app): State<Shared>,
    c: crate::governance::context::WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let schedule =
        service::create_schedule(&app, &c, body_json::<service::ScheduleDraft>(&body)?).await?;
    Ok((StatusCode::CREATED, Json(schedule)))
}

async fn pause_schedule(
    State(app): State<Shared>,
    c: crate::governance::context::WsCtx,
) -> Result<StatusCode> {
    service::pause_schedule(&app, &c, &id(&c.params)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resume_schedule(
    State(app): State<Shared>,
    c: crate::governance::context::WsCtx,
) -> Result<StatusCode> {
    service::resume_schedule(&app, &c, &id(&c.params)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_schedule(
    State(app): State<Shared>,
    c: crate::governance::context::WsCtx,
) -> Result<StatusCode> {
    service::delete_schedule(&app, &c, &id(&c.params)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn alerts(
    State(app): State<Shared>,
    c: crate::governance::context::WsCtx,
) -> Result<Json<Value>> {
    Ok(Json(
        json!({"items": service::schedule_alerts(&app, &c, &id(&c.params)?).await?}),
    ))
}

async fn task(State(app): State<Shared>, c: OrgCtx) -> Result<Json<Value>> {
    let id = id(&c.params)?;
    let t = service::task(&app, &c.identity, &c.org, &id).await?;
    Ok(Json(
        json!({"id": t.id, "kind": t.kind, "state": t.state, "attempts": t.attempts, "last_error": t.last_error}),
    ))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .post("/api/v1/orgs/{org}/erase", erase)
        .get("/api/v1/orgs/{org}/tasks/{id}", task)
        .get("/api/v1/workspaces/{ws}/schedules", schedules)
        .post("/api/v1/workspaces/{ws}/schedules", create_schedule)
        .put(
            "/api/v1/workspaces/{ws}/schedules/{id}/pause",
            pause_schedule,
        )
        .put(
            "/api/v1/workspaces/{ws}/schedules/{id}/resume",
            resume_schedule,
        )
        .delete("/api/v1/workspaces/{ws}/schedules/{id}", delete_schedule)
        .get("/api/v1/workspaces/{ws}/schedules/{id}/alerts", alerts)
}
