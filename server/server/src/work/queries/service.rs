//! Saved queries: the person's own, and all of a workspace's for its admins. Every change is audited with the change itself.

use super::{
    model::{Draft, SavedQuery, problem},
    store,
};
use crate::{
    compute::service::Ctx,
    governance::audit::{model::Who, service::Uow},
    kernel::{
        error::{Error, Result},
        new_id, now_ms,
        state::App,
    },
};

fn mine(ctx: &Ctx<'_>, q: &SavedQuery) -> bool {
    ctx.standing.is_admin() || q.owner == ctx.identity.subject
}

fn missing() -> Error {
    Error::NotFound("no such saved query".into())
}

/// # Errors
/// The database could not be read.
pub async fn list(app: &App, ctx: &Ctx<'_>) -> Result<Vec<SavedQuery>> {
    let owner = (!ctx.standing.is_admin()).then_some(ctx.identity.subject.as_str());
    store::list(app.db.pool(), &ctx.workspace.id, owner).await
}

/// # Errors
/// `NotFound`: not there, or someone else's.
pub async fn get(app: &App, ctx: &Ctx<'_>, id: &str) -> Result<SavedQuery> {
    let q = store::get(app.db.pool(), &ctx.workspace.id, id)
        .await?
        .ok_or_else(missing)?;
    if mine(ctx, &q) { Ok(q) } else { Err(missing()) }
}

/// # Errors
/// The draft is not valid.
pub async fn create(app: &App, ctx: &Ctx<'_>, d: Draft) -> Result<SavedQuery> {
    if let Some(p) = problem(&d) {
        return Err(Error::Invalid(p.into()));
    }
    let now = now_ms();
    let q = SavedQuery {
        id: new_id(),
        org_id: ctx.workspace.org_id.clone(),
        workspace_id: ctx.workspace.id.clone(),
        owner: ctx.identity.subject.clone(),
        name: d.name.trim().into(),
        sql: d.sql,
        target_id: d.target_id,
        created_ms: now,
        updated_ms: now,
    };
    let mut uow = Uow::begin(app).await?;
    store::insert(&mut *uow.tx, &q).await?;
    uow.audit(
        &q.org_id,
        &Who::of(ctx.identity),
        "create.query",
        &format!("query {}", q.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(q)
}

/// # Errors
/// `NotFound`, or the draft is not valid.
pub async fn update(app: &App, ctx: &Ctx<'_>, id: &str, d: Draft) -> Result<SavedQuery> {
    if let Some(p) = problem(&d) {
        return Err(Error::Invalid(p.into()));
    }
    let mut q = get(app, ctx, id).await?;
    q.name = d.name.trim().into();
    q.sql = d.sql;
    q.target_id = d.target_id;
    q.updated_ms = now_ms();
    let mut uow = Uow::begin(app).await?;
    store::update(&mut *uow.tx, &q).await?;
    uow.audit(
        &q.org_id,
        &Who::of(ctx.identity),
        "update.query",
        &format!("query {}", q.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(q)
}

/// # Errors
/// `NotFound`.
pub async fn delete(app: &App, ctx: &Ctx<'_>, id: &str) -> Result<()> {
    let q = get(app, ctx, id).await?;
    let mut uow = Uow::begin(app).await?;
    store::delete(&mut *uow.tx, &q.id).await?;
    uow.audit(
        &q.org_id,
        &Who::of(ctx.identity),
        "delete.query",
        &format!("query {}", q.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(())
}
