//! Workspaces and who works in them. Account admins manage workspaces; a workspace's admins manage who is assigned and what
//! they are entitled to. An account admin is an implicit admin of every workspace of their organization, so nobody can lock
//! the account out of its own workspaces.

use super::{
    model::{Member, Role, Workspace, best_role, valid_name},
    store,
};
use crate::{
    governance::{
        audit::{model::Who, service::Uow},
        orgs::{model::belongs, service as orgs},
    },
    kernel::{
        auth::Identity,
        error::{Denied, Error, Result},
        new_id, now_ms,
        state::App,
    },
};

/// What the caller is in a workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Account admin: may do everything here, and need not be assigned.
    Account,
    Member(Role),
}

impl Standing {
    #[must_use]
    pub fn is_admin(self) -> bool {
        matches!(self, Self::Account | Self::Member(Role::Admin))
    }
}

/// The workspace and the caller's standing in it. A workspace the caller is not part of is "not found".
///
/// # Errors
/// `NotFound`.
pub async fn open(app: &App, identity: &Identity, id: &str) -> Result<(Workspace, Standing)> {
    let gone = || Error::NotFound("not found".into());
    let w = store::get(app.db.pool(), id).await?.ok_or_else(gone)?;
    if !belongs(identity, &w.org_id) {
        return Err(gone());
    }
    // A service principal belongs to one workspace and can open no other.
    if identity
        .confined_to
        .as_deref()
        .is_some_and(|only| only != id)
    {
        return Err(gone());
    }
    let sides = orgs::sides(app, identity, &w.org_id).await?;
    // Every side must stand somewhere in the workspace: an agent is let in only if it was assigned too.
    let members = store::members(app.db.pool(), id).await?;
    let mut standing = Standing::Account;
    let mut worst: Option<Standing> = None;
    for side in &sides {
        let s = if side.account_admin {
            Some(Standing::Account)
        } else {
            best_role(&members, &side.principals).map(Standing::Member)
        };
        match s {
            None => return Err(gone()),
            Some(s) => worst = Some(worst.map_or(s, |w| if rank(s) < rank(w) { s } else { w })),
        }
    }
    if let Some(w) = worst {
        standing = w;
    }
    Ok((w, standing))
}

fn rank(s: Standing) -> u8 {
    match s {
        Standing::Member(Role::User) => 0,
        Standing::Member(Role::Admin) => 1,
        Standing::Account => 2,
    }
}

pub async fn deny(app: &App, identity: &Identity, w: &Workspace, action: &'static str) -> Error {
    let _ = crate::governance::audit::service::record(
        app,
        &w.org_id,
        &Who::of(identity),
        action,
        &format!("workspace {}", w.id),
        "denied",
    )
    .await;
    Error::Denied(Denied {
        action,
        target: format!("workspace {}", w.name),
    })
}

/// The workspaces of an organization the caller may open.
///
/// # Errors
/// Not part of the organization.
pub async fn mine(app: &App, identity: &Identity, org: &str) -> Result<Vec<(Workspace, Standing)>> {
    orgs::get(app, identity, org).await?;
    let mut out = Vec::new();
    for w in store::in_org(app.db.pool(), org).await? {
        if let Ok((w, s)) = open(app, identity, &w.id).await {
            out.push((w, s));
        }
    }
    Ok(out)
}

