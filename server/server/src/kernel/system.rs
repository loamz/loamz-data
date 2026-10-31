//! The routes that belong to no feature: health and readiness.

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    auth::AUDIENCE,
    error::{Error, Result},
    http::{Routes, json as body_json},
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

/// What a page may ask the sign-in service for through us. Nothing else is accepted, and the client is always ours.
#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    code_verifier: String,
    #[serde(default)]
    refresh_token: String,
}

fn need(field: &str, name: &str) -> Result<()> {
    if field.is_empty() {
        Err(Error::Invalid(format!("{name} is required")))
    } else {
        Ok(())
    }
}

/// The sign-in token exchange, done by the server for the browser.
///
/// Every workspace is a website of its own, and the identity provider only lets exact addresses call its token endpoint from a
/// page. Rather than open it to every address, the page asks its own server, which asks the identity provider directly. Only two
/// requests pass (exchange a code, renew a session), always as the `loamz-data` client; nothing is stored or logged.
async fn token(State(app): State<Shared>, body: Bytes) -> Result<Response> {
    let r: TokenRequest = body_json(&body)?;
    if [&r.code, &r.redirect_uri, &r.code_verifier, &r.refresh_token]
        .iter()
        .any(|f| f.len() > 4096)
    {
        return Err(Error::Invalid("the request is too large".into()));
    }
    let form: Vec<(&str, &str)> = match r.grant_type.as_str() {
        "authorization_code" => {
            need(&r.code, "code")?;
            need(&r.redirect_uri, "redirect_uri")?;
            need(&r.code_verifier, "code_verifier")?;
            vec![
                ("grant_type", "authorization_code"),
                ("client_id", AUDIENCE),
                ("code", &r.code),
                ("redirect_uri", &r.redirect_uri),
                ("code_verifier", &r.code_verifier),
            ]
        }
        "refresh_token" => {
            need(&r.refresh_token, "refresh_token")?;
            vec![
                ("grant_type", "refresh_token"),
                ("client_id", AUDIENCE),
                ("refresh_token", &r.refresh_token),
            ]
        }
        _ => {
            return Err(Error::Invalid(
                "grant_type must be authorization_code or refresh_token".into(),
            ));
        }
    };
    let (status, text) = app.verifier.exchange(&form).await.map_err(|e| {
        tracing::warn!(
            error = e,
            "the sign-in service did not answer a token request"
        );
        Error::Unavailable("the sign-in service did not answer".into())
    })?;
    // The identity provider's own answer, success or refusal; any other status is not something to pass on.
    let code = match status {
        200 | 400 | 401 => StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
        _ => StatusCode::BAD_GATEWAY,
    };
    Ok((
        code,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        text,
    )
        .into_response())
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/healthz", healthz)
        .get("/readyz", readyz)
        .post("/api/v1/auth/token", token)
}
