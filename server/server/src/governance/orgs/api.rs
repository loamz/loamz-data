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
        // A person's own space, and an organization its admin has just set up, always have somewhere to work.
        if mine.is_empty() && (m.kind == "personal" || m.admin) {
            let name = if m.kind == "personal" {
                "Personal"
            } else {
                "Main"
            };
            workspaces::create(&app, &identity, &m.id, name).await?;
            mine = workspaces::mine(&app, &identity, &m.id).await?;
        }
        for (w, standing) in mine {
            spaces.push(json!({"id": w.id, "name": w.name, "org_id": m.id, "org_alias": m.alias, "admin": standing.is_admin()}));
        }
    }
    let orgs: Vec<Value> = memberships.iter().map(|m| json!({"id": m.id, "alias": m.alias, "kind": m.kind, "has_account": m.has_account, "account_admin": m.admin})).collect();
    Ok(Json(json!({
        "subject": identity.subject,
        "name": identity.name,
        "email": identity.email,
        "platform_admin": identity.has_role(crate::kernel::auth::PLATFORM_ADMIN_ROLE),
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

#[derive(Deserialize)]
struct NewOrg {
    alias: String,
    admin_email: String,
    admin_name: String,
}

fn invited(i: &super::model::Invited) -> Value {
    json!({"subject": i.subject, "email": i.email, "temporary_password": i.temporary_password})
}

/// Makes an organization and its first admin (the installation's admin only). The temporary password is shown once.
async fn create_org(
    State(app): State<Shared>,
    identity: Identity,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: NewOrg = body_json(&body)?;
    let (o, first) = service::create_org(
        &app,
        &identity,
        &req.alias,
        &req.admin_email,
        &req.admin_name,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": o.id, "alias": o.alias, "admin": invited(&first)})),
    ))
}

#[derive(Deserialize)]
struct NewPerson {
    email: String,
    name: String,
}

/// Adds a person to the organization; a new one gets a temporary password, shown once.
async fn add_person(
    State(app): State<Shared>,
    c: OrgCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: NewPerson = body_json(&body)?;
    let added = service::add_person(&app, &c.identity, &c.org, &req.email, &req.name).await?;
    Ok((StatusCode::CREATED, Json(invited(&added))))
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
        .post("/api/v1/orgs", create_org)
        .post("/api/v1/orgs/{org}/account", create_account)
        .post("/api/v1/orgs/{org}/people", add_person)
        .get("/api/v1/orgs/{org}/admins", admins)
        .put("/api/v1/orgs/{org}/admins", add_admin)
        .delete("/api/v1/orgs/{org}/admins/{principal}", remove_admin)
}
