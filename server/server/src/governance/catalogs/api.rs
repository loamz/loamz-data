use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    model::{Draft, Kind, Securable},
    service,
};
use crate::{
    governance::{context::WsCtx, lifecycle::service as lifecycle},
    kernel::{
        error::{Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
};

#[derive(Deserialize)]
struct Create {
    kind: String,
    #[serde(default)]
    parent_id: String,
    name: String,
    #[serde(default)]
    classification: String,
    #[serde(default)]
    placement: String,
    #[serde(default)]
    consistency: String,
}

#[derive(Deserialize)]
struct Params {
    #[serde(default)]
    parent_id: String,
}

#[derive(Deserialize)]
struct Transfer {
    owner: String,
}

#[must_use]
pub fn view(s: &Securable) -> Value {
    json!({
        "id": s.id, "kind": s.kind.as_str(), "parent_id": s.parent_id, "name": s.name, "owner": s.owner,
        "classification": s.classification, "placement": s.placement, "consistency": s.consistency, "created_ms": s.created_ms,
    })
}

fn id(c: &WsCtx) -> Result<String> {
    c.params
        .get("id")
        .cloned()
        .ok_or_else(|| Error::Internal("the route has no id".into()))
}

async fn create(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: Create = body_json(&body)?;
    let kind = Kind::parse(&req.kind).ok_or_else(|| {
        Error::Invalid("kind must be catalog, schema, table, volume or view".into())
    })?;
    let draft = Draft {
        kind,
        parent_id: req.parent_id,
        name: req.name,
        classification: req.classification,
        placement: req.placement,
        consistency: req.consistency,
    };
    let made =
        service::create(&app, &c.identity, &c.workspace.id, c.org(), &c.sides, draft).await?;
    Ok((StatusCode::CREATED, Json(view(&made))))
}

async fn list(State(app): State<Shared>, c: WsCtx, Query(p): Query<Params>) -> Result<Json<Value>> {
    let items: Vec<Value> = service::list(
        &app,
        &c.identity,
        &c.workspace.id,
        c.org(),
        &c.sides,
        &p.parent_id,
    )
    .await?
    .iter()
    .map(view)
    .collect();
    Ok(Json(json!({"items": items})))
}

async fn get(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    Ok(Json(view(
        &service::get(
            &app,
            &c.identity,
            &c.workspace.id,
            c.org(),
            &c.sides,
            &id(&c)?,
        )
        .await?,
    )))
}

/// Dropping is a task: the entry vanishes now, and the bytes go when the worker has finished and checked.
async fn drop_one(State(app): State<Shared>, c: WsCtx) -> Result<(StatusCode, Json<Value>)> {
    let task = lifecycle::request_drop(
        &app,
        &c.identity,
        &c.workspace.id,
        c.org(),
        &c.sides,
        &id(&c)?,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"task_id": task.id}))))
}

async fn transfer(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<StatusCode> {
    let req: Transfer = body_json(&body)?;
    service::transfer(
        &app,
        &c.identity,
        &c.workspace.id,
        c.org(),
        &c.sides,
        &id(&c)?,
        &req.owner,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .post("/api/v1/workspaces/{ws}/securables", create)
        .get("/api/v1/workspaces/{ws}/securables", list)
        .get("/api/v1/workspaces/{ws}/securables/{id}", get)
        .delete("/api/v1/workspaces/{ws}/securables/{id}", drop_one)
        .put("/api/v1/workspaces/{ws}/securables/{id}/owner", transfer)
}
