//! What a request is about, worked out once at the edge: the organization or workspace in its address, who the caller is in
//! it, and what they speak as. Handlers take [`OrgCtx`], [`WsCtx`] or [`FileCtx`] and never repeat these checks.

use std::collections::HashMap;

use axum::{
    extract::{FromRequestParts, Path},
    http::{header::AUTHORIZATION, request::Parts},
};

use super::{
    orgs::{model::personal_id, service as orgs},
    workspaces::{model::Workspace, service as workspaces, service::Standing},
};
use crate::{
    compute::service as compute,
    kernel::{auth::Identity, authz::Side, error::Error, state::Shared},
};

fn param(p: &HashMap<String, String>, name: &str) -> Result<String, Error> {
    p.get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))
}

/// A request inside an organization (`/api/v1/orgs/{org}/…`).
pub struct OrgCtx {
    pub identity: Identity,
    pub org: String,
    pub sides: Vec<Side>,
    pub params: HashMap<String, String>,
}

impl FromRequestParts<Shared> for OrgCtx {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self, Error> {
        let identity = Identity::from_request_parts(parts, app).await?;
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, app)
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let org = param(&params, "org")?;
        if org == personal_id(&identity.subject) {
            orgs::ensure_personal(app, &identity).await?;
        }
        orgs::get(app, &identity, &org).await?;
        let sides = orgs::sides(app, &identity, &org).await?;
        Ok(Self {
            identity,
            org,
            sides,
            params,
        })
    }
}

/// A request inside a workspace (`/api/v1/workspaces/{ws}/…`).
pub struct WsCtx {
    pub identity: Identity,
    pub workspace: Workspace,
    pub standing: Standing,
    pub sides: Vec<Side>,
    pub params: HashMap<String, String>,
}

impl WsCtx {
    #[must_use]
    pub fn org(&self) -> &str {
        &self.workspace.org_id
    }

    async fn build(parts: &mut Parts, app: &Shared, identity: Identity) -> Result<Self, Error> {
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, app)
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let (workspace, standing) =
            workspaces::open(app, &identity, &param(&params, "ws")?).await?;
        let sides = orgs::sides(app, &identity, &workspace.org_id).await?;
        Ok(Self {
            identity,
            workspace,
            standing,
            sides,
            params,
        })
    }
}

impl FromRequestParts<Shared> for WsCtx {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self, Error> {
        let identity = Identity::from_request_parts(parts, app).await?;
        Self::build(parts, app, identity).await
    }
}

/// A request for the files of a volume or table. Besides a person's own token it accepts a running job's token (`jt_…`): that person's
/// rights, narrowed to what the job declared, for exactly one workspace. Nothing else accepts one.
pub struct FileCtx(pub WsCtx);

impl FromRequestParts<Shared> for FileCtx {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self, Error> {
        let bearer = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let Some(token) = bearer.filter(|t| t.starts_with("jt_")) else {
            let identity = Identity::from_request_parts(parts, app).await?;
            return WsCtx::build(parts, app, identity).await.map(Self);
        };
        let (identity, workspace) = compute::identity_for_token(app, token).await?;
        let ctx = WsCtx::build(parts, app, identity).await?;
        // A token is for one workspace: another workspace's address does not exist for it.
        if ctx.workspace.id != workspace {
            return Err(Error::NotFound("not found".into()));
        }
        Ok(Self(ctx))
    }
}
