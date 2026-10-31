use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::service;
use crate::{
    governance::{context::OrgCtx, workspaces::service as workspaces},
    kernel::{
        auth::Identity,
        error::Result,
        http::{Routes, json as body_json},
        state::Shared,
    },
};

/// Who the token says the caller is, the organizations they belong to, and the workspaces they can open.
async fn me(State(app): State<Shared>, identity: Identity) -> Result<Json<Value>> {
    let memberships = service::memberships(&app, &identity).await?;
    let mut spaces = Vec::new();
    for m in &memberships {
        if !m.has_account {
            continue;
        }
        let mut mine = workspaces::mine(&app, &identity, &m.id).await?;
        // A person's own space always has somewhere to work.
        if mine.is_empty() && m.kind == "personal" {
            workspaces::create(&app, &identity, &m.id, "Personal").await?;
            mine = workspaces::mine(&app, &identity, &m.id).await?;
        }
        for (w, standing) in mine {
            spaces.push(json!({"id": w.id, "name": w.name, "org_id": m.id, "org_alias": m.alias, "admin": standing.is_admin()}));
        }
    }
    let orgs: Vec<Value> = memberships.iter().map(|m| json!({"id": m.id, "alias": m.alias, "kind": m.kind, "has_account": m.has_account, "account_admin": m.admin, "can_create_account": m.can_create})).collect();
    Ok(Json(json!({
        "subject": identity.subject,
        "name": identity.name,
        "email": identity.email,
        "kind": match identity.kind { crate::kernel::auth::Kind::Person => "person", crate::kernel::auth::Kind::Service => "service" },
        "groups": identity.groups,
        "agent": identity.actor,
        "orgs": orgs,
        "workspaces": spaces,
    })))
}

/// Creates the Data account of an organization the caller belongs to (needs the Keycloak role `account-admin`).
async fn create_account(
    State(app): State<Shared>,
    identity: Identity,
    Path(org): Path<String>,
) -> Result<(StatusCode, Json<Value>)> {
    let o = service::create(&app, &identity, &org).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": o.id, "alias": o.alias})),
    ))
}

async fn admins(State(app): State<Shared>, c: OrgCtx) -> Result<Json<Value>> {
    Ok(Json(
        json!({"items": service::admins(&app, &c.identity, &c.org).await?}),
    ))
}

#[derive(Deserialize)]
struct Principal {
    principal: String,
}

async fn add_admin(State(app): State<Shared>, c: OrgCtx, body: Bytes) -> Result<StatusCode> {
    let req: Principal = body_json(&body)?;
    service::add_admin(&app, &c.identity, &c.org, &req.principal).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_admin(State(app): State<Shared>, c: OrgCtx) -> Result<StatusCode> {
    let principal = c.params.get("principal").cloned().unwrap_or_default();
    service::remove_admin(&app, &c.identity, &c.org, &principal).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/me", me)
        .post("/api/v1/orgs/{org}/account", create_account)
        .get("/api/v1/orgs/{org}/admins", admins)
        .put("/api/v1/orgs/{org}/admins", add_admin)
        .delete("/api/v1/orgs/{org}/admins/{principal}", remove_admin)
}