/// Creates a workspace. Account admins only. The creator is its first admin.
///
/// # Errors
/// Not an account admin, a bad or taken name.
pub async fn create(app: &App, identity: &Identity, org: &str, name: &str) -> Result<Workspace> {
    orgs::require_admin(app, identity, org).await?;
    if !valid_name(name) {
        return Err(Error::Invalid(
            "a workspace name is words, digits, spaces and dashes, up to 64 characters".into(),
        ));
    }
    let w = Workspace {
        id: new_id(),
        org_id: org.to_owned(),
        name: name.to_owned(),
        created_ms: now_ms(),
    };
    let mut uow = Uow::begin(app).await?;
    if store::name_taken(&mut *uow.tx, org, name).await? {
        return Err(Error::Conflict(format!(
            "there is already a workspace called {name}"
        )));
    }
    store::insert(&mut *uow.tx, &w, &identity.subject).await?;
    store::put_member(
        &mut *uow.tx,
        &w.id,
        &format!("user:{}", identity.subject),
        Role::Admin,
        w.created_ms,
    )
    .await?;
    uow.audit(
        org,
        &Who::of(identity),
        "workspace.create",
        &format!("workspace {}", w.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(w)
}

/// # Errors
/// Not an account admin, a bad or taken name.
pub async fn rename(app: &App, identity: &Identity, id: &str, name: &str) -> Result<Workspace> {
    let (w, standing) = open(app, identity, id).await?;
    if standing != Standing::Account {
        return Err(deny(app, identity, &w, "rename a workspace").await);
    }
    if !valid_name(name) {
        return Err(Error::Invalid(
            "a workspace name is words, digits, spaces and dashes, up to 64 characters".into(),
        ));
    }
    let mut uow = Uow::begin(app).await?;
    if store::name_taken(&mut *uow.tx, &w.org_id, name).await? {
        return Err(Error::Conflict(format!(
            "there is already a workspace called {name}"
        )));
    }
    store::rename(&mut *uow.tx, id, name).await?;
    uow.audit(
        &w.org_id,
        &Who::of(identity),
        "workspace.rename",
        &format!("workspace {id}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(Workspace {
        name: name.to_owned(),
        ..w
    })
}

/// Removes a workspace and its assignments. The organization's catalogs are not touched: they belong to the account.
///
/// `catalogs` is how many catalogs and compute targets the workspace owns: it cannot be deleted while it owns any.
///
/// # Errors
/// Not an account admin, or the workspace still owns catalogs.
pub async fn delete(app: &App, identity: &Identity, id: &str, catalogs: i64) -> Result<()> {
    let (w, standing) = open(app, identity, id).await?;
    if standing != Standing::Account {
        return Err(deny(app, identity, &w, "delete a workspace").await);
    }
    if catalogs > 0 {
        return Err(Error::Conflict(
            "this workspace still owns catalogs or compute: remove them first".into(),
        ));
    }
    let mut uow = Uow::begin(app).await?;
    store::delete(&mut uow.tx, id).await?;
    uow.audit(
        &w.org_id,
        &Who::of(identity),
        "workspace.delete",
        &format!("workspace {id}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// # Errors
/// Not an admin of the workspace.
pub async fn members(app: &App, identity: &Identity, id: &str) -> Result<Vec<Member>> {
    let (w, standing) = open(app, identity, id).await?;
    if !standing.is_admin() {
        return Err(deny(app, identity, &w, "list workspace members").await);
    }
    store::members(app.db.pool(), id).await
}

/// Assigns a person, group or service principal to the workspace, or changes their role.
///
/// # Errors
/// Not an admin of the workspace, or the principal is not valid.
pub async fn assign(
    app: &App,
    identity: &Identity,
    id: &str,
    principal: &str,
    role: Role,
) -> Result<()> {
    let (w, standing) = open(app, identity, id).await?;
    if !standing.is_admin() {
        return Err(deny(app, identity, &w, "assign to a workspace").await);
    }
    if !crate::governance::orgs::model::valid_principal(principal) {
        return Err(Error::Invalid("principal is not valid".into()));
    }
    let mut uow = Uow::begin(app).await?;
    if orgs::is_erasing_in(&mut uow.tx, &w.org_id, principal).await? {
        return Err(Error::NotFound("not found".into()));
    }
    store::put_member(&mut *uow.tx, id, principal, role, now_ms()).await?;
    uow.audit(
        &w.org_id,
        &Who::of(identity),
        &format!("workspace.assign.{}", role.as_str()),
        &format!("workspace {id} <- {principal}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// Takes a principal out of the workspace. A workspace always keeps one admin.
///
/// # Errors
/// Not an admin, not assigned, or the last admin.
pub async fn unassign(app: &App, identity: &Identity, id: &str, principal: &str) -> Result<()> {
    let (w, standing) = open(app, identity, id).await?;
    if !standing.is_admin() {
        return Err(deny(app, identity, &w, "unassign from a workspace").await);
    }
    let mut uow = Uow::begin(app).await?;
    let all = store::members(&mut *uow.tx, id).await?;
    let admins = all.iter().filter(|m| m.role == Role::Admin).count();
    if all
        .iter()
        .any(|m| m.principal == principal && m.role == Role::Admin)
        && admins <= 1
    {
        return Err(Error::Conflict(
            "a workspace needs at least one admin".into(),
        ));
    }
    if store::remove_member(&mut uow.tx, id, principal).await? == 0 {
        return Err(Error::NotFound(
            "that principal is not assigned here".into(),
        ));
    }
    uow.audit(
        &w.org_id,
        &Who::of(identity),
        "workspace.unassign",
        &format!("workspace {id} <- {principal}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// What each assigned principal is entitled to, as `(principal, entitlement)`.
///
/// # Errors
/// Not an admin of the workspace.
pub async fn entitlements(
    app: &App,
    identity: &Identity,
    id: &str,
) -> Result<Vec<(String, String)>> {
    let (w, standing) = open(app, identity, id).await?;
    if !standing.is_admin() {
        return Err(deny(app, identity, &w, "list entitlements").await);
    }
    store::entitlements(app.db.pool(), id).await
}

/// Gives or takes an entitlement. The principal must be assigned to the workspace.
///
/// # Errors
/// Not an admin, an unknown entitlement, or the principal is not assigned.
pub async fn set_entitlement(
    app: &App,
    identity: &Identity,
    id: &str,
    principal: &str,
    entitlement: &str,
    on: bool,
) -> Result<()> {
    let (w, standing) = open(app, identity, id).await?;
    if !standing.is_admin() {
        return Err(deny(app, identity, &w, "change entitlements").await);
    }
    if !super::model::ENTITLEMENTS.contains(&entitlement) {
        return Err(Error::Invalid(format!(
            "entitlement must be one of {}",
            super::model::ENTITLEMENTS.join(", ")
        )));
    }
    let mut uow = Uow::begin(app).await?;
    if orgs::is_erasing_in(&mut uow.tx, &w.org_id, principal).await? {
        return Err(Error::NotFound("not found".into()));
    }
    if !store::members(&mut *uow.tx, id)
        .await?
        .iter()
        .any(|m| m.principal == principal)
    {
        return Err(Error::Invalid(
            "assign the principal to the workspace first".into(),
        ));
    }
    if on {
        store::put_entitlement(&mut *uow.tx, id, principal, entitlement).await?;
    } else {
        store::remove_entitlement(&mut *uow.tx, id, principal, entitlement).await?;
    }
    uow.audit(
        &w.org_id,
        &Who::of(identity),
        if on {
            "workspace.entitle"
        } else {
            "workspace.unentitle"
        },
        &format!("workspace {id} <- {principal} {entitlement}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// Whether the caller holds the entitlement in the workspace (through themselves or a group). Account admins and workspace
/// admins hold them all.
///
/// # Errors
/// The database could not be read.
pub async fn entitled(
    app: &App,
    identity: &Identity,
    id: &str,
    standing: Standing,
    entitlement: &str,
) -> Result<bool> {
    // An agent acting for a person is entitled only where both are: the person's entitlement is not the agent's to borrow, and
    // an admin's agent is not an admin. Each side stands on its own.
    let Some(w) = store::get(app.db.pool(), id).await? else {
        return Ok(false);
    };
    let sides = orgs::sides(app, identity, &w.org_id).await?;
    if sides.len() == 1 {
        if standing.is_admin() {
            return Ok(true);
        }
        let held = store::entitlements(app.db.pool(), id).await?;
        return Ok(held
            .iter()
            .any(|(p, e)| e == entitlement && identity.principals().contains(p)));
    }
    let members = store::members(app.db.pool(), id).await?;
    let held = store::entitlements(app.db.pool(), id).await?;
    Ok(sides.iter().all(|side| {
        side.account_admin
            || best_role(&members, &side.principals) == Some(Role::Admin)
            || held
                .iter()
                .any(|(p, e)| e == entitlement && side.principals.contains(p))
    }))
}

/// Used when a person is erased.
///
/// # Errors
/// The database could not be written.
pub async fn forget_principal(uow: &mut Uow<'_>, org: &str, principal: &str) -> Result<()> {
    store::forget_principal(&mut uow.tx, org, principal).await
}

/// How many workspace assignments a principal still has (the erase check).
///
/// # Errors
/// The database could not be read.
pub async fn count_assignments(app: &App, org: &str, principal: &str) -> Result<i64> {
    store::count_assignments(app.db.pool(), org, principal).await
}
