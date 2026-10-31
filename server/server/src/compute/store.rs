use std::fmt::Write as _;

use sqlx::{Any, Executor, Row};

use super::model::{Run, Target};
use crate::kernel::error::Result;

const TARGET: &str = "id, org_id, name, kind, endpoint, max_memory_mb, max_cpu_millis, created_by, created_ms, last_seen_ms";
const RUN: &str = "id, org_id, workspace_id, kind, requested_by, on_behalf_of, target_id, state, title, spec, result, stdout, stderr, exit_code, error, cancel, lease_until, created_ms, started_ms, finished_ms";

fn target(r: &sqlx::any::AnyRow) -> std::result::Result<Target, sqlx::Error> {
    Ok(Target {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        name: r.try_get("name")?,
        kind: r.try_get("kind")?,
        endpoint: r.try_get("endpoint")?,
        max_memory_mb: r.try_get("max_memory_mb")?,
        max_cpu_millis: r.try_get("max_cpu_millis")?,
        created_by: r.try_get("created_by")?,
        created_ms: r.try_get("created_ms")?,
        last_seen_ms: r.try_get("last_seen_ms")?,
    })
}

fn run(r: &sqlx::any::AnyRow) -> std::result::Result<Run, sqlx::Error> {
    Ok(Run {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        kind: r.try_get("kind")?,
        requested_by: r.try_get("requested_by")?,
        on_behalf_of: r.try_get("on_behalf_of")?,
        target_id: r.try_get("target_id")?,
        state: r.try_get("state")?,
        title: r.try_get("title")?,
        spec: r.try_get("spec")?,
        result: r.try_get("result")?,
        stdout: r.try_get("stdout")?,
        stderr: r.try_get("stderr")?,
        exit_code: r.try_get("exit_code")?,
        error: r.try_get("error")?,
        cancel: r.try_get::<i64, _>("cancel")? != 0,
        lease_until: r.try_get("lease_until")?,
        created_ms: r.try_get("created_ms")?,
        started_ms: r.try_get("started_ms")?,
        finished_ms: r.try_get("finished_ms")?,
    })
}

// ---- targets ----------------------------------------------------------------------------------------------------

pub async fn insert_target<'e, E: Executor<'e, Database = Any>>(
    e: E,
    t: &Target,
    secret_hash: &str,
) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO compute_targets ({TARGET}, secret_hash) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"
    ))
    .bind(&t.id)
    .bind(&t.org_id)
    .bind(&t.name)
    .bind(&t.kind)
    .bind(&t.endpoint)
    .bind(t.max_memory_mb)
    .bind(t.max_cpu_millis)
    .bind(&t.created_by)
    .bind(t.created_ms)
    .bind(t.last_seen_ms)
    .bind(secret_hash)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn targets_in<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
) -> Result<Vec<Target>> {
    let rows = sqlx::query(&format!(
        "SELECT {TARGET} FROM compute_targets WHERE org_id = $1 ORDER BY name"
    ))
    .bind(org)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(target)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub async fn target_by_id<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
) -> Result<Option<Target>> {
    let r = sqlx::query(&format!(
        "SELECT {TARGET} FROM compute_targets WHERE org_id = $1 AND id = $2"
    ))
    .bind(org)
    .bind(id)
    .fetch_optional(e)
    .await?;
    Ok(r.as_ref().map(target).transpose()?)
}

pub async fn target_by_secret<'e, E: Executor<'e, Database = Any>>(
    e: E,
    hash: &str,
) -> Result<Option<Target>> {
    let r = sqlx::query(&format!(
        "SELECT {TARGET} FROM compute_targets WHERE secret_hash = $1"
    ))
    .bind(hash)
    .fetch_optional(e)
    .await?;
    Ok(r.as_ref().map(target).transpose()?)
}

pub async fn name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query("SELECT id FROM compute_targets WHERE org_id = $1 AND name = $2")
            .bind(org)
            .bind(name)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

