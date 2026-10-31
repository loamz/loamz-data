//! The routes that belong to no feature: health and readiness.

use axum::{Json, extract::State};
use serde_json::{Value, json};

use super::{
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

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/healthz", healthz)
        .get("/readyz", readyz)
}
