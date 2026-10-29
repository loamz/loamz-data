//! The routes that belong to no feature: health, readiness, and who am I.

use axum::{Json, extract::State};
use serde_json::{Value, json};

use super::{
    auth::{Caller, Kind},
    error::{Error, Result},
    http::Routes,
    state::Shared,
};

async fn healthz() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

/// Ready means the database answers.
async fn readyz(State(app): State<Shared>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1")
        .execute(app.db.pool())
        .await
        .map_err(|e| Error::Internal(format!("not ready: {e}")))?;
    Ok(Json(json!({"status": "ready"})))
}

async fn me(State(app): State<Shared>, caller: Caller) -> Json<Value> {
    Json(json!({
        "subject": caller.subject,
        "kind": match caller.kind { Kind::Person => "person", Kind::Service => "service" },
        "tenant": caller.tenant,
        "acting_for": caller.actor.as_ref().map(|_| &caller.subject),
        "agent": caller.actor,
        "tenant_owner": app.owns_root(&caller),
    }))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/healthz", healthz)
        .get("/readyz", readyz)
        .get("/api/v1/me", me)
}
