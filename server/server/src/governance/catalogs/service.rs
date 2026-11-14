//! Create, find and list catalog entries. Every call asks IAM first, and every change is recorded with it in one unit of work.

use std::collections::HashSet;

use super::{
    model::{Draft, Kind, Metadata, Securable, check, check_metadata, join_folder},
    policy::{self, Policy, Resolved},
    store,
};
use crate::{
    governance::{
        audit::{model::Who, service::Uow},
        iam::service as iam,
    },
    kernel::{
        auth::Identity,
        authz::{Grant, Level, Link, Privilege, Side, Target, metastore, metastore_id},
        error::{Error, Result},
        new_id, now_ms,
        state::App,
    },
    storage::locations::service as locations,
};

const MAX_DEPTH: usize = 4;
const SEARCH_PAGE_LIMIT: i64 = 100;
const SEARCH_PAGE_SIZE: usize = 100;
const SEARCH_PAGES: usize = 20;

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

/// A catalog belongs to one workspace: from any other it is not there.
fn in_workspace(workspace: &str, entry: &Securable) -> Result<()> {
    if entry.workspace_id != workspace {
        return Err(Error::NotFound("not found".into()));
    }
    Ok(())
}

/// How many catalogs a workspace owns. A workspace that owns any cannot be deleted.
///
/// # Errors
/// The database could not be read.
pub async fn count_in_workspace(app: &App, org: &str, workspace: &str) -> Result<i64> {
    store::count_catalogs(app.db.pool(), org, workspace).await
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
    in_workspace(workspace, &entry)?;
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
    let need = match draft.kind {
        Kind::Catalog => Privilege::CreateCatalog,
        Kind::Schema => Privilege::CreateSchema,
        Kind::Table => Privilege::CreateTable,
        Kind::Volume => Privilege::CreateVolume,
        Kind::View => Privilege::CreateView,
    };
    iam::require(app, identity, org, sides, need, &parent).await?;
    // A table exists only with its Iceberg metadata (made by the tables service); nothing else has any.
    if (draft.kind == Kind::Table) != draft.table.is_some() {
        return Err(Error::Invalid(
            "a table is created with its columns, through the tables service".into(),
        ));
    }
    if draft.location.is_some() && draft.kind != Kind::Catalog {
        return Err(Error::Invalid(
            "only a catalog is put on a storage location; what is inside it follows".into(),
        ));
    }
    let (id, metadata) = draft.table.unwrap_or_else(|| (new_id(), String::new()));
    // Where its bytes go: a catalog has its own root in its workspace's prefix, a schema shares its catalog's, and a table or volume
    // has a folder of its own under it.
    let root = parent_entry
        .as_ref()
        .map_or_else(String::new, |p| p.folder.clone());
    let catalog_root = match &draft.location {
        Some(name) => locations::managed_root(app, identity, workspace, name).await?,
        None => workspace.to_owned(),
    };
    let folder = match draft.kind {
        Kind::Catalog => join_folder(&catalog_root, &id),
        Kind::Schema => root,
        Kind::Table | Kind::Volume => join_folder(&root, &id),
        Kind::View => String::new(),
    };
    let entry = Securable {
        id,
        org_id: org.to_owned(),
        workspace_id: workspace.to_owned(),
        folder,
        kind: draft.kind,
        parent_id: draft.parent_id,
        name: draft.name,
        description: None,
        tags: Vec::new(),
        // The one who asked owns it; an agent acting for them is given everything on it so it can keep working.
        owner: owner_of(identity),
        classification: draft.classification,
        placement: draft.placement,
        consistency: draft.consistency,
        state: "active".into(),
        created_ms: now_ms(),
        metadata,
    };
    let mut uow = Uow::begin(app).await?;
    if store::name_taken(&mut *uow.tx, org, workspace, &entry.parent_id, &entry.name).await? {
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

/// Moves a table to its next metadata file in the same unit of work as its audit entry; false if another commit got there first.
///
/// # Errors
/// The database could not be written.
pub async fn advance(uow: &mut Uow<'_>, org: &str, id: &str, from: &str, to: &str) -> Result<bool> {
    store::advance(&mut *uow.tx, org, id, from, to).await
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
    let mut conn = app.db.pool().acquire().await?;
    store::has_descendant(
        &mut conn,
        org,
        entry_id,
        &interesting.iter().copied().collect::<Vec<_>>(),
        depth,
    )
    .await
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
        if in_workspace(workspace, &child).is_err() {
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

fn escape_like(value: &str) -> String {
    format!(
        "%{}%",
        value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

async fn display_path(app: &App, org: &str, entry: &Securable) -> Result<String> {
    let current = store::get(app.db.pool(), org, &entry.id)
        .await?
        .filter(|s| s.state == "active" && s.workspace_id == entry.workspace_id)
        .ok_or_else(|| Error::NotFound("not found".into()))?;
    let mut names = vec![current.name];
    let mut parent = current.parent_id;
    let mut depth = 0;
    while !parent.is_empty() {
        if depth >= MAX_DEPTH {
            return Err(Error::Internal("catalog hierarchy is too deep".into()));
        }
        let ancestor = store::get(app.db.pool(), org, &parent)
            .await?
            .filter(|s| s.state == "active")
            .ok_or_else(|| Error::NotFound("not found".into()))?;
        names.push(ancestor.name.clone());
        parent = ancestor.parent_id;
        depth += 1;
    }
    names.reverse();
    Ok(names.join("."))
}

/// Search filters supplied by one workspace search request.
pub struct SearchRequest {
    pub q: Option<String>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub owner: Option<String>,
    pub limit: Option<i64>,
}

/// Searches a bounded set of workspace candidates and only returns entries accepted by the catalog visibility rule.
///
/// # Errors
/// The filter is invalid, or the database could not be read.
pub async fn search(
    app: &App,
    workspace: &str,
    org: &str,
    sides: &[Side],
    request: SearchRequest,
) -> Result<Vec<(Securable, String)>> {
    let limit = request.limit.unwrap_or(25);
    if !(1..=100).contains(&limit) {
        return Err(Error::Invalid("limit must be between 1 and 100".into()));
    }
    let result_limit = usize::try_from(limit)
        .map_err(|_| Error::Invalid("limit must be between 1 and 100".into()))?;
    let kind = request.kind.as_deref().unwrap_or("");
    if !kind.is_empty() && Kind::parse(kind).is_none() {
        return Err(Error::Invalid(
            "kind must be catalog, schema, table, volume or view".into(),
        ));
    }
    let owner = request.owner.as_deref().unwrap_or("");
    if !owner.is_empty() && !crate::governance::orgs::model::valid_principal(owner) {
        return Err(Error::Invalid("owner must be a valid principal".into()));
    }
    let tag = request.tag.as_deref().unwrap_or("");
    if !tag.is_empty() && check_metadata(&None, &[tag.to_owned()]).is_err() {
        return Err(Error::Invalid(
            "tag must be a plain word of up to 32 characters".into(),
        ));
    }
    let q = request.q.as_deref().unwrap_or("");
    if q.chars().count() > 2000 {
        return Err(Error::Invalid("q must be 2000 characters or fewer".into()));
    }
    if q.is_empty() && kind.is_empty() && tag.is_empty() && owner.is_empty() {
        return Err(Error::Invalid("provide q, kind, tag or owner".into()));
    }
    let tag_pattern = if tag.is_empty() {
        String::new()
    } else {
        format!(
            "%\"{}\"%",
            tag.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    };
    let text_pattern = if q.is_empty() {
        String::new()
    } else {
        escape_like(&q.to_lowercase())
    };
    let grants = iam::grants_of(app, org, sides).await?;
    let mut visible = Vec::new();
    let mut after_name = String::new();
    let mut after_id = String::new();
    for _ in 0..SEARCH_PAGES {
        let candidates = store::search_candidates(
            app.db.pool(),
            store::CandidateSearch {
                org,
                workspace,
                kind,
                owner,
                tag_pattern: &tag_pattern,
                text_pattern: &text_pattern,
                after_name: &after_name,
                after_id: &after_id,
                limit: SEARCH_PAGE_LIMIT,
            },
        )
        .await?;
        if candidates.is_empty() {
            break;
        }
        let full_page = candidates.len() == SEARCH_PAGE_SIZE;
        if let Some(last) = candidates.last() {
            after_name.clone_from(&last.name);
            after_id.clone_from(&last.id);
        }
        for candidate in candidates {
            let (entry, target) = match target_in(app, workspace, org, &candidate.id).await {
                Ok(found) => found,
                Err(Error::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            if may_view(app, org, sides, &grants, &entry, &target).await? {
                let path = match display_path(app, org, &entry).await {
                    Ok(path) => path,
                    Err(Error::NotFound(_)) => continue,
                    Err(error) => return Err(error),
                };
                visible.push((entry, path));
                if visible.len() == result_limit {
                    return Ok(visible);
                }
            }
        }
        if !full_page {
            break;
        }
    }
    Ok(visible)
}

/// Replaces description and tags, with the audit entry in the same transaction. Needs MODIFY.
///
/// # Errors
/// The entry is not visible, the caller lacks MODIFY, metadata is invalid, or the database could not be written.
pub async fn set_metadata(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    id: &str,
    metadata: Metadata,
) -> Result<()> {
    let (_, target) = target_in(app, workspace, org, id).await?;
    iam::require(app, identity, org, sides, Privilege::Modify, &target).await?;
    check_metadata(&metadata.description, &metadata.tags).map_err(Error::Invalid)?;
    let mut uow = Uow::begin(app).await?;
    if !store::set_metadata(
        &mut uow.tx,
        org,
        id,
        metadata.description.as_deref(),
        &metadata.tags,
    )
    .await?
    {
        return Err(Error::NotFound("not found".into()));
    }
    uow.audit(
        org,
        &Who::of(identity),
        "securable.metadata.set",
        &target.label,
        "ok",
    )
    .await?;
    uow.commit().await
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
    store::forget_catalog_compute(&mut uow.tx, id).await?;
    store::put_policy(&mut uow.tx, org, id, &Policy::default(), "", 0).await?;
    store::delete(&mut *uow.tx, org, id).await
}

/// The compute a catalog says may read it, by id; empty means any compute of its workspace. Given the id of anything in the catalog.
///
/// # Errors
/// The database could not be read.
pub async fn compute_policy_of(app: &App, org: &str, id: &str) -> Result<Vec<String>> {
    let mut at = store::get(app.db.pool(), org, id)
        .await?
        .ok_or_else(|| Error::NotFound("not found".into()))?;
    while !at.parent_id.is_empty() {
        at = store::get(app.db.pool(), org, &at.parent_id)
            .await?
            .ok_or_else(|| Error::Internal("a parent is missing".into()))?;
    }
    store::compute_allowed(app.db.pool(), &at.id).await
}

/// The compute that may read a catalog. Whoever may see the catalog may ask.
///
/// # Errors
/// Not there for this caller, or not a catalog.
pub async fn compute_policy(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    catalog: &str,
) -> Result<Vec<String>> {
    let entry = get(app, identity, workspace, org, sides, catalog).await?;
    if entry.kind != Kind::Catalog {
        return Err(Error::Invalid("only a catalog has a compute policy".into()));
    }
    store::compute_allowed(app.db.pool(), catalog).await
}

/// Says which compute may read the catalog (empty: any compute of its workspace). Needs `MANAGE` on the catalog. The caller has
/// checked the compute is the workspace's.
///
/// # Errors
/// Not there for this caller, not a catalog, or not allowed.
pub async fn set_compute_policy(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    catalog: &str,
    targets: &[String],
) -> Result<()> {
    let (entry, target) = target_in(app, workspace, org, catalog).await?;
    if entry.kind != Kind::Catalog {
        return Err(Error::Invalid("only a catalog has a compute policy".into()));
    }
    iam::require(app, identity, org, sides, Privilege::Manage, &target).await?;
    let mut uow = Uow::begin(app).await?;
    store::set_compute_allowed(&mut uow.tx, catalog, targets).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "catalog.compute.set",
        &format!("catalog {catalog}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// A compute target is gone: no catalog names it any more.
///
/// # Errors
/// The database could not be written.
pub async fn forget_compute(uow: &mut Uow<'_>, target: &str) -> Result<()> {
    store::forget_compute(&mut uow.tx, target).await
}

/// Whether the row is still there, in any state (the drop check).
///
/// # Errors
/// The database could not be read.
pub async fn exists(app: &App, org: &str, id: &str) -> Result<bool> {
    Ok(store::get(app.db.pool(), org, id).await?.is_some())
}

/// Finds an entry by its names, `catalog`, then `schema`, then `table` or `volume`, as seen from a workspace. An entry that is not there,
/// is being dropped, or sits in another workspace's catalog is "not found".
///
/// # Errors
/// `NotFound`.
pub async fn resolve(
    app: &App,
    workspace: &str,
    org: &str,
    path: &[String],
) -> Result<(Securable, Target)> {
    let gone = || Error::NotFound("not found".into());
    let mut parent = String::new();
    for name in path {
        let next = store::child_by_name(app.db.pool(), org, workspace, &parent, name)
            .await?
            .filter(|s| s.state == "active")
            .ok_or_else(gone)?;
        parent = next.id;
    }
    if parent.is_empty() {
        return Err(gone());
    }
    target_in(app, workspace, org, &parent).await
}

/// The rules saved for a table (none: an empty policy). Only whoever manages the table may read them.
///
/// # Errors
/// Not there for this caller, not a table, or not allowed.
pub async fn policy_of(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    id: &str,
) -> Result<Policy> {
    let (entry, target) = target_in(app, workspace, org, id).await?;
    if entry.kind != Kind::Table {
        return Err(Error::Invalid(
            "only a table has row and column rules".into(),
        ));
    }
    iam::require(app, identity, org, sides, Privilege::Manage, &target).await?;
    Ok(store::get_policy(app.db.pool(), id)
        .await?
        .unwrap_or_default())
}

/// Saves the rules of a table, replacing any before them. Needs `MANAGE`.
///
/// # Errors
/// Not there for this caller, not a table, not allowed, or a rule is not valid.
pub async fn set_policy(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    id: &str,
    rules: Policy,
) -> Result<()> {
    let (entry, target) = target_in(app, workspace, org, id).await?;
    if entry.kind != Kind::Table {
        return Err(Error::Invalid(
            "only a table has row and column rules".into(),
        ));
    }
    iam::require(app, identity, org, sides, Privilege::Manage, &target).await?;
    policy::validate(&rules).map_err(Error::Invalid)?;
    let who = Who::of(identity);
    let mut uow = Uow::begin(app).await?;
    store::put_policy(&mut uow.tx, org, id, &rules, &owner_of(identity), now_ms()).await?;
    uow.audit(org, &who, "table.policy.set", &format!("table {id}"), "ok")
        .await?;
    uow.commit().await
}

/// What applies to this person when they read a table: the rules worked out for them, or nothing when the table has none or they
/// manage it (they see it whole).
///
/// # Errors
/// The table is not there.
pub async fn rules_for(
    app: &App,
    identity: &Identity,
    org: &str,
    sides: &[Side],
    id: &str,
) -> Result<Option<Resolved>> {
    let Some(rules) = store::get_policy(app.db.pool(), id).await? else {
        return Ok(None);
    };
    let (_, target) = target(app, org, id).await?;
    if iam::may(app, identity, org, sides, Privilege::Manage, &target).await? {
        return Ok(None);
    }
    policy::resolve(&rules, &owner_of(identity), &identity.groups)
        .map(Some)
        .map_err(Error::Internal)
}

/// Whether this person is held to rules on the table. Anything that would hand them its raw files (a job, an Iceberg client) is
/// refused then.
///
/// # Errors
/// The table is not there.
pub async fn is_restricted(
    app: &App,
    identity: &Identity,
    org: &str,
    sides: &[Side],
    id: &str,
) -> Result<bool> {
    Ok(rules_for(app, identity, org, sides, id).await?.is_some())
}