pub async fn set_secret<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    hash: &str,
) -> Result<()> {
    sqlx::query("UPDATE compute_targets SET secret_hash = $2, last_seen_ms = 0 WHERE id = $1")
        .bind(id)
        .bind(hash)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn delete_target<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM compute_targets WHERE id = $1")
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn touch<'e, E: Executor<'e, Database = Any>>(e: E, id: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE compute_targets SET last_seen_ms = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(e)
        .await?;
    Ok(())
}

// ---- runs -------------------------------------------------------------------------------------------------------

pub async fn insert_run<'e, E: Executor<'e, Database = Any>>(e: E, r: &Run) -> Result<()> {
    sqlx::query(&format!("INSERT INTO runs ({RUN}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)"))
        .bind(&r.id)
        .bind(&r.org_id)
        .bind(&r.workspace_id)
        .bind(&r.kind)
        .bind(&r.requested_by)
        .bind(&r.on_behalf_of)
        .bind(&r.target_id)
        .bind(&r.state)
        .bind(&r.title)
        .bind(&r.spec)
        .bind(&r.result)
        .bind(&r.stdout)
        .bind(&r.stderr)
        .bind(r.exit_code)
        .bind(&r.error)
        .bind(i64::from(r.cancel))
        .bind(r.lease_until)
        .bind(r.created_ms)
        .bind(r.started_ms)
        .bind(r.finished_ms)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn run_get<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    id: &str,
) -> Result<Option<Run>> {
    let r = sqlx::query(&format!(
        "SELECT {RUN} FROM runs WHERE workspace_id = $1 AND id = $2"
    ))
    .bind(workspace)
    .bind(id)
    .fetch_optional(e)
    .await?;
    Ok(r.as_ref().map(run).transpose()?)
}

pub async fn run_by_id<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<Option<Run>> {
    let r = sqlx::query(&format!("SELECT {RUN} FROM runs WHERE id = $1"))
        .bind(id)
        .fetch_optional(e)
        .await?;
    Ok(r.as_ref().map(run).transpose()?)
}

/// Newest first. `by` limits it to what one person asked for; `kind` to statements or jobs. The result, output and spec are left out of
/// a list (they can be large): ask for the one run to read them.
pub async fn runs_list<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    by: Option<&str>,
    kind: Option<&str>,
    limit: i64,
) -> Result<Vec<Run>> {
    let mut sql = format!("SELECT {RUN} FROM runs WHERE workspace_id = $1");
    let mut n = 1;
    if by.is_some() {
        n += 1;
        let _ = write!(sql, " AND (requested_by = ${n} OR on_behalf_of = ${n})");
    }
    if kind.is_some() {
        n += 1;
        let _ = write!(sql, " AND kind = ${n}");
    }
    let _ = write!(sql, " ORDER BY created_ms DESC LIMIT ${}", n + 1);
    let mut q = sqlx::query(&sql).bind(workspace);
    if let Some(b) = by {
        q = q.bind(b);
    }
    if let Some(k) = kind {
        q = q.bind(k);
    }
    let rows = q.bind(limit).fetch_all(e).await?;
    rows.iter()
        .map(run)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

/// Takes the oldest queued run assigned to this target. The update only succeeds if nobody else took it first.
pub async fn lease_one(
    c: &mut sqlx::AnyConnection,
    target: &str,
    now: i64,
    until: i64,
) -> Result<Option<Run>> {
    let due = sqlx::query(
        "SELECT id FROM runs WHERE state = 'queued' AND target_id = $1 ORDER BY created_ms LIMIT 1",
    )
    .bind(target)
    .fetch_optional(&mut *c)
    .await?;
    let Some(due) = due else { return Ok(None) };
    let id: String = due.try_get("id")?;
    let taken = sqlx::query("UPDATE runs SET state = 'running', lease_until = $2, started_ms = $3 WHERE id = $1 AND state = 'queued'").bind(&id).bind(until).bind(now).execute(&mut *c).await?;
    if taken.rows_affected() != 1 {
        return Ok(None);
    }
    let r = sqlx::query(&format!("SELECT {RUN} FROM runs WHERE id = $1"))
        .bind(&id)
        .fetch_one(&mut *c)
        .await?;
    Ok(Some(run(&r)?))
}

/// Renews the lease of a run this target is working on; returns whether it should stop (it was cancelled).
pub async fn heartbeat<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    target: &str,
    until: i64,
) -> Result<Option<bool>> {
    let r = sqlx::query("UPDATE runs SET lease_until = $3 WHERE id = $1 AND target_id = $2 AND state = 'running' RETURNING cancel").bind(id).bind(target).bind(until).fetch_optional(e).await?;
    Ok(match r {
        Some(row) => Some(row.try_get::<i64, _>("cancel")? != 0),
        None => None,
    })
}

pub struct Outcome<'a> {
    pub state: &'a str,
    pub exit_code: i64,
    pub stdout: &'a str,
    pub stderr: &'a str,
    pub result: &'a str,
    pub error: &'a str,
}

