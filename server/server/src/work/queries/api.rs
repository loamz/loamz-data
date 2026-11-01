use axum::{Json, body::Bytes, extract::State, http::StatusCode};
use serde_json::{Value, json};

use super::{model::Draft, service};
use crate::{
    compute::service::Ctx,
    governance::context::WsCtx,
    kernel::{
        error::{Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
};

fn ctx(c: &WsCtx) -> Ctx<'_> {
    Ctx {
        identity: &c.identity,
        workspace: &c.workspace,
        standing: c.standing,
        sides: &c.sides,
    }
}

fn id(c: &WsCtx) -> Result<String> {
    c.params
        .get("id")
        .cloned()
        .ok_or_else(|| Error::Internal("the route has no id".into()))
}

async fn list(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    Ok(Json(json!({"items": service::list(&app, &ctx(&c)).await?})))
}

async fn create(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let q = service::create(&app, &ctx(&c), body_json::<Draft>(&body)?).await?;
    Ok((StatusCode::CREATED, Json(json!(q))))
}

async fn get(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    Ok(Json(json!(service::get(&app, &ctx(&c), &id(&c)?).await?)))
}

async fn update(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<Json<Value>> {
    let q = service::update(&app, &ctx(&c), &id(&c)?, body_json::<Draft>(&body)?).await?;
    Ok(Json(json!(q)))
}

async fn delete(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    service::delete(&app, &ctx(&c), &id(&c)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/workspaces/{ws}/queries", list)
        .post("/api/v1/workspaces/{ws}/queries", create)
        .get("/api/v1/workspaces/{ws}/queries/{id}", get)
        .put("/api/v1/workspaces/{ws}/queries/{id}", update)
        .delete("/api/v1/workspaces/{ws}/queries/{id}", delete)
}
