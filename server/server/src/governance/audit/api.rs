use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::service;
use crate::{
    governance::{context::OrgCtx, orgs::service as orgs},
    kernel::{error::Result, http::Routes, state::Shared},
};

#[derive(Deserialize)]
struct Params {
    limit: Option<i64>,
}

/// The organization's recent entries. Reading the trail is for account admins.
async fn list(
    State(app): State<Shared>,
    c: OrgCtx,
    Query(p): Query<Params>,
) -> Result<Json<Value>> {
    orgs::require_admin(&app, &c.identity, &c.org).await?;
    let entries = service::recent(&app, &c.org, p.limit.unwrap_or(100)).await?;
    let items: Vec<Value> = entries
        .into_iter()
        .map(|e| json!({"seq": e.seq, "at_ms": e.at_ms, "actor": e.actor, "on_behalf_of": e.on_behalf_of, "action": e.action, "target": e.target, "outcome": e.outcome}))
        .collect();
    Ok(Json(json!({"items": items})))
}

/// Walks the organization's whole chain. It says only whether it holds and where it first breaks.
async fn verify(State(app): State<Shared>, c: OrgCtx) -> Result<Json<Value>> {
    orgs::require_admin_hidden(&app, &c.identity, &c.org).await?;
    let v = service::verify(&app, &c.org).await?;
    Ok(Json(
        json!({"intact": v.first_broken.is_none(), "entries": v.entries, "first_broken": v.first_broken}),
    ))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/orgs/{org}/audit", list)
        .get("/api/v1/orgs/{org}/audit/verify", verify)
}
