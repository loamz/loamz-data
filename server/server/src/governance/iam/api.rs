use axum::{Json, body::Bytes, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::service;
use crate::{
    governance::{catalogs::service as catalogs, context::WsCtx},
    kernel::{
        authz::{Role, Target, metastore},
        error::{Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
};

/// `metastore` stands for the organization's metastore, where catalogs hang.
async fn resolve(app: &Shared, c: &WsCtx) -> Result<Target> {
    let id = c
        .params
        .get("id")
        .ok_or_else(|| Error::Internal("the route has no id".into()))?;
    if id == "metastore" {
        Ok(metastore(c.org()))
    } else {
        Ok(catalogs::target_in(app, &c.workspace.id, c.org(), id)
            .await?
            .1)
    }
}

#[derive(Deserialize)]
struct Assign {
    principal: String,
    role: String,
}

async fn assign(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<StatusCode> {
    let req: Assign = body_json(&body)?;
    let role = Role::parse(&req.role)
        .ok_or_else(|| Error::Invalid("role must be reader, contributor or owner".into()))?;
    let target = resolve(&app, &c).await?;
    service::assign(
        &app,
        &c.identity,
        c.org(),
        &c.sides,
        &target,
        &req.principal,
        role,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    let principal = c.params.get("principal").cloned().unwrap_or_default();
    let target = resolve(&app, &c).await?;
    service::revoke(&app, &c.identity, c.org(), &c.sides, &target, &principal).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let target = resolve(&app, &c).await?;
    let held = service::list(&app, &c.identity, c.org(), &c.sides, &target).await?;
    let items: Vec<Value> = held.into_iter().map(|(principal, privileges)| json!({"principal": principal, "role": Role::of(&privileges).map(Role::as_str), "privileges": privileges})).collect();
    Ok(Json(json!({"items": items})))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .put("/api/v1/workspaces/{ws}/securables/{id}/grants", assign)
        .get("/api/v1/workspaces/{ws}/securables/{id}/grants", list)
        .delete(
            "/api/v1/workspaces/{ws}/securables/{id}/grants/{principal}",
            revoke,
        )
}
