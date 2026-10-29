//! Create, find and list catalog entries. Every call asks IAM first.

use super::{
    model::{Draft, Kind, Securable, check},
    store,
};
use crate::{
    governance::{
        audit::{model::Who, service as audit},
        iam::service as iam,
    },
    kernel::{
        auth::Caller,
        authz::{Privilege, Target, root, root_id},
        error::{Error, Result},
        new_id, now_ms,
        state::App,
    },
};

const MAX_DEPTH: usize = 4;

/// An entry and the chain of containers above it. An entry that is not there, in another tenant, or already being
/// dropped is "not found", with no permission check: callers ask IAM about what this returns.
///
/// # Errors
/// `NotFound`.
pub async fn target(app: &App, tenant: &str, id: &str) -> Result<(Securable, Target)> {
    let gone = || Error::NotFound("not found".into());
    let entry = store::get(&app.db, tenant, id)
        .await?
        .filter(|s| s.state == "active")
        .ok_or_else(gone)?;
    let mut chain = vec![entry.id.clone()];
    let mut up = entry.parent_id.clone();
    while !up.is_empty() {
        if chain.len() > MAX_DEPTH {
            return Err(Error::Internal("catalog hierarchy is too deep".into()));
        }
        // Whatever is inside something being dropped is gone from view too, even before the task reaches it.
        let parent = store::get(&app.db, tenant, &up)
            .await?
            .filter(|p| p.state == "active")
            .ok_or_else(gone)?;
        chain.push(up);
        up = parent.parent_id;
    }
    chain.push(root_id(tenant));
    let label = format!("{} {}", entry.kind.as_str(), entry.id);
    Ok((entry, Target { chain, label }))
}

/// # Errors
/// Not allowed, not valid, or the name is taken.
pub async fn create(app: &App, caller: &Caller, draft: Draft) -> Result<Securable> {
    let draft = check(draft).map_err(Error::Invalid)?;
    let parent = if draft.parent_id.is_empty() {
        root(&caller.tenant)
    } else {
        let (p, t) = target(app, &caller.tenant, &draft.parent_id).await?;
        if Some(p.kind) != draft.kind.parent() {
            return Err(Error::Invalid(format!(
                "a {} goes inside a {}",
                draft.kind.as_str(),
                draft.kind.parent().map_or("tenant", Kind::as_str)
            )));
        }
        t
    };
    iam::require(app, caller, Privilege::Create, &parent).await?;
    if store::name_taken(&app.db, &caller.tenant, &draft.parent_id, &draft.name).await? {
        return Err(Error::Conflict(format!(
            "{} already exists here",
            draft.name
        )));
    }
    let entry = Securable {
        id: new_id(),
        tenant: caller.tenant.clone(),
        kind: draft.kind,
        parent_id: draft.parent_id,
        name: draft.name,
        // The one who made it owns it, and so can manage it: the creator's own grant, not a special case.
        owner: caller.subject.clone(),
        classification: draft.classification,
        placement: draft.placement,
        consistency: draft.consistency,
        state: "active".into(),
        created_ms: now_ms(),
    };
    store::insert(&app.db, &entry).await?;
    let (_, own) = target(app, &caller.tenant, &entry.id).await?;
    iam::own(app, caller, &own).await?;
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        &format!("create.{}", entry.kind.as_str()),
        &own.label,
        "ok",
    )
    .await?;
    Ok(entry)
}

/// # Errors
/// Not found, or the caller may not see it.
pub async fn get(app: &App, caller: &Caller, id: &str) -> Result<Securable> {
    let (entry, target) = target(app, &caller.tenant, id).await?;
    iam::require(app, caller, Privilege::Use, &target).await?;
    Ok(entry)
}

/// The entries directly inside `parent` (the tenant's root when empty) that the caller may see.
///
/// # Errors
/// Not found, or the caller may not see the parent.
pub async fn list(app: &App, caller: &Caller, parent: &str) -> Result<Vec<Securable>> {
    let (parent_chain, grants) = if parent.is_empty() {
        (
            vec![root_id(&caller.tenant)],
            iam::grants_of(app, caller).await?,
        )
    } else {
        let (_, t) = target(app, &caller.tenant, parent).await?;
        iam::require(app, caller, Privilege::Use, &t).await?;
        (t.chain, iam::grants_of(app, caller).await?)
    };
    let mut out = Vec::new();
    for child in store::children(&app.db, &caller.tenant, parent).await? {
        if child.state != "active" {
            continue;
        }
        let mut chain = vec![child.id.clone()];
        chain.extend(parent_chain.iter().cloned());
        if iam::allowed(
            caller,
            &grants,
            Privilege::Use,
            &Target {
                chain,
                label: String::new(),
            },
        ) {
            out.push(child);
        }
    }
    Ok(out)
}

/// An entry and everything inside it, parents before children. Used by lifecycle, so it includes entries being dropped.
///
/// # Errors
/// The database could not be read.
pub async fn subtree(app: &App, tenant: &str, id: &str) -> Result<Vec<Securable>> {
    let mut out = Vec::new();
    let mut queue = vec![id.to_owned()];
    while let Some(next) = queue.pop() {
        if let Some(entry) = store::get(&app.db, tenant, &next).await? {
            for child in store::children(&app.db, tenant, &next).await? {
                queue.push(child.id);
            }
            out.push(entry);
        }
    }
    Ok(out)
}

/// Entries a person owns, at any depth (the erase walk).
///
/// # Errors
/// The database could not be read.
pub async fn owned_by(app: &App, tenant: &str, owner: &str) -> Result<Vec<Securable>> {
    store::owned_by(&app.db, tenant, owner).await
}

/// # Errors
/// The database could not be written.
pub async fn mark_dropping(app: &App, tenant: &str, id: &str) -> Result<()> {
    store::set_state(&app.db, tenant, id, "dropping").await
}

/// # Errors
/// The database could not be written.
pub async fn remove_row(app: &App, tenant: &str, id: &str) -> Result<()> {
    store::delete(&app.db, tenant, id).await
}

/// Whether the row is still there, in any state (the drop check).
///
/// # Errors
/// The database could not be read.
pub async fn exists(app: &App, tenant: &str, id: &str) -> Result<bool> {
    Ok(store::get(&app.db, tenant, id).await?.is_some())
}
