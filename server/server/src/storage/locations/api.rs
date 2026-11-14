use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    model::{Credential, Location},
    service::{self, NewCredential, NewLocation},
};
use crate::{
    governance::context::WsCtx,
    kernel::{
        error::{Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
};

/// Keeps credential metadata, denials and errors out of caches.
pub async fn no_store(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = request.uri().path();
    let credential_route =
        path.starts_with("/api/v1/workspaces/") && path.contains("/storage/credentials");
    let mut response = next.run(request).await;
    if credential_route {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
    }
    response
}

fn safe_transport(
    peer: Option<std::net::SocketAddr>,
    forwarded: Option<&str>,
    trusted: Option<std::net::IpAddr>,
    dev: bool,
) -> bool {
    peer.is_some_and(|p| {
        (dev && p.ip().is_loopback()) || (Some(p.ip()) == trusted && forwarded == Some("https"))
    })
}

fn require_tls(
    app: &Shared,
    peer: ConnectInfo<std::net::SocketAddr>,
    headers: &HeaderMap,
) -> Result<()> {
    let forwarded = if headers.get_all("x-forwarded-proto").iter().count() == 1 {
        headers
            .get("x-forwarded-proto")
            .and_then(|h| h.to_str().ok())
    } else {
        None
    };
    if safe_transport(
        Some(peer.0),
        forwarded,
        app.storage_tls_proxy,
        app.storage_allow_http_dev,
    ) {
        Ok(())
    } else {
        Err(Error::Invalid(
            "a trusted TLS connection is required".into(),
        ))
    }
}

fn param(c: &WsCtx, name: &str) -> Result<String> {
    c.params
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))
}

fn credential(c: &Credential) -> Value {
    json!({"id": c.id, "name": c.name, "provider": c.provider, "endpoint": c.endpoint, "region": c.region, "generation": c.generation, "created_by": c.created_by, "created_ms": c.created_ms})
}

fn location(l: &Location) -> Value {
    json!({"id": l.id, "name": l.name, "credential_id": l.credential_id, "bucket": l.bucket, "prefix": l.prefix, "created_by": l.created_by, "created_ms": l.created_ms})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialBody {
    name: String,
    provider: String,
    endpoint: String,
    #[serde(default)]
    region: String,
    secret: Value,
}

async fn create_credential(
    State(app): State<Shared>,
    c: WsCtx,
    peer: ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    require_tls(&app, peer, &headers)?;
    service::authorize_credential(&app, &c.identity, &c.workspace.id).await?;
    if body.len() > 32768 {
        return Err(Error::TooLarge);
    }
    let req: CredentialBody = serde_json::from_slice(&body)
        .map_err(|_| Error::Invalid("invalid credential body".into()))?;
    let made = service::create_credential(
        &app,
        &c.identity,
        &c.workspace.id,
        NewCredential {
            name: req.name,
            provider: req.provider,
            endpoint: req.endpoint,
            region: req.region,
            secret: req.secret,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(credential(&made))))
}

async fn get_credential(
    State(app): State<Shared>,
    c: WsCtx,
    peer: ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<Value>)> {
    require_tls(&app, peer, &headers)?;
    let found = service::credential(&app, &c.identity, &c.workspace.id, &param(&c, "id")?).await?;
    let mut headers = HeaderMap::new();
    let etag = axum::http::HeaderValue::from_str(&format!("\"{}\"", found.generation))
        .map_err(|_| Error::Internal("invalid credential generation".into()))?;
    headers.insert(axum::http::header::ETAG, etag);
    Ok((headers, Json(credential(&found))))
}

async fn rotate_credential(
    State(app): State<Shared>,
    c: WsCtx,
    peer: ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    require_tls(&app, peer, &headers)?;
    let id = param(&c, "id")?;
    service::credential(&app, &c.identity, &c.workspace.id, &id).await?;
    if body.len() > 32768 {
        return Err(Error::TooLarge);
    }
    let expected = headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix('"').and_then(|v| v.strip_suffix('"')))
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| Error::Invalid("if-match must be a generation".into()))?;
    let secret: Value = serde_json::from_slice(&body)
        .map_err(|_| Error::Invalid("invalid credential body".into()))?;
    service::rotate_credential(&app, &c.identity, &c.workspace.id, &id, expected, secret).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckBody {
    location_id: String,
}

