use std::collections::HashMap;

use sqlx::{Any, Executor, Row};

use super::model::Entry;
use crate::kernel::{error::Result, new_id};

fn entry(r: &sqlx::any::AnyRow) -> std::result::Result<Entry, sqlx::Error> {
    Ok(Entry {
        org: r.try_get("org_id")?,
        seq: r.try_get("seq")?,
        at_ms: r.try_get("at_ms")?,
        actor: r.try_get("actor")?,
        on_behalf_of: r.try_get("on_behalf_of")?,
        action: r.try_get("action")?,
        target: r.try_get("target")?,
        outcome: r.try_get("outcome")?,
        prev_hash: r.try_get("prev_hash")?,
        hash: r.try_get("hash")?,
    })
}

const COLUMNS: &str =
    "org_id, seq, at_ms, actor, on_behalf_of, action, target, outcome, prev_hash, hash";

pub async fn last<'e, E: Executor<'e, Database = Any>>(e: E, org: &str) -> Result<Option<Entry>> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM audit WHERE org_id = $1 ORDER BY seq DESC LIMIT 1"
    ))
    .bind(org)
    .fetch_optional(e)
    .await?;
    Ok(row.as_ref().map(entry).transpose()?)
}

pub async fn insert<'e, E: Executor<'e, Database = Any>>(e: E, x: &Entry) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO audit ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"
    ))
    .bind(&x.org)
    .bind(x.seq)
    .bind(x.at_ms)
    .bind(&x.actor)
    .bind(&x.on_behalf_of)
    .bind(&x.action)
    .bind(&x.target)
    .bind(&x.outcome)
    .bind(&x.prev_hash)
    .bind(&x.hash)
    .execute(e)
    .await?;
    Ok(())
}

/// Entries of one organization after `after` (exclusive), oldest first.
pub async fn since<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    after: i64,
    limit: i64,
) -> Result<Vec<Entry>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM audit WHERE org_id = $1 AND seq > $2 ORDER BY seq LIMIT $3"
    ))
    .bind(org)
    .bind(after)
    .bind(limit)
    .fetch_all(e)
    .await?;
    Ok(rows
        .iter()
        .map(entry)
        .collect::<std::result::Result<_, _>>()?)
}

/// The newest entries of one organization, newest first.
pub async fn recent<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    limit: i64,
) -> Result<Vec<Entry>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM audit WHERE org_id = $1 ORDER BY seq DESC LIMIT $2"
    ))
    .bind(org)
    .bind(limit)
    .fetch_all(e)
    .await?;
    Ok(rows
        .iter()
        .map(entry)
        .collect::<std::result::Result<_, _>>()?)
}

pub async fn name_ref(c: &mut sqlx::AnyConnection, org: &str, name: &str) -> Result<String> {
    if let Some(reference) =
        sqlx::query_scalar("SELECT ref FROM audit_names WHERE org_id = $1 AND name = $2")
            .bind(org)
            .bind(name)
            .fetch_optional(&mut *c)
            .await?
    {
        return Ok(reference);
    }

    let reference = format!("p_{}", new_id());
    sqlx::query(
        "INSERT INTO audit_names (org_id, ref, name) VALUES ($1,$2,$3) ON CONFLICT (org_id,name) DO NOTHING",
    )
    .bind(org)
    .bind(reference)
    .bind(name)
    .execute(&mut *c)
    .await?;
    Ok(
        sqlx::query_scalar("SELECT ref FROM audit_names WHERE org_id = $1 AND name = $2")
            .bind(org)
            .bind(name)
            .fetch_one(&mut *c)
            .await?,
    )
}

pub async fn names<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
) -> Result<HashMap<String, String>> {
    let rows = sqlx::query("SELECT ref, name FROM audit_names WHERE org_id = $1")
        .bind(org)
        .fetch_all(e)
        .await?;
    rows.iter()
        .map(|r| Ok((r.try_get("ref")?, r.try_get("name")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn forget_name<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    name: &str,
) -> Result<()> {
    sqlx::query("DELETE FROM audit_names WHERE org_id = $1 AND name = $2")
        .bind(org)
        .bind(name)
        .execute(e)
        .await?;
    Ok(())
}
