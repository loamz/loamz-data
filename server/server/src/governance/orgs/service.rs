//! Organizations and who manages them. The account of a Keycloak organization is created by someone who holds the
//! Keycloak role `account-admin` and belongs to it; a person's own space exists for them from the first sign-in.

use super::{
    model::{Org, belongs, personal_id, valid_principal},
    store,
};
use crate::{
    governance::audit::{model::Who, service::Uow},
    kernel::{
        auth::{ACCOUNT_ADMIN_ROLE, Identity},
        authz::Side,
        error::{Error, Result},
        now_ms,
        state::App,
    },
};

/// The organization, if the caller belongs to it. Anything else is "not found": nobody learns which accounts exist.
///
/// # Errors
/// `NotFound`.
pub async fn get(app: &App, identity: &Identity, org: &str) -> Result<Org> {
    if !belongs(identity, org) {
        return Err(Error::NotFound("not found".into()));
    }
    store::get(app.db.pool(), org)
        .await?
        .ok_or_else(|| Error::NotFound("not found".into()))
}

/// Makes sure the person's own space exists, and returns it. Cheap when it does.
///
/// # Errors
/// The database could not be written.
pub async fn ensure_personal(app: &App, identity: &Identity) -> Result<Org> {
    let id = personal_id(&identity.subject);
    if let Some(o) = store::get(app.db.pool(), &id).await? {
        return Ok(o);
    }
    let o = Org {
        id: id.clone(),
        kind: "personal".into(),
        alias: "Personal".into(),
        created_ms: now_ms(),
    };
    let mut uow = Uow::begin(app).await?;
    // Another request may have made it while we waited for our turn.
    if let Some(existing) = store::get(&mut *uow.tx, &id).await? {
        return Ok(existing);
    }
    let me = format!("user:{}", identity.subject);
    store::insert(&mut *uow.tx, &o, &identity.subject).await?;
    store::add_admin(&mut *uow.tx, &id, &me, &identity.subject, o.created_ms).await?;
    uow.audit(
        &id,
        &Who::of(identity),
        "account.create",
        &format!("org {id}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(o)
}

/// Creates the Data account of a Keycloak organization the caller belongs to. Needs the Keycloak role `account-admin`; the
/// creator becomes its first admin.
///
/// # Errors
/// Not allowed, not a member, or the account exists.
pub async fn create(app: &App, identity: &Identity, org: &str) -> Result<Org> {
    let reference = identity
        .orgs
        .iter()
        .find(|o| o.id == org)
        .ok_or_else(|| Error::NotFound("not found".into()))?;
    if identity.actor.is_some() || !identity.has_role(ACCOUNT_ADMIN_ROLE) {
        return Err(Error::Denied(crate::kernel::error::Denied {
            action: "create account",
            target: format!("org {org}"),
        }));
    }
    let o = Org {
        id: org.to_owned(),
        kind: "org".into(),
        alias: reference.alias.clone(),
        created_ms: now_ms(),
    };
    let mut uow = Uow::begin(app).await?;
    if store::get(&mut *uow.tx, org).await?.is_some() {
        return Err(Error::Conflict(
            "this organization already has an account".into(),
        ));
    }
    store::insert(&mut *uow.tx, &o, &identity.subject).await?;
    store::add_admin(
        &mut *uow.tx,
        org,
        &format!("user:{}", identity.subject),
        &identity.subject,
        o.created_ms,
    )
    .await?;
    uow.audit(
        org,
        &Who::of(identity),
        "account.create",
        &format!("org {org}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(o)
}

/// Whether any of the principals is an account admin of the organization.
///
/// # Errors
/// The database could not be read.
pub async fn is_admin(app: &App, org: &str, principals: &[String]) -> Result<bool> {
    Ok(store::admins(app.db.pool(), org)
        .await?
        .iter()
        .any(|a| principals.contains(a)))
}

/// The sides of a request in this organization: the person (with their groups), and the agent acting for them if any. An
/// agent never inherits the person's admin rights: it is an admin only if it is named as one itself.
///
/// # Errors
/// The database could not be read.
pub async fn sides(app: &App, identity: &Identity, org: &str) -> Result<Vec<Side>> {
    let mut out = vec![Side::of(
        identity,
        is_admin(app, org, &identity.principals()).await?,
    )];
    if let Some(actor) = identity.actor_principals() {
        let admin = is_admin(app, org, &actor).await?;
        out.push(Side {
            principals: actor,
            account_admin: admin,
        });
    }
    Ok(out)
}

/// The admins of the account, as principals.
///
/// # Errors
/// Not an admin.
pub async fn admins(app: &App, identity: &Identity, org: &str) -> Result<Vec<String>> {
    require_admin(app, identity, org).await?;
    store::admins(app.db.pool(), org).await
}

/// # Errors
/// Not an admin.
pub async fn require_admin(app: &App, identity: &Identity, org: &str) -> Result<()> {
    get(app, identity, org).await?;
    if sides(app, identity, org)
        .await?
        .iter()
        .all(|s| s.account_admin)
    {
        return Ok(());
    }
    let who = Who::of(identity);
    crate::governance::audit::service::record(
        app,
        org,
        &who,
        "manage account",
        &format!("org {org}"),
        "denied",
    )
    .await?;
    Err(Error::Denied(crate::kernel::error::Denied {
        action: "manage the account",
        target: format!("org {org}"),
    }))
}

/// # Errors
/// Not an admin, or the principal is not valid.
pub async fn add_admin(app: &App, identity: &Identity, org: &str, principal: &str) -> Result<()> {
    require_admin(app, identity, org).await?;
    if !valid_principal(principal) {
        return Err(Error::Invalid("principal is not valid".into()));
    }
    let mut uow = Uow::begin(app).await?;
    store::add_admin(&mut *uow.tx, org, principal, &identity.subject, now_ms()).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "account.admin.add",
        principal,
        "ok",
    )
    .await?;
    uow.commit().await
}

/// An account always keeps at least one admin.
///
/// # Errors
/// Not an admin, not an admin there, or the last one.
pub async fn remove_admin(
    app: &App,
    identity: &Identity,
    org: &str,
    principal: &str,
) -> Result<()> {
    require_admin(app, identity, org).await?;
    let mut uow = Uow::begin(app).await?;
    if store::admins(&mut *uow.tx, org).await?.len() <= 1 {
        return Err(Error::Conflict(
            "an account needs at least one admin".into(),
        ));
    }
    if store::remove_admin(&mut *uow.tx, org, principal).await? == 0 {
        return Err(Error::NotFound("that principal is not an admin".into()));
    }
    uow.audit(
        org,
        &Who::of(identity),
        "account.admin.remove",
        principal,
        "ok",
    )
    .await?;
    uow.commit().await
}

/// Used when a person is erased: they stop being an admin, unless that would leave the account with none.
///
/// # Errors
/// They are the last admin, or the database could not be written.
pub async fn forget_principal(uow: &mut Uow<'_>, org: &str, principal: &str) -> Result<()> {
    let admins = store::admins(&mut *uow.tx, org).await?;
    if admins.iter().any(|a| a == principal) {
        if admins.len() <= 1 {
            return Err(Error::Conflict(
                "they are the account's only admin: add another admin first".into(),
            ));
        }
        store::remove_admin(&mut *uow.tx, org, principal).await?;
    }
    Ok(())
}

/// Whether the principal is still an admin of the account (the erase check).
///
/// # Errors
/// The database could not be read.
pub async fn still_admin(app: &App, org: &str, principal: &str) -> Result<bool> {
    Ok(store::admins(app.db.pool(), org)
        .await?
        .iter()
        .any(|a| a == principal))
}

/// One organization as the signed-in person sees it.
#[derive(Debug)]
pub struct Membership {
    pub id: String,
    pub alias: String,
    pub kind: String,
    /// Whether Data has an account for it yet.
    pub has_account: bool,
    pub admin: bool,
    /// Whether the caller may create the account (they hold the Keycloak role `account-admin`).
    pub can_create: bool,
}

/// The person's own space (made if needed) and every organization named in their token.
///
/// # Errors
/// The database could not be read or written.
pub async fn memberships(app: &App, identity: &Identity) -> Result<Vec<Membership>> {
    let personal = ensure_personal(app, identity).await?;
    let mut out = vec![Membership {
        id: personal.id.clone(),
        alias: personal.alias,
        kind: personal.kind,
        has_account: true,
        admin: is_admin(app, &personal.id, &identity.principals()).await?,
        can_create: false,
    }];
    for r in &identity.orgs {
        let org = store::get(app.db.pool(), &r.id).await?;
        let admin = if org.is_some() {
            is_admin(app, &r.id, &identity.principals()).await?
        } else {
            false
        };
        out.push(Membership {
            id: r.id.clone(),
            alias: r.alias.clone(),
            kind: "org".into(),
            has_account: org.is_some(),
            admin,
            can_create: org.is_none() && identity.has_role(ACCOUNT_ADMIN_ROLE),
        });
    }
    Ok(out)
}
