use sqlx::{Any, Executor, Row};

use super::model::SavedQuery;
use crate::kernel::error::Result;

const COLUMNS: &str =
    "id, org_id, workspace_id, owner, name, sql, target_id, created_ms, updated_ms";

fn query(r: &sqlx::any::AnyRow) -> std::result::Result<SavedQuery, sqlx::Error> {
    Ok(SavedQuery {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        owner: r.try_get("owner")?,
        name: r.try_get("name")?,
        sql: r.try_get("sql")?,
        target_id: r.try_get("target_id")?,
        created_ms: r.try_get("created_ms")?,
        updated_ms: r.try_get("updated_ms")?,
    })
}

pub async fn insert<'e, E: Executor<'e, Database = Any>>(e: E, q: &SavedQuery) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO saved_queries ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"
    ))
    .bind(&q.id)
    .bind(&q.org_id)
    .bind(&q.workspace_id)
    .bind(&q.owner)
    .bind(&q.name)
    .bind(&q.sql)
    .bind(&q.target_id)
    .bind(q.created_ms)
    .bind(q.updated_ms)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn get<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    id: &str,
) -> Result<Option<SavedQuery>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM saved_queries WHERE workspace_id = $1 AND id = $2"
    ))
    .bind(workspace)
    .bind(id)
    .fetch_optional(e)
    .await?;
    Ok(r.as_ref().map(query).transpose()?)
}

/// Newest change first. `owner` limits it to one person's.
pub async fn list<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    owner: Option<&str>,
) -> Result<Vec<SavedQuery>> {
    let rows = if let Some(o) = owner {
        sqlx::query(&format!("SELECT {COLUMNS} FROM saved_queries WHERE workspace_id = $1 AND owner = $2 ORDER BY updated_ms DESC LIMIT 500"))
            .bind(workspace)
            .bind(o)
            .fetch_all(e)
            .await?
    } else {
        sqlx::query(&format!("SELECT {COLUMNS} FROM saved_queries WHERE workspace_id = $1 ORDER BY updated_ms DESC LIMIT 500"))
            .bind(workspace)
            .fetch_all(e)
            .await?
    };
    rows.iter()
        .map(query)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub async fn update<'e, E: Executor<'e, Database = Any>>(e: E, q: &SavedQuery) -> Result<()> {
    sqlx::query("UPDATE saved_queries SET name = $2, sql = $3, target_id = $4, updated_ms = $5 WHERE id = $1")
        .bind(&q.id)
        .bind(&q.name)
        .bind(&q.sql)
        .bind(&q.target_id)
        .bind(q.updated_ms)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn delete<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM saved_queries WHERE id = $1")
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}
