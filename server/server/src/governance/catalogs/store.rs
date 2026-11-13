use sqlx::{Any, Executor, Row};

use super::model::{Kind, Securable};
use crate::kernel::error::{Error, Result};

const COLUMNS: &str = "id, org_id, workspace_id, folder, kind, parent_id, name, owner, classification, placement, consistency, state, created_ms, metadata";

fn row(r: &sqlx::any::AnyRow) -> Result<Securable> {
    let kind: String = r.try_get("kind")?;
    Ok(Securable {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        folder: r.try_get("folder")?,
        kind: Kind::parse(&kind)
            .ok_or_else(|| Error::Internal(format!("unknown kind {kind} in the database")))?,
        parent_id: r.try_get("parent_id")?,
        name: r.try_get("name")?,
        owner: r.try_get("owner")?,
        classification: r.try_get("classification")?,
        placement: r.try_get("placement")?,
        consistency: r.try_get("consistency")?,
        state: r.try_get("state")?,
        created_ms: r.try_get("created_ms")?,
        metadata: r.try_get("metadata")?,
    })
}

pub async fn insert<'e, E: Executor<'e, Database = Any>>(e: E, s: &Securable) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO securables ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)"
    ))
    .bind(&s.id)
    .bind(&s.org_id)
    .bind(&s.workspace_id)
    .bind(&s.folder)
    .bind(s.kind.as_str())
    .bind(&s.parent_id)
    .bind(&s.name)
    .bind(&s.owner)
    .bind(&s.classification)
    .bind(&s.placement)
    .bind(&s.consistency)
    .bind(&s.state)
    .bind(s.created_ms)
    .bind(&s.metadata)
    .execute(e)
    .await?;
    Ok(())
}

/// How many catalogs the workspace owns.
pub async fn count_catalogs<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    workspace: &str,
) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM securables WHERE org_id = $1 AND workspace_id = $2 AND kind = 'catalog'",
    )
    .bind(org)
    .bind(workspace)
    .fetch_one(e)
    .await?)
}

pub async fn get<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
) -> Result<Option<Securable>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND id = $2"
    ))
    .bind(org)
    .bind(id)
    .fetch_optional(e)
    .await?;
    r.as_ref().map(row).transpose()
}

pub async fn name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    workspace: &str,
    parent: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query(
            "SELECT id FROM securables WHERE org_id = $1 AND workspace_id = $2 AND parent_id = $3 AND name = $4",
        )
            .bind(org)
            .bind(workspace)
            .bind(parent)
            .bind(name)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

pub async fn children<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    parent: &str,
) -> Result<Vec<Securable>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND parent_id = $2 ORDER BY name"
    ))
    .bind(org)
    .bind(parent)
    .fetch_all(e)
    .await?;
    rows.iter().map(row).collect()
}

pub async fn owned_by<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    owner: &str,
) -> Result<Vec<Securable>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND owner = $2 ORDER BY created_ms"
    ))
    .bind(org)
    .bind(owner)
    .fetch_all(e)
    .await?;
    rows.iter().map(row).collect()
}

pub async fn set_state<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
    state: &str,
) -> Result<()> {
    sqlx::query("UPDATE securables SET state = $3 WHERE org_id = $1 AND id = $2")
        .bind(org)
        .bind(id)
        .bind(state)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn set_owner<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
    owner: &str,
) -> Result<()> {
    sqlx::query("UPDATE securables SET owner = $3 WHERE org_id = $1 AND id = $2")
        .bind(org)
        .bind(id)
        .bind(owner)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn delete<'e, E: Executor<'e, Database = Any>>(e: E, org: &str, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM securables WHERE org_id = $1 AND id = $2")
        .bind(org)
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn child_by_name<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    workspace: &str,
    parent: &str,
    name: &str,
) -> Result<Option<Securable>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND workspace_id = $2 AND parent_id = $3 AND name = $4"
    ))
    .bind(org)
    .bind(workspace)
    .bind(parent)
    .bind(name)
    .fetch_optional(e)
    .await?;
    r.as_ref().map(row).transpose()
}

/// Moves a table to its next metadata file, only if it still is at `from` (a commit that lost a race gets false).
pub async fn advance<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
    from: &str,
    to: &str,
) -> Result<bool> {
    let r = sqlx::query(
        "UPDATE securables SET metadata = $4 WHERE org_id = $1 AND id = $2 AND metadata = $3 AND state = 'active'",
    )
    .bind(org)
    .bind(id)
    .bind(from)
    .bind(to)
    .execute(e)
    .await?;
    Ok(r.rows_affected() == 1)
}
