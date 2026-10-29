use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};

use super::service;
use crate::kernel::{auth::Caller, error::Result, http::Routes, state::Shared};

async fn put(
    State(app): State<Shared>,
    caller: Caller,
    Path((id, path)): Path<(String, String)>,
    body: Body,
) -> Result<(StatusCode, Json<Value>)> {
    let size = service::put(&app, &caller, &id, &path, body).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"path": path, "size": size})),
    ))
}

/// Always sent as a download that the browser may not guess the type of: a file in a volume is data, never a page.
async fn get(
    State(app): State<Shared>,
    caller: Caller,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response> {
    let (body, size) = service::get(&app, &caller, &id, &path).await?;
    let mut res = Response::new(body);
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    Ok(res)
}

async fn remove(
    State(app): State<Shared>,
    caller: Caller,
    Path((id, path)): Path<(String, String)>,
) -> Result<StatusCode> {
    service::delete(&app, &caller, &id, &path).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list(
    State(app): State<Shared>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let items: Vec<Value> = service::list(&app, &caller, &id)
        .await?
        .into_iter()
        .map(|e| json!({"path": e.path, "size": e.size}))
        .collect();
    Ok(Json(json!({"items": items})))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/volumes/{id}/files", list)
        .put("/api/v1/volumes/{id}/files/{*path}", put)
        .get("/api/v1/volumes/{id}/files/{*path}", get)
        .delete("/api/v1/volumes/{id}/files/{*path}", remove)
}
