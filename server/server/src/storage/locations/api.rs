use axum::{Json, body::Bytes, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    model::{Credential, Location, Vending},
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

fn param(c: &WsCtx, name: &str) -> Result<String> {
    c.params
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))
}

fn credential(c: &Credential) -> Value {
    let vending = match &c.vending {
        Vending::None => json!({"kind": "none"}),
        Vending::Sts { role_arn } => json!({"kind": "sts", "role_arn": role_arn}),
        Vending::R2 { token_ref } => json!({"kind": "r2", "token_ref": token_ref}),
    };
    json!({"id": c.id, "name": c.name, "endpoint": c.endpoint, "region": c.region, "key_id_ref": c.key_id_ref, "secret_ref": c.secret_ref, "vending": vending, "created_by": c.created_by, "created_ms": c.created_ms})
}

fn location(l: &Location) -> Value {
    json!({"id": l.id, "name": l.name, "credential_id": l.credential_id, "bucket": l.bucket, "prefix": l.prefix, "created_by": l.created_by, "created_ms": l.created_ms})
}

#[derive(Deserialize)]
struct VendingBody {
    kind: String,
    #[serde(default)]
    role_arn: String,
    #[serde(default)]
    token_ref: String,
}

#[derive(Deserialize)]
struct CredentialBody {
    name: String,
    endpoint: String,
    #[serde(default)]
    region: String,
    key_id_ref: String,
    secret_ref: String,
    vending: VendingBody,
}

async fn create_credential(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: CredentialBody = body_json(&body)?;
    let vending = match req.vending.kind.as_str() {
        "none" => Vending::None,
        "sts" => Vending::Sts {
            role_arn: req.vending.role_arn,
        },
        "r2" => Vending::R2 {
            token_ref: req.vending.token_ref,
        },
        _ => return Err(Error::Invalid("vending is none, sts or r2".into())),
    };
    let made = service::create_credential(
        &app,
        &c.identity,
        &c.workspace.id,
        NewCredential {
            name: req.name,
            endpoint: req.endpoint,
            region: req.region,
            key_id_ref: req.key_id_ref,
            secret_ref: req.secret_ref,
            vending,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(credential(&made))))
}

async fn credentials(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    let items: Vec<Value> = service::credentials(&app, &c.identity, &c.workspace.id)
        .await?
        .iter()
        .map(credential)
        .collect();
    Ok(Json(json!({"items": items})))
}

async fn delete_credential(State(app): State<Shared>, c: WsCtx) -> Result<StatusCode> {
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
