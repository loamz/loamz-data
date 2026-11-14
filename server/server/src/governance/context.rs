//! What a request is about, worked out once at the edge: the organization or workspace in its address, who the caller is in
//! it, and what they speak as. Handlers take [`OrgCtx`], [`WsCtx`] or [`FileCtx`] and never repeat these checks.

use std::collections::HashMap;

use axum::{
    extract::{FromRequestParts, Path},
    http::{header::AUTHORIZATION, request::Parts},
};

use super::{
    orgs::{model::personal_id, service as orgs},
    workspaces::{
        model::{TOKEN_PREFIX, Workspace},
        principals, service as workspaces,
        service::Standing,
    },
};
use crate::{
    compute::service as compute,
    kernel::{
        auth::{AuthError, Identity, Kind, OrgRef},
        authz::Side,
        error::Error,
        state::Shared,
    },
};

/// Who a bearer token stands for: a person or a client by their `OpenID Connect` token, or a service principal by its own token.
/// What every door (the HTTP API, the BI connection) uses, so a token means the same everywhere.
///
/// # Errors
/// The token is not one this server accepts.
pub async fn identity_of(app: &Shared, token: &str) -> Result<Identity, Error> {
    if token.starts_with(TOKEN_PREFIX) {
        let sp = principals::authenticate(app, token)
            .await?
            .ok_or(Error::Unauthorized)?;
        return Ok(Identity {
            subject: sp.id,
            kind: Kind::Service,
            client: "service-principal".into(),
            name: sp.name,
            email: String::new(),
            groups: vec![],
            // For an organization the id is its name.
            orgs: vec![OrgRef {
                id: sp.org_id.clone(),
                alias: sp.org_id,
            }],
            roles: vec![],
            actor: None,
            scope: None,
            confined_to: Some(sp.workspace_id),
        });
    }
    match app.verifier.verify(token) {
        Ok(caller) => Ok(caller),
        Err(AuthError::UnknownKey) => {
            // The provider may have rotated its keys: look once more, then decide.
            if let Err(e) = app.verifier.refresh().await {
                tracing::warn!(error = e, "could not refresh the signing keys");
            }
            app.verifier.verify(token).map_err(|_| Error::Unauthorized)
        }
        Err(_) => Err(Error::Unauthorized),
    }
}

/// Who is calling: a person or a client by their `OpenID Connect` token, or a service principal by its own token.
impl FromRequestParts<Shared> for Identity {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self, Error> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(Error::Unauthorized)?;
        identity_of(app, token).await
    }
}

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

    /// The context of a workspace for a caller already known: who they are in it, and what they speak as.
    ///
    /// # Errors
    /// No such workspace, or the caller may not see it.
    pub async fn open(app: &Shared, identity: Identity, workspace: &str) -> Result<Self, Error> {
        let (workspace, standing) = workspaces::open(app, &identity, workspace).await?;
        let sides = orgs::sides(app, &identity, &workspace.org_id).await?;
        Ok(Self {
            identity,
            workspace,
            standing,
            sides,
            params: HashMap::new(),
        })
    }

    async fn build(parts: &mut Parts, app: &Shared, identity: Identity) -> Result<Self, Error> {
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, app)
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let mut ctx = Self::open(app, identity, &param(&params, "ws")?).await?;
        ctx.params = params;
        Ok(ctx)
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
