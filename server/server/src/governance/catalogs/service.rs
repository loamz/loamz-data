//! Create, find and list catalog entries. Every call asks IAM first, and every change is recorded with it in one unit of work.

use std::collections::HashSet;

use super::{
    model::{Draft, Kind, Securable, check},
    store,
};
use crate::{
    governance::{
        audit::{model::Who, service::Uow},
        iam::service as iam,
        workspaces::service as workspaces,
    },
    kernel::{
        auth::Identity,
        authz::{Grant, Level, Link, Privilege, Side, Target, metastore, metastore_id},
        error::{Error, Result},
        new_id, now_ms,
        state::App,
    },
};

const MAX_DEPTH: usize = 4;

fn level(kind: Kind) -> Level {
    match kind {
        Kind::Catalog => Level::Catalog,
        Kind::Schema => Level::Schema,
        _ => Level::Leaf,
    }
}

/// An entry and the chain of containers above it, with who owns each. An entry that is not there, in another organization, or
/// already being dropped (or inside something being dropped) is "not found", with no permission check: callers ask IAM about
/// what this returns.
///
/// # Errors
/// `NotFound`.
pub async fn target(app: &App, org: &str, id: &str) -> Result<(Securable, Target)> {
    let gone = || Error::NotFound("not found".into());
    let entry = store::get(app.db.pool(), org, id)
        .await?
        .filter(|s| s.state == "active")
        .ok_or_else(gone)?;
    let mut chain = vec![Link {
        id: entry.id.clone(),
        level: level(entry.kind),
        owner: entry.owner.clone(),
    }];
    let mut up = entry.parent_id.clone();
    while !up.is_empty() {
        if chain.len() > MAX_DEPTH {
            return Err(Error::Internal("catalog hierarchy is too deep".into()));
        }
        // Whatever is inside something being dropped is gone from view too, even before the task reaches it.
        let parent = store::get(app.db.pool(), org, &up)
            .await?
            .filter(|p| p.state == "active")
            .ok_or_else(gone)?;
        chain.push(Link {
            id: parent.id.clone(),
            level: level(parent.kind),
            owner: parent.owner.clone(),
        });
        up = parent.parent_id;
    }
    chain.push(Link {
        id: metastore_id(org),
        level: Level::Metastore,
        owner: String::new(),
    });
    let label = format!("{} {}", entry.kind.as_str(), entry.id);
    Ok((entry, Target { chain, label }))
}

/// The catalog an entry sits in (or is).
async fn catalog_of(app: &App, org: &str, entry: &Securable) -> Result<String> {
    let mut at = entry.clone();
    while !at.parent_id.is_empty() {
        at = store::get(app.db.pool(), org, &at.parent_id)
            .await?
            .ok_or_else(|| Error::Internal("a parent is missing".into()))?;
    }
    Ok(at.id)
}

/// A catalog bound to other workspaces is not there from this one.
async fn in_workspace(app: &App, workspace: &str, org: &str, entry: &Securable) -> Result<()> {
    if !workspaces::catalog_visible(app, workspace, &catalog_of(app, org, entry).await?).await? {
        return Err(Error::NotFound("not found".into()));
    }
    Ok(())
}

/// What `target` returns, also checking the entry is visible from this workspace.
///
/// # Errors
/// `NotFound`.
pub async fn target_in(
    app: &App,
    workspace: &str,
    org: &str,
    id: &str,
) -> Result<(Securable, Target)> {
    let (entry, t) = target(app, org, id).await?;
    in_workspace(app, workspace, org, &entry).await?;
    Ok((entry, t))
}

fn owner_of(identity: &Identity) -> String {
    match identity.kind {
        crate::kernel::auth::Kind::Person => format!("user:{}", identity.subject),
        crate::kernel::auth::Kind::Service => format!("sp:{}", identity.subject),
    }
}

