use loamz_lake::sql::Db;
use sqlx::Row;

use super::model::Entry;
use crate::kernel::error::Result;

fn entry(r: &sqlx::any::AnyRow) -> std::result::Result<Entry, sqlx::Error> {
    Ok(Entry {
        seq: r.try_get("seq")?,
        at_ms: r.try_get("at_ms")?,
        tenant: r.try_get("tenant")?,
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
    "seq, at_ms, tenant, actor, on_behalf_of, action, target, outcome, prev_hash, hash";

pub async fn last(db: &Db) -> Result<Option<Entry>> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM audit ORDER BY seq DESC LIMIT 1"
    ))
    .fetch_optional(db.pool())
    .await?;
    Ok(row.as_ref().map(entry).transpose()?)
}

pub async fn insert(db: &Db, e: &Entry) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO audit ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"
    ))
    .bind(e.seq)
    .bind(e.at_ms)
    .bind(&e.tenant)
    .bind(&e.actor)
    .bind(&e.on_behalf_of)
    .bind(&e.action)
    .bind(&e.target)
    .bind(&e.outcome)
    .bind(&e.prev_hash)
    .bind(&e.hash)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Entries from `after` (exclusive), oldest first.
pub async fn since(db: &Db, after: i64, limit: i64) -> Result<Vec<Entry>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM audit WHERE seq > $1 ORDER BY seq LIMIT $2"
    ))
    .bind(after)
    .bind(limit)
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .iter()
        .map(entry)
        .collect::<std::result::Result<_, _>>()?)
}

/// The newest entries of one tenant, newest first.
pub async fn recent(db: &Db, tenant: &str, limit: i64) -> Result<Vec<Entry>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM audit WHERE tenant = $1 ORDER BY seq DESC LIMIT $2"
    ))
    .bind(tenant)
    .bind(limit)
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .iter()
        .map(entry)
        .collect::<std::result::Result<_, _>>()?)
}
