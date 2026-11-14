//! The `MLflow` tracking address: `/api/v1/workspaces/{ws}/mlflow`. Point the standard client at it (`MLFLOW_TRACKING_URI`) with a Loamz
//! access token (`MLFLOW_TRACKING_TOKEN`). One wildcard route per method, and [`model::route`] decides what exists below it: anything
//! else answers 404. Errors are in `MLflow`'s own shape so the client understands them.

use axum::{
    Json,
    body::Bytes,
    extract::{RawQuery, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use super::{model, service};
use crate::{
    governance::context::WsCtx,
    kernel::{error::Error, http::Routes, state::Shared},
};

/// An error as `MLflow`'s client reads it: `{"error_code", "message"}`.
struct Refused(Error);

impl From<Error> for Refused {
    fn from(e: Error) -> Self {
        Self(e)
    }
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self.0 {
            Error::Invalid(m) | Error::Unsupported(m) => (
                StatusCode::BAD_REQUEST,
                "INVALID_PARAMETER_VALUE",
                m.clone(),
            ),
            Error::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "UNAUTHENTICATED",
                "sign in first".to_owned(),
            ),
            Error::NotFound(m) => (StatusCode::NOT_FOUND, "RESOURCE_DOES_NOT_EXIST", m.clone()),
            Error::Denied(_) => (
                StatusCode::FORBIDDEN,
                "PERMISSION_DENIED",
                "you do not have permission to do that".to_owned(),
            ),
            Error::Conflict(m) => (
                StatusCode::BAD_REQUEST,
                "RESOURCE_ALREADY_EXISTS",
                m.clone(),
            ),
            Error::TooManyRequests(m) => (
                StatusCode::TOO_MANY_REQUESTS,
                "REQUEST_LIMIT_EXCEEDED",
                m.clone(),
            ),
            Error::Unavailable(m) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "TEMPORARILY_UNAVAILABLE",
                m.clone(),
            ),
            other => {
                tracing::warn!(error = ?other, "an MLflow gateway request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "something went wrong on our side".to_owned(),
                )
            }
        };
        (
            status,
            Json(json!({"error_code": code, "message": message})),
        )
            .into_response()
    }
}

async fn handle(
    method: &'static str,
    app: Shared,
    c: WsCtx,
    query: Option<String>,
    body: Bytes,
) -> Result<Response, Refused> {
    let path = c.params.get("path").cloned().unwrap_or_default();
    let op = model::route(method, &path).ok_or_else(|| Error::NotFound("not found".into()))?;
    let query: Vec<(String, String)> = query
        .map(|q| {
            reqwest::Url::parse(&format!("http://x/?{q}"))
                .map(|u| {
                    u.query_pairs()
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let json: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).map_err(|_| Error::Invalid("the body is not JSON".into()))?
    };
    let answer = service::handle(&app, &c, op, &query, &json).await?;
    // `/version` is plain text; everything else is JSON.
    Ok(match answer {
        Value::String(text) if op == model::Op::Version => {
            ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
        }
        other => Json(other).into_response(),
    })
}

/// The tracking routes: what the standard `mlflow` client calls, below one workspace.
#[must_use]
pub fn routes() -> Routes {
    let base = "/api/v1/workspaces/{ws}/mlflow/{*path}";
    Routes::new()
        .get(
            base,
            |State(app): State<Shared>, c: WsCtx, RawQuery(q): RawQuery, body: Bytes| async move {
                handle("GET", app, c, q, body).await
            },
        )
        .post(
            base,
            |State(app): State<Shared>, c: WsCtx, RawQuery(q): RawQuery, body: Bytes| async move {
                handle("POST", app, c, q, body).await
            },
        )
}