/// Ends a run that is running on this target. Returns whether it was.
pub async fn finish<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    target: &str,
    o: &Outcome<'_>,
    now: i64,
) -> Result<bool> {
    let done = sqlx::query("UPDATE runs SET state = $3, exit_code = $4, stdout = $5, stderr = $6, result = $7, error = $8, finished_ms = $9 WHERE id = $1 AND target_id = $2 AND state = 'running'")
        .bind(id)
        .bind(target)
        .bind(o.state)
        .bind(o.exit_code)
        .bind(o.stdout)
        .bind(o.stderr)
        .bind(o.result)
        .bind(o.error)
        .bind(now)
        .execute(e)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Ends a run the server itself gives up on (never picked up, or its agent stopped answering).
pub async fn fail<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    from: &str,
    state: &str,
    error: &str,
    now: i64,
) -> Result<bool> {
    let done = sqlx::query(
        "UPDATE runs SET state = $3, error = $4, finished_ms = $5 WHERE id = $1 AND state = $2",
    )
    .bind(id)
    .bind(from)
    .bind(state)
    .bind(error)
    .bind(now)
    .execute(e)
    .await?;
    Ok(done.rows_affected() == 1)
}

pub async fn request_cancel<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<()> {
    sqlx::query("UPDATE runs SET cancel = 1 WHERE id = $1 AND state = 'running'")
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

/// Runs that are running past their lease, and queued ones nobody took in time: `(id, org, state)`.
pub async fn stale<'e, E: Executor<'e, Database = Any>>(
    e: E,
    now: i64,
    queued_before: i64,
) -> Result<Vec<(String, String, String)>> {
    let rows = sqlx::query("SELECT id, org_id, state FROM runs WHERE (state = 'running' AND lease_until < $1) OR (state = 'queued' AND created_ms < $2)").bind(now).bind(queued_before).fetch_all(e).await?;
    rows.iter()
        .map(|r| Ok((r.try_get("id")?, r.try_get("org_id")?, r.try_get("state")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

/// Runs still waiting or running on a target (used when the target is removed).
pub async fn open_on_target<'e, E: Executor<'e, Database = Any>>(
    e: E,
    target: &str,
) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "SELECT id, state FROM runs WHERE target_id = $1 AND state IN ('queued','running')",
    )
    .bind(target)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| Ok((r.try_get("id")?, r.try_get("state")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

// ---- run tokens -------------------------------------------------------------------------------------------------

pub struct TokenRow {
    pub run_id: String,
    pub org_id: String,
    pub workspace_id: String,
    pub identity: String,
    pub reads: String,
    pub writes: String,
    pub expires_ms: i64,
}

pub async fn insert_token<'e, E: Executor<'e, Database = Any>>(
    e: E,
    hash: &str,
    t: &TokenRow,
) -> Result<()> {
    sqlx::query("INSERT INTO run_tokens (token_hash, run_id, org_id, workspace_id, identity, reads, writes, expires_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(hash)
        .bind(&t.run_id)
        .bind(&t.org_id)
        .bind(&t.workspace_id)
        .bind(&t.identity)
        .bind(&t.reads)
        .bind(&t.writes)
        .bind(t.expires_ms)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn token_get<'e, E: Executor<'e, Database = Any>>(
    e: E,
    hash: &str,
) -> Result<Option<TokenRow>> {
    let r = sqlx::query("SELECT run_id, org_id, workspace_id, identity, reads, writes, expires_ms FROM run_tokens WHERE token_hash = $1").bind(hash).fetch_optional(e).await?;
    r.map(|r| {
        Ok(TokenRow {
            run_id: r.try_get("run_id")?,
            org_id: r.try_get("org_id")?,
            workspace_id: r.try_get("workspace_id")?,
            identity: r.try_get("identity")?,
            reads: r.try_get("reads")?,
            writes: r.try_get("writes")?,
            expires_ms: r.try_get("expires_ms")?,
        })
    })
    .transpose()
    .map_err(|e: sqlx::Error| e.into())
}

pub async fn tokens_delete_for_run<'e, E: Executor<'e, Database = Any>>(
    e: E,
    run: &str,
) -> Result<()> {
    sqlx::query("DELETE FROM run_tokens WHERE run_id = $1")
        .bind(run)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn tokens_delete_expired<'e, E: Executor<'e, Database = Any>>(
    e: E,
    now: i64,
) -> Result<()> {
    sqlx::query("DELETE FROM run_tokens WHERE expires_ms < $1")
        .bind(now)
        .execute(e)
        .await?;
    Ok(())
}

/// Every serverless target of every organization (the provider worker serves them all).
pub async fn serverless_targets<'e, E: Executor<'e, Database = Any>>(e: E) -> Result<Vec<Target>> {
    let rows = sqlx::query(&format!(
        "SELECT {TARGET} FROM compute_targets WHERE kind = 'serverless'"
    ))
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(target)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}
