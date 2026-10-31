//! Grants, and the one place a request asks "may I?". Everything else calls [`require`] or [`allowed`].

use super::{model, store};
use crate::{
    governance::audit::{model::Who, service as audit, service::Uow},
    kernel::{
        auth::Identity,
        authz::{Grant, Privilege, Role, Side, Target, authorize, can_see},
        error::{Denied, Error, Result},
        now_ms,
        state::App,
    },
};

/// What the caller's identities hold in the organization.
///
/// # Errors
/// The database could not be read.
pub async fn grants_of(app: &App, org: &str, sides: &[Side]) -> Result<Vec<Grant>> {
    let mut conn = app.db.pool().acquire().await?;
    let mut out = Vec::new();
    for side in sides {
        out.extend(store::held_by(&mut conn, org, &side.principals).await?);
    }
    Ok(out)
}

/// The same decision as [`require`], from grants already loaded (for filtering a list).
#[must_use]
pub fn allowed(sides: &[Side], grants: &[Grant], need: Privilege, target: &Target) -> bool {
    authorize(sides, need, target, grants)
}

#[must_use]
pub fn visible(sides: &[Side], grants: &[Grant], target: &Target) -> bool {
    can_see(sides, target, grants)
}

/// Allows the call or ends it. A caller who may not even see the thing is told it does not exist, so a refusal never reveals
/// what is there. Every refusal is recorded.
///
/// # Errors
/// `NotFound` if the caller may not see the target, `Denied` if it may see it but not do this.
pub async fn require(
    app: &App,
    identity: &Identity,
    org: &str,
    sides: &[Side],
    need: Privilege,
    target: &Target,
) -> Result<()> {
    let grants = grants_of(app, org, sides).await?;
    if allowed(sides, &grants, need, target) {
        return Ok(());
    }
    audit::record(
        app,
        org,
        &Who::of(identity),
        need.as_str(),
        &target.label,
        "denied",
    )
    .await?;
    if visible(sides, &grants, target) {
        Err(Error::Denied(Denied {
            action: need.as_str(),
            target: target.label.clone(),
        }))
    } else {
        Err(Error::NotFound("not found".into()))
    }
}

/// Gives a principal a role on a target. Needs `MANAGE`.
///
/// # Errors
/// Not allowed, or a malformed principal.
pub async fn assign(
    app: &App,
    identity: &Identity,
    org: &str,
    sides: &[Side],
    target: &Target,
    principal: &str,
    role: Role,
) -> Result<()> {
    require(app, identity, org, sides, Privilege::Manage, target).await?;
    if !crate::governance::orgs::model::valid_principal(principal) {
        return Err(Error::Invalid("principal is not valid".into()));
    }
    let now = now_ms();
    let mut uow = Uow::begin(app).await?;
    for (securable, privilege) in model::placement(target, role) {
        store::put(
            &mut *uow.tx,
            org,
            principal,
            &securable,
            privilege.as_str(),
            &identity.subject,
            now,
        )
        .await?;
    }
    uow.audit(
        org,
        &Who::of(identity),
        &format!("grant.{}", role.as_str()),
        &format!("{} <- {principal}", target.label),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// Takes every privilege a principal holds directly on a target. Needs `MANAGE`.
///
/// # Errors
/// Not allowed, or the principal held nothing there.
pub async fn revoke(
    app: &App,
    identity: &Identity,
    org: &str,
    sides: &[Side],
    target: &Target,
    principal: &str,
) -> Result<()> {
    require(app, identity, org, sides, Privilege::Manage, target).await?;
    let mut uow = Uow::begin(app).await?;
    if store::remove(&mut *uow.tx, org, principal, target.id()).await? == 0 {
        return Err(Error::NotFound("that principal holds nothing here".into()));
    }
    uow.audit(
        org,
        &Who::of(identity),
        "revoke",
        &format!("{} <- {principal}", target.label),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// Who holds what directly on a target, as `(principal, privileges)`. Needs `MANAGE`.
///
/// # Errors
/// Not allowed.
pub async fn list(
    app: &App,
    identity: &Identity,
    org: &str,
    sides: &[Side],
    target: &Target,
) -> Result<Vec<(String, Vec<String>)>> {
    require(app, identity, org, sides, Privilege::Manage, target).await?;
    let mut by: Vec<(String, Vec<String>)> = Vec::new();
    for (principal, privilege) in store::on(app.db.pool(), org, target.id()).await? {
        match by.last_mut() {
            Some((p, list)) if *p == principal => list.push(privilege),
            _ => by.push((principal, vec![privilege])),
        }
    }
    Ok(by)
}

/// Gives the agent that made something the right to keep working with it (the person who asked owns it).
///
/// # Errors
/// The database could not be written.
pub async fn grant_all(
    uow: &mut Uow<'_>,
    org: &str,
    principal: &str,
    securable: &str,
    by: &str,
) -> Result<()> {
    store::put(
        &mut *uow.tx,
        org,
        principal,
        securable,
        Privilege::AllPrivileges.as_str(),
        by,
        now_ms(),
    )
    .await
}

/// Used by lifecycle when a securable is dropped.
///
/// # Errors
/// The database could not be written.
pub async fn forget_securable(uow: &mut Uow<'_>, org: &str, securable: &str) -> Result<()> {
    store::remove_on(&mut *uow.tx, org, securable).await
}

/// Used by lifecycle when a person is erased.
///
/// # Errors
/// The database could not be written.
pub async fn forget_principal(uow: &mut Uow<'_>, org: &str, principal: &str) -> Result<()> {
    store::remove_for(&mut *uow.tx, org, principal).await
}

/// How many grants a principal still holds (the erase check).
///
/// # Errors
/// The database could not be read.
pub async fn count_for(app: &App, org: &str, principal: &str) -> Result<i64> {
    store::count_for(app.db.pool(), org, principal).await
}
