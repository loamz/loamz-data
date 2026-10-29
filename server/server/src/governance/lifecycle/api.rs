use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::service;
use crate::kernel::{
    auth::Caller,
    error::Result,
    http::{Routes, json as body_json},
    state::Shared,
};

#[derive(Deserialize)]
struct Erase {
    subject: String,
}

async fn erase(
    State(app): State<Shared>,
    caller: Caller,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: Erase = body_json(&body)?;
    let task = service::request_erase(&app, &caller, &req.subject).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"task_id": task.id}))))
}

async fn task(
    State(app): State<Shared>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let t = service::task(&app, &caller, &id).await?;
    Ok(Json(
        json!({"id": t.id, "kind": t.kind, "state": t.state, "attempts": t.attempts, "last_error": t.last_error}),
    ))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .post("/api/v1/erase", erase)
        .get("/api/v1/tasks/{id}", task)
}
