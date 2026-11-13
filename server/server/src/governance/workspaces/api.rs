use axum::{Json, body::Bytes, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    model::{Role, ServicePrincipal, Workspace},
    principals,
    service::{self, Standing},
};
use crate::{
    governance::{
        catalogs::service as catalogs,
        context::{OrgCtx, WsCtx},
    },
    kernel::{
        error::{Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
};

fn view(w: &Workspace, standing: Standing) -> Value {
    json!({"id": w.id, "org_id": w.org_id, "name": w.name, "created_ms": w.created_ms, "admin": standing.is_admin(), "account_admin": standing == Standing::Account})
}

fn param(c: &WsCtx, name: &str) -> Result<String> {
    c.params
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

#[derive(Deserialize)]
struct Assign {
    principal: String,
    role: String,
}

#[derive(Deserialize)]
struct Entitle {
    principal: String,
    entitlement: String,
}

async fn list(State(app): State<Shared>, c: OrgCtx) -> Result<Json<Value>> {
    let items: Vec<Value> = service::mine(&app, &c.identity, &c.org)
        .await?
        .iter()
        .map(|(w, s)| view(w, *s))
        .collect();
    Ok(Json(json!({"items": items})))
}

async fn create(
    State(app): State<Shared>,
    c: OrgCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: Name = body_json(&body)?;
    let w = service::create(&app, &c.identity, &c.org, &req.name).await?;
    Ok((StatusCode::CREATED, Json(view(&w, Standing::Account))))
}

async fn get(c: WsCtx) -> Json<Value> {
    Json(view(&c.workspace, c.standing))
}

async fn rename(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<Json<Value>> {
    let req: Name = body_json(&body)?;
    let w = service::rename(&app, &c.identity, &c.workspace.id, &req.name).await?;
    Ok(Json(view(&w, c.standing)))
}

async fn delete(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    // The workspace owns its catalogs and its compute; one that still has any cannot go (they would have nowhere to be).
    let catalogs = catalogs::count_in_workspace(&app, c.org(), &c.workspace.id).await?
        + crate::compute::service::count_in_workspace(&app, &c.workspace.id).await?;
    service::delete(&app, &c.identity, &c.workspace.id, catalogs).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn members(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let items: Vec<Value> = service::members(&app, &c.identity, &c.workspace.id)
        .await?
        .into_iter()
        .map(|m| json!({"principal": m.principal, "role": m.role.as_str()}))
        .collect();
    Ok(Json(json!({"items": items})))
}

async fn assign(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<StatusCode> {
    let req: Assign = body_json(&body)?;
    let role = Role::parse(&req.role)
        .ok_or_else(|| Error::Invalid("role must be admin or user".into()))?;
    service::assign(&app, &c.identity, &c.workspace.id, &req.principal, role).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unassign(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    service::unassign(&app, &c.identity, &c.workspace.id, &param(&c, "principal")?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn entitlements(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let items: Vec<Value> = service::entitlements(&app, &c.identity, &c.workspace.id)
        .await?
        .into_iter()
        .map(|(p, e)| json!({"principal": p, "entitlement": e}))
        .collect();
    Ok(Json(
        json!({"items": items, "available": super::model::ENTITLEMENTS}),
    ))
}

async fn entitle(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Result<StatusCode> {
    let req: Entitle = body_json(&body)?;
    service::set_entitlement(
        &app,
        &c.identity,
        &c.workspace.id,
        &req.principal,
        &req.entitlement,
        true,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unentitle(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    service::set_entitlement(
        &app,
        &c.identity,
        &c.workspace.id,
        &param(&c, "principal")?,
        &param(&c, "entitlement")?,
        false,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn service_principal(sp: &ServicePrincipal) -> Value {
    json!({"id": sp.id, "name": sp.name, "principal": sp.principal(), "created_by": sp.created_by, "created_ms": sp.created_ms})
}

/// Makes a service principal. Its token is in the answer, once.
async fn create_service_principal(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: Name = body_json(&body)?;
    let (sp, token) = principals::create(&app, &c.identity, &c.workspace.id, &req.name).await?;
    let mut shown = service_principal(&sp);
    shown["token"] = json!(token);
    Ok((StatusCode::CREATED, Json(shown)))
}

async fn service_principals(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let items: Vec<Value> = principals::list(&app, &c.identity, &c.workspace.id)
        .await?
        .iter()
        .map(service_principal)
        .collect();
    Ok(Json(json!({"items": items})))
}

async fn renew_service_principal_token(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let token = principals::rotate(&app, &c.identity, &c.workspace.id, &param(&c, "id")?).await?;
    Ok(Json(json!({"token": token})))
}

async fn delete_service_principal(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    principals::delete(&app, &c.identity, &c.workspace.id, &param(&c, "id")?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/orgs/{org}/workspaces", list)
        .post("/api/v1/orgs/{org}/workspaces", create)
        .get("/api/v1/workspaces/{ws}", get)
        .put("/api/v1/workspaces/{ws}", rename)
        .delete("/api/v1/workspaces/{ws}", delete)
        .get("/api/v1/workspaces/{ws}/members", members)
        .put("/api/v1/workspaces/{ws}/members", assign)
        .delete("/api/v1/workspaces/{ws}/members/{principal}", unassign)
        .get("/api/v1/workspaces/{ws}/entitlements", entitlements)
        .put("/api/v1/workspaces/{ws}/entitlements", entitle)
        .delete(
            "/api/v1/workspaces/{ws}/entitlements/{principal}/{entitlement}",
            unentitle,
        )
        .get(
            "/api/v1/workspaces/{ws}/service-principals",
            service_principals,
        )
        .post(
            "/api/v1/workspaces/{ws}/service-principals",
            create_service_principal,
        )
        .post(
            "/api/v1/workspaces/{ws}/service-principals/{id}/token",
            renew_service_principal_token,
        )
        .delete(
            "/api/v1/workspaces/{ws}/service-principals/{id}",
            delete_service_principal,
        )
}
