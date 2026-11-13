//! Service principals of a workspace: identities that are not people (a BI tool, an ML app, a pipeline). A workspace's admins make
//! them and hold their tokens; a token is shown once and only its hash is kept. A service principal is assigned to its own
//! workspace as a plain user, is named in grants as `sp:<id>`, and can open no other workspace.

use super::{
    model::{Role, ServicePrincipal, TOKEN_PREFIX, valid_name},
    service::{deny, open},
    store,
};
use crate::{
    governance::audit::{model::Who, service::Uow},
    kernel::{
        auth::Identity,
        error::{Error, Result},
        hash_secret, new_id, new_secret, now_ms,
        state::App,
    },
};

/// The workspace's admins may manage its service principals; a service principal never manages them (a token must not be able to
/// make more tokens).
async fn manage(
    app: &App,
    identity: &Identity,
    workspace: &str,
    action: &'static str,
) -> Result<String> {
    let (w, standing) = open(app, identity, workspace).await?;
    if !standing.is_admin() || identity.confined_to.is_some() {
        return Err(deny(app, identity, &w, action).await);
    }
    Ok(w.org_id)
}

/// Makes a service principal and its token (shown once). It is assigned to the workspace as a user.
///
/// # Errors
/// Not an admin of the workspace, the name is not valid or taken.
pub async fn create(
    app: &App,
    identity: &Identity,
    workspace: &str,
    name: &str,
) -> Result<(ServicePrincipal, String)> {
    let org = manage(app, identity, workspace, "create a service principal").await?;
    if !valid_name(name) {
        return Err(Error::Invalid(
            "the name is words, digits, spaces, dashes or underscores, up to 64".into(),
        ));
    }
    let token = new_secret(TOKEN_PREFIX);
    let sp = ServicePrincipal {
        id: new_id(),
        org_id: org.clone(),
        workspace_id: workspace.to_owned(),
        name: name.to_owned(),
        created_by: format!("user:{}", identity.subject),
        created_ms: now_ms(),
    };
    let mut uow = Uow::begin(app).await?;
    if store::service_principal_name_taken(&mut *uow.tx, workspace, name).await? {
        return Err(Error::Conflict(format!("{name} already exists here")));
    }
    store::insert_service_principal(&mut *uow.tx, &sp, &hash_secret(&token)).await?;
    store::put_member(
        &mut *uow.tx,
        workspace,
        &sp.principal(),
        Role::User,
        sp.created_ms,
    )
    .await?;
    uow.audit(
        &org,
        &Who::of(identity),
        "workspace.service-principal.create",
        &sp.principal(),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok((sp, token))
}

/// The workspace's service principals (never their tokens).
///
/// # Errors
/// Not an admin of the workspace.
pub async fn list(
    app: &App,
    identity: &Identity,
    workspace: &str,
) -> Result<Vec<ServicePrincipal>> {
    manage(app, identity, workspace, "list service principals").await?;
    store::service_principals(app.db.pool(), workspace).await
}

/// Gives it a new token (shown once); the old one stops working at once.
///
/// # Errors
/// Not an admin of the workspace, or no such service principal.
pub async fn rotate(app: &App, identity: &Identity, workspace: &str, id: &str) -> Result<String> {
    let org = manage(
        app,
        identity,
        workspace,
        "renew a service principal's token",
    )
    .await?;
    let token = new_secret(TOKEN_PREFIX);
    let mut uow = Uow::begin(app).await?;
    if store::set_service_principal_secret(&mut *uow.tx, workspace, id, &hash_secret(&token))
        .await?
        == 0
    {
        return Err(Error::NotFound("not found".into()));
    }
    uow.audit(
        &org,
        &Who::of(identity),
        "workspace.service-principal.rotate",
        &format!("sp:{id}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(token)
}

/// Removes it and its assignment; its token stops working at once. Grants that name it stay behind, harmless: its id is never
/// used again.
///
/// # Errors
/// Not an admin of the workspace, or no such service principal.
pub async fn delete(app: &App, identity: &Identity, workspace: &str, id: &str) -> Result<()> {
    let org = manage(app, identity, workspace, "delete a service principal").await?;
    let mut uow = Uow::begin(app).await?;
    if store::delete_service_principal(&mut uow.tx, workspace, id).await? == 0 {
        return Err(Error::NotFound("not found".into()));
    }
    store::remove_member(&mut uow.tx, workspace, &format!("sp:{id}")).await?;
    uow.audit(
        &org,
        &Who::of(identity),
        "workspace.service-principal.delete",
        &format!("sp:{id}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// The service principal a token belongs to, if it is one of ours and still valid.
///
/// # Errors
/// The database could not be read.
pub async fn authenticate(app: &App, token: &str) -> Result<Option<ServicePrincipal>> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Ok(None);
    }
    store::service_principal_by_secret(app.db.pool(), &hash_secret(token)).await
}
