use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::service;
use crate::{
    governance::iam::service as iam,
    kernel::{
        auth::Caller,
        authz::{Privilege, root},
        error::Result,
        http::Routes,
        state::Shared,
    },
};

#[derive(Deserialize)]
struct Params {
    limit: Option<i64>,
}

/// The tenant's recent entries. Reading the trail needs `manage` on the tenant.
async fn list(
    State(app): State<Shared>,
    caller: Caller,
    Query(p): Query<Params>,
) -> Result<Json<Value>> {
    iam::require(&app, &caller, Privilege::Manage, &root(&caller.tenant)).await?;
    let entries = service::recent(&app, &caller.tenant, p.limit.unwrap_or(100)).await?;
    let items: Vec<Value> = entries
        .into_iter()
        .map(|e| json!({"seq": e.seq, "at_ms": e.at_ms, "actor": e.actor, "on_behalf_of": e.on_behalf_of, "action": e.action, "target": e.target, "outcome": e.outcome}))
        .collect();
    Ok(Json(json!({"items": items})))
}

/// Walks the whole chain. It says only whether it holds and where it first breaks.
async fn verify(State(app): State<Shared>, caller: Caller) -> Result<Json<Value>> {
    iam::require(&app, &caller, Privilege::Manage, &root(&caller.tenant)).await?;
    let v = service::verify(&app).await?;
    Ok(Json(
        json!({"intact": v.first_broken.is_none(), "entries": v.entries, "first_broken": v.first_broken}),
    ))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/audit", list)
        .get("/api/v1/audit/verify", verify)
}
