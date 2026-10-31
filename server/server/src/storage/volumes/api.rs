use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};

use super::service::{self, Ask};
use crate::{
    governance::context::WsCtx,
    kernel::{
        error::{Error, Result},
        http::Routes,
        state::Shared,
    },
};

fn ask(c: &WsCtx) -> Ask<'_> {
    Ask {
        identity: &c.identity,
        workspace: &c.workspace.id,
        org: c.org(),
        sides: &c.sides,
    }
}

fn param(c: &WsCtx, name: &str) -> Result<String> {
    c.params
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))
}

async fn put(State(app): State<Shared>, c: WsCtx, body: Body) -> Result<(StatusCode, Json<Value>)> {
    let (id, path) = (param(&c, "id")?, param(&c, "path")?);
    let size = service::put(&app, &ask(&c), &id, &path, body).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"path": path, "size": size})),
    ))
}

/// Always sent as a download that the browser may not guess the type of: a file in a volume is data, never a page.
async fn get(State(app): State<Shared>, c: WsCtx) -> Result<Response> {
    let (id, path) = (param(&c, "id")?, param(&c, "path")?);
    let (body, size) = service::get(&app, &ask(&c), &id, &path).await?;
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

async fn remove(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    let (id, path) = (param(&c, "id")?, param(&c, "path")?);
    service::delete(&app, &ask(&c), &id, &path).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let items: Vec<Value> = service::list(&app, &ask(&c), &param(&c, "id")?)
        .await?
        .into_iter()
        .map(|e| json!({"path": e.path, "size": e.size}))
        .collect();
    Ok(Json(json!({"items": items})))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/workspaces/{ws}/volumes/{id}/files", list)
        .put("/api/v1/workspaces/{ws}/volumes/{id}/files/{*path}", put)
        .get("/api/v1/workspaces/{ws}/volumes/{id}/files/{*path}", get)
        .delete("/api/v1/workspaces/{ws}/volumes/{id}/files/{*path}", remove)
}
