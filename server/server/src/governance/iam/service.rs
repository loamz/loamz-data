//! Grants, and the one place a request asks "may I?". Everything else calls [`require`] or [`allowed`].

use super::{model, store};
use crate::{
    governance::audit::{model::Who, service as audit},
    kernel::{
        auth::Caller,
        authz::{Grant, Privilege, Role, Target, authorize, root_id},
        error::{Denied, Error, Result},
        now_ms,
        state::App,
    },
};

/// What the caller's identities hold in the caller's tenant. A tenant's owners hold everything at its root, and that
/// holds for the person only: an agent acting for them has to be given its own grants.
///
/// # Errors
/// The database could not be read.
pub async fn grants_of(app: &App, caller: &Caller) -> Result<Vec<Grant>> {
    let mut who = vec![caller.subject.as_str()];
    if let Some(actor) = &caller.actor {
        who.push(actor);
    }
    let mut grants = store::held_by(&app.db, &caller.tenant, &who).await?;
    if app.owns_root(caller) {
        grants.extend(Role::Owner.privileges().iter().map(|&privilege| Grant {
            principal: caller.subject.clone(),
            securable: root_id(&caller.tenant),
            privilege,
        }));
    }
    Ok(grants)
}

/// The same decision as [`require`], from grants already loaded (for filtering a list).
#[must_use]
pub fn allowed(caller: &Caller, grants: &[Grant], need: Privilege, target: &Target) -> bool {
    authorize(
        &caller.subject,
        caller.actor.as_deref(),
        need,
        &target.chain,
        grants,
    )
}

/// Allows the call or ends it. A caller who may not even see the thing is told it does not exist, so a refusal never
/// reveals what is there. Every refusal is recorded.
///
/// # Errors
/// `NotFound` if the caller may not use the target, `Denied` if it may use it but not do this.
pub async fn require(app: &App, caller: &Caller, need: Privilege, target: &Target) -> Result<()> {
    let grants = grants_of(app, caller).await?;
    if allowed(caller, &grants, need, target) {
        return Ok(());
    }
    // Every member knows their own tenant exists, so its admin doors answer "no" rather than "not found".
    let sees = target.chain.len() == 1 || allowed(caller, &grants, Privilege::Use, target);
    let action = need.as_str();
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        action,
        &target.label,
        "denied",
    )
    .await?;
    if sees {
        Err(Error::Denied(Denied {
            action,
            target: target.label.clone(),
        }))
    } else {
        Err(Error::NotFound("not found".into()))
    }
}

/// Gives a principal a role on a target. Needs `manage`.
///
/// # Errors
/// Not allowed, or a malformed principal.
pub async fn assign(
    app: &App,
    caller: &Caller,
    target: &Target,
    principal: &str,
    role: Role,
) -> Result<()> {
    require(app, caller, Privilege::Manage, target).await?;
    if !model::valid_principal(principal) {
        return Err(Error::Invalid("principal is not valid".into()));
    }
    let now = now_ms();
    for privilege in model::privileges_of(role) {
        store::put(
            &app.db,
            &caller.tenant,
            principal,
            target.id(),
            privilege,
            &caller.subject,
            now,
        )
        .await?;
    }
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        &format!("grant.{}", role.as_str()),
        &format!("{} <- {principal}", target.label),
        "ok",
    )
    .await
}

/// Makes the caller (and the agent acting for them) owner of what they just created. Needs no check: creating it was
/// already allowed, and the owner of a new thing is whoever made it.
///
/// # Errors
/// The database could not be written.
pub async fn own(app: &App, caller: &Caller, target: &Target) -> Result<()> {
    let now = now_ms();
    let mut who = vec![caller.subject.as_str()];
    if let Some(actor) = &caller.actor {
        who.push(actor);
    }
    for principal in who {
        for privilege in model::privileges_of(Role::Owner) {
            store::put(
                &app.db,
                &caller.tenant,
                principal,
                target.id(),
                privilege,
                &caller.subject,
                now,
            )
            .await?;
        }
    }
    Ok(())
}

/// Takes every privilege a principal holds directly on a target. Needs `manage`.
///
/// # Errors
/// Not allowed, or the principal held nothing there.
pub async fn revoke(app: &App, caller: &Caller, target: &Target, principal: &str) -> Result<()> {
    require(app, caller, Privilege::Manage, target).await?;
    if store::remove(&app.db, &caller.tenant, principal, target.id()).await? == 0 {
        return Err(Error::NotFound("that principal holds nothing here".into()));
    }
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        "revoke",
        &format!("{} <- {principal}", target.label),
        "ok",
    )
    .await
}

/// Who holds what directly on a target, as `(principal, role or privileges)`. Needs `manage`.
///
/// # Errors
/// Not allowed.
pub async fn list(
    app: &App,
    caller: &Caller,
    target: &Target,
) -> Result<Vec<(String, Vec<String>)>> {
    require(app, caller, Privilege::Manage, target).await?;
    let mut by: Vec<(String, Vec<String>)> = Vec::new();
    for (principal, privilege) in store::on(&app.db, &caller.tenant, target.id()).await? {
        match by.last_mut() {
            Some((p, list)) if *p == principal => list.push(privilege),
            _ => by.push((principal, vec![privilege])),
        }
    }
    Ok(by)
}

/// Used by lifecycle when a securable is dropped.
///
/// # Errors
/// The database could not be written.
pub async fn forget_securable(app: &App, tenant: &str, securable: &str) -> Result<()> {
    store::remove_on(&app.db, tenant, securable).await
}

/// Used by lifecycle when a person is erased.
///
/// # Errors
/// The database could not be written.
pub async fn forget_principal(app: &App, tenant: &str, principal: &str) -> Result<()> {
    store::remove_for(&app.db, tenant, principal).await
}

/// How many grants a principal still holds (the erase check).
///
/// # Errors
/// The database could not be read.
pub async fn count_for(app: &App, tenant: &str, principal: &str) -> Result<i64> {
    store::count_for(&app.db, tenant, principal).await
}