async fn check_credential(
    State(app): State<Shared>,
    c: WsCtx,
    peer: ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    require_tls(&app, peer, &headers)?;
    let id = param(&c, "id")?;
    service::credential(&app, &c.identity, &c.workspace.id, &id).await?;
    if body.len() > 4096 {
        return Err(Error::TooLarge);
    }
    let req: CheckBody =
        serde_json::from_slice(&body).map_err(|_| Error::Invalid("invalid check body".into()))?;
    let (generation, status) =
        service::check_credential(&app, &c.identity, &c.workspace.id, &id, &req.location_id)
            .await?;
    Ok((
        StatusCode::OK,
        Json(json!({"status": status, "generation": generation})),
    ))
}

async fn credentials(
    State(app): State<Shared>,
    c: WsCtx,
    peer: ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    require_tls(&app, peer, &headers)?;
    let items: Vec<Value> = service::credentials(&app, &c.identity, &c.workspace.id)
        .await?
        .iter()
        .map(credential)
        .collect();
    Ok(Json(json!({"items": items})))
}

async fn delete_credential(
    State(app): State<Shared>,
    c: WsCtx,
    peer: ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    require_tls(&app, peer, &headers)?;
    service::delete_credential(&app, &c.identity, &c.workspace.id, &param(&c, "id")?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct LocationBody {
    name: String,
    credential_id: String,
    bucket: String,
    #[serde(default)]
    prefix: String,
}

async fn create_location(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: LocationBody = body_json(&body)?;
    let made = service::create_location(
        &app,
        &c.identity,
        &c.workspace.id,
        NewLocation {
            name: req.name,
            credential_id: req.credential_id,
            bucket: req.bucket,
            prefix: req.prefix,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(location(&made))))
}

async fn locations(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let items: Vec<Value> = service::locations(&app, &c.identity, &c.workspace.id)
        .await?
        .iter()
        .map(location)
        .collect();
    Ok(Json(json!({"items": items})))
}

async fn delete_location(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
    service::delete_location(&app, &c.identity, &c.workspace.id, &param(&c, "id")?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .get("/api/v1/workspaces/{ws}/storage/credentials", credentials)
        .post(
            "/api/v1/workspaces/{ws}/storage/credentials",
            create_credential,
        )
        .get(
            "/api/v1/workspaces/{ws}/storage/credentials/{id}",
            get_credential,
        )
        .post(
            "/api/v1/workspaces/{ws}/storage/credentials/{id}/check",
            check_credential,
        )
        .put(
            "/api/v1/workspaces/{ws}/storage/credentials/{id}/secret",
            rotate_credential,
        )
        .delete(
            "/api/v1/workspaces/{ws}/storage/credentials/{id}",
            delete_credential,
        )
        .get("/api/v1/workspaces/{ws}/storage/locations", locations)
        .post("/api/v1/workspaces/{ws}/storage/locations", create_location)
        .delete(
            "/api/v1/workspaces/{ws}/storage/locations/{id}",
            delete_location,
        )
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    #[test]
    fn only_trusted_tls_proxy_or_explicit_dev_is_accepted() {
        let trusted = "10.0.0.2".parse().ok();
        let right = "10.0.0.2:5000".parse().ok();
        let wrong = "10.0.0.3:5000".parse().ok();
        assert!(safe_transport(right, Some("https"), trusted, false));
        assert!(!safe_transport(wrong, Some("https"), trusted, false));
        assert!(!safe_transport(right, Some("http"), trusted, false));
        assert!(!safe_transport(None, Some("https"), trusted, false));
        assert!(safe_transport(
            "127.0.0.1:5000".parse().ok(),
            None,
            None,
            true
        ));
        assert!(!safe_transport(None, None, None, true));
    }
}
