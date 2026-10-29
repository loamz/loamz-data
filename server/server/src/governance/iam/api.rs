use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{model::role_of, service};
use crate::{
    governance::catalogs::service as catalogs,
    kernel::{
        auth::Caller,
        authz::{Role, Target, root},
        error::{Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
};

/// `root` stands for the tenant itself, where catalogs hang.
async fn resolve(app: &Shared, caller: &Caller, id: &str) -> Result<Target> {
    if id == "root" {
        Ok(root(&caller.tenant))
    } else {
        Ok(catalogs::target(app, &caller.tenant, id).await?.1)
    }
}

#[derive(Deserialize)]
struct Assign {
    principal: String,
    role: String,
}

async fn assign(
    State(app): State<Shared>,
    caller: Caller,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<StatusCode> {
    let req: Assign = body_json(&body)?;
    let role = Role::parse(&req.role)
        .ok_or_else(|| Error::Invalid("role must be reader, contributor or owner".into()))?;
    let target = resolve(&app, &caller, &id).await?;
    service::assign(&app, &caller, &target, &req.principal, role).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke(
    State(app): State<Shared>,
    caller: Caller,
    Path((id, principal)): Path<(String, String)>,
) -> Result<StatusCode> {
    let target = resolve(&app, &caller, &id).await?;
    service::revoke(&app, &caller, &target, &principal).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list(
    State(app): State<Shared>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let target = resolve(&app, &caller, &id).await?;
    let held = service::list(&app, &caller, &target).await?;
    let items: Vec<Value> = held
        .into_iter()
        .map(|(principal, privileges)| json!({"principal": principal, "role": role_of(&privileges).map(Role::as_str), "privileges": privileges}))
        .collect();
    Ok(Json(json!({"items": items})))
}

#[must_use]
pub fn routes() -> Routes {
    Routes::new()
        .put("/api/v1/securables/{id}/grants", assign)
        .get("/api/v1/securables/{id}/grants", list)
        .delete("/api/v1/securables/{id}/grants/{principal}", revoke)
}
