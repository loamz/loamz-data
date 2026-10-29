use loamz_lake::sql::Db;
use sqlx::Row;

use super::model::Task;
use crate::kernel::error::Result;

const COLUMNS: &str = "id, tenant, kind, subject, requested_by, on_behalf_of, state, attempts, lease_until, created_ms, last_error";

fn row(r: &sqlx::any::AnyRow) -> Result<Task> {
    Ok(Task {
        id: r.try_get("id")?,
        tenant: r.try_get("tenant")?,
        kind: r.try_get("kind")?,
        subject: r.try_get("subject")?,
        requested_by: r.try_get("requested_by")?,
        on_behalf_of: r.try_get("on_behalf_of")?,
        state: r.try_get("state")?,
        attempts: r.try_get("attempts")?,
        lease_until: r.try_get("lease_until")?,
        created_ms: r.try_get("created_ms")?,
        last_error: r.try_get("last_error")?,
    })
}

pub async fn insert(db: &Db, t: &Task) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO tasks ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"
    ))
    .bind(&t.id)
    .bind(&t.tenant)
    .bind(&t.kind)
    .bind(&t.subject)
    .bind(&t.requested_by)
    .bind(&t.on_behalf_of)
    .bind(&t.state)
    .bind(t.attempts)
    .bind(t.lease_until)
    .bind(t.created_ms)
    .bind(&t.last_error)
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn get(db: &Db, tenant: &str, id: &str) -> Result<Option<Task>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM tasks WHERE tenant = $1 AND id = $2"
    ))
    .bind(tenant)
    .bind(id)
    .fetch_optional(db.pool())
    .await?;
    r.as_ref().map(row).transpose()
}

/// Takes the oldest task that is due, if there is one. The update only succeeds if nobody else took it first: that is
/// what makes two workers safe.
pub async fn lease(db: &Db, now: i64, until: i64) -> Result<Option<Task>> {
    let due = sqlx::query("SELECT id FROM tasks WHERE state = 'pending' AND lease_until < $1 ORDER BY created_ms LIMIT 1")
        .bind(now)
        .fetch_optional(db.pool())
        .await?;
    let Some(due) = due else { return Ok(None) };
    let id: String = due.try_get("id")?;
    let taken = sqlx::query("UPDATE tasks SET lease_until = $3, attempts = attempts + 1 WHERE id = $1 AND state = 'pending' AND lease_until < $2")
        .bind(&id)
        .bind(now)
        .bind(until)
        .execute(db.pool())
        .await?;
    if taken.rows_affected() != 1 {
        return Ok(None);
    }
    let r = sqlx::query(&format!("SELECT {COLUMNS} FROM tasks WHERE id = $1"))
        .bind(&id)
        .fetch_one(db.pool())
        .await?;
    row(&r).map(Some)
}

pub async fn finish(db: &Db, id: &str) -> Result<()> {
    sqlx::query("UPDATE tasks SET state = 'done', last_error = '' WHERE id = $1")
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Leaves the task pending to be tried again at `retry_at`, or gives up.
pub async fn fail(db: &Db, id: &str, error: &str, retry_at: i64, give_up: bool) -> Result<()> {
    let state = if give_up { "failed" } else { "pending" };
    sqlx::query("UPDATE tasks SET state = $2, lease_until = $3, last_error = $4 WHERE id = $1")
        .bind(id)
        .bind(state)
        .bind(retry_at)
        .bind(error)
        .execute(db.pool())
        .await?;
    Ok(())
}
