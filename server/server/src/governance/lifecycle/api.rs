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

async fn task(State(app): State<Shared>, c: OrgCtx) -> Result<Json<Value>> {
    let id = c
        .params
        .get("id")
        .cloned()
        .ok_or_else(|| Error::Internal("the route has no id".into()))?;
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
}