/// # Errors
/// Not allowed, not valid, or the name is taken.
pub async fn create(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    draft: Draft,
) -> Result<Securable> {
    let draft = check(draft).map_err(Error::Invalid)?;
    let (parent, parent_entry) = if draft.parent_id.is_empty() {
        (metastore(org), None)
    } else {
        let (p, t) = target_in(app, workspace, org, &draft.parent_id).await?;
        if Some(p.kind) != draft.kind.parent() {
            return Err(Error::Invalid(format!(
                "a {} goes inside a {}",
                draft.kind.as_str(),
                draft.kind.parent().map_or("metastore", Kind::as_str)
            )));
        }
        (t, Some(p))
    };
    let _ = parent_entry;
    let need = match draft.kind {
        Kind::Catalog => Privilege::CreateCatalog,
        Kind::Schema => Privilege::CreateSchema,
        Kind::Table => Privilege::CreateTable,
        Kind::Volume => Privilege::CreateVolume,
        Kind::View => Privilege::CreateView,
    };
    iam::require(app, identity, org, sides, need, &parent).await?;
    let entry = Securable {
        id: new_id(),
        org_id: org.to_owned(),
        kind: draft.kind,
        parent_id: draft.parent_id,
        name: draft.name,
        // The one who asked owns it; an agent acting for them is given everything on it so it can keep working.
        owner: owner_of(identity),
        classification: draft.classification,
        placement: draft.placement,
        consistency: draft.consistency,
        state: "active".into(),
        created_ms: now_ms(),
    };
    let mut uow = Uow::begin(app).await?;
    if store::name_taken(&mut *uow.tx, org, &entry.parent_id, &entry.name).await? {
        return Err(Error::Conflict(format!(
            "{} already exists here",
            entry.name
        )));
    }
    store::insert(&mut *uow.tx, &entry).await?;
    if let Some(agent) = &identity.actor {
        iam::grant_all(
            &mut uow,
            org,
            &format!("sp:{agent}"),
            &entry.id,
            &identity.subject,
        )
        .await?;
    }
    uow.audit(
        org,
        &Who::of(identity),
        &format!("create.{}", entry.kind.as_str()),
        &format!("{} {}", entry.kind.as_str(), entry.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(entry)
}

/// Whether anything the caller holds sits at or below this entry (so a container they can drill into is shown).
async fn contains_any(
    app: &App,
    org: &str,
    entry_id: &str,
    interesting: &HashSet<&str>,
    depth: usize,
) -> Result<bool> {
    if interesting.contains(entry_id) {
        return Ok(true);
    }
    if depth == 0 {
        return Ok(false);
    }
    for child in store::children(app.db.pool(), org, entry_id).await? {
        if child.state == "active"
            && Box::pin(contains_any(app, org, &child.id, interesting, depth - 1)).await?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn may_view(
    app: &App,
    org: &str,
    sides: &[Side],
    grants: &[Grant],
    entry: &Securable,
    target: &Target,
) -> Result<bool> {
    if iam::visible(sides, grants, target) {
        return Ok(true);
    }
    // One may walk down through a container to what one was given inside it, but not read the container.
    if matches!(entry.kind, Kind::Catalog | Kind::Schema) {
        let interesting: HashSet<&str> = grants.iter().map(|g| g.securable.as_str()).collect();
        return contains_any(app, org, &entry.id, &interesting, MAX_DEPTH).await;
    }
    Ok(false)
}

/// # Errors
/// Not found, or the caller may not see it.
pub async fn get(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    id: &str,
) -> Result<Securable> {
    let (entry, target) = target_in(app, workspace, org, id).await?;
    let grants = iam::grants_of(app, org, sides).await?;
    if !may_view(app, org, sides, &grants, &entry, &target).await? {
        crate::governance::audit::service::record(
            app,
            org,
            &Who::of(identity),
            "view",
            &target.label,
            "denied",
        )
        .await?;
        return Err(Error::NotFound("not found".into()));
    }
    Ok(entry)
}

/// The entries directly inside `parent` (the metastore when empty) that the caller may see from this workspace.
///
/// # Errors
/// Not found, or the caller may not see the parent.
pub async fn list(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    parent: &str,
) -> Result<Vec<Securable>> {
    let grants = iam::grants_of(app, org, sides).await?;
    let parent_chain = if parent.is_empty() {
        vec![Link {
            id: metastore_id(org),
            level: Level::Metastore,
            owner: String::new(),
        }]
    } else {
        get(app, identity, workspace, org, sides, parent).await?;
        target(app, org, parent).await?.1.chain
    };
    let mut out = Vec::new();
    for child in store::children(app.db.pool(), org, parent).await? {
        if child.state != "active" {
            continue;
        }
        if parent.is_empty() && !workspaces::catalog_visible(app, workspace, &child.id).await? {
            continue;
        }
        let mut chain = vec![Link {
            id: child.id.clone(),
            level: level(child.kind),
            owner: child.owner.clone(),
        }];
        chain.extend(parent_chain.iter().cloned());
        if may_view(
            app,
            org,
            sides,
            &grants,
            &child,
            &Target {
                chain,
                label: String::new(),
            },
        )
        .await?
        {
            out.push(child);
        }
    }
    Ok(out)
}

/// Hands an entry to a new owner. Needs `MANAGE`.
///
/// # Errors
/// Not allowed, or the new owner is not a valid principal.
pub async fn transfer(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    id: &str,
    owner: &str,
) -> Result<()> {
    let (_, target) = target_in(app, workspace, org, id).await?;
    iam::require(app, identity, org, sides, Privilege::Manage, &target).await?;
    if !crate::governance::orgs::model::valid_principal(owner) {
        return Err(Error::Invalid("owner is not a valid principal".into()));
    }
    let mut uow = Uow::begin(app).await?;
    store::set_owner(&mut *uow.tx, org, id, owner).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "transfer",
        &format!("{} -> {owner}", target.label),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// An entry and everything inside it, parents before children. Used by lifecycle, so it includes entries being dropped.
///
/// # Errors
/// The database could not be read.
pub async fn subtree(app: &App, org: &str, id: &str) -> Result<Vec<Securable>> {
    let mut out = Vec::new();
    let mut queue = vec![id.to_owned()];
    while let Some(next) = queue.pop() {
        if let Some(entry) = store::get(app.db.pool(), org, &next).await? {
            for child in store::children(app.db.pool(), org, &next).await? {
                queue.push(child.id);
            }
            out.push(entry);
        }
    }
    Ok(out)
}

/// Entries a principal owns, at any depth (the erase walk).
///
/// # Errors
/// The database could not be read.
pub async fn owned_by(app: &App, org: &str, owner: &str) -> Result<Vec<Securable>> {
    store::owned_by(app.db.pool(), org, owner).await
}

/// # Errors
/// The database could not be written.
pub async fn mark_dropping(uow: &mut Uow<'_>, org: &str, id: &str) -> Result<()> {
    store::set_state(&mut *uow.tx, org, id, "dropping").await
}

/// # Errors
/// The database could not be written.
pub async fn remove_row(uow: &mut Uow<'_>, org: &str, id: &str) -> Result<()> {
    store::delete(&mut *uow.tx, org, id).await
}

/// Whether the row is still there, in any state (the drop check).
///
/// # Errors
/// The database could not be read.
pub async fn exists(app: &App, org: &str, id: &str) -> Result<bool> {
    Ok(store::get(app.db.pool(), org, id).await?.is_some())
}
