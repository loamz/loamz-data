use std::fmt::Write as _;

use sqlx::{Any, Executor, Row};

use super::model::{Run, RunFilter, Target};
use crate::kernel::error::{Error, Result};

const TARGET: &str = "id, org_id, workspace_id, name, kind, endpoint, max_memory_mb, max_cpu_millis, created_by, created_ms, last_seen_ms";
const RUN: &str = "id, org_id, workspace_id, kind, requested_by, on_behalf_of, target_id, state, title, spec, result, stdout, stderr, exit_code, error, cancel, lease_until, created_ms, started_ms, finished_ms";

fn target(r: &sqlx::any::AnyRow) -> std::result::Result<Target, sqlx::Error> {
    Ok(Target {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
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
        "INSERT INTO compute_targets ({TARGET}, secret_hash) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"
    ))
    .bind(&t.id)
    .bind(&t.org_id)
    .bind(&t.workspace_id)
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
    workspace: &str,
) -> Result<Vec<Target>> {
    let rows = sqlx::query(&format!(
        "SELECT {TARGET} FROM compute_targets WHERE workspace_id = $1 ORDER BY name"
    ))
    .bind(workspace)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(target)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub async fn target_by_id<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    id: &str,
) -> Result<Option<Target>> {
    let r = sqlx::query(&format!(
        "SELECT {TARGET} FROM compute_targets WHERE workspace_id = $1 AND id = $2"
    ))
    .bind(workspace)
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
    workspace: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query("SELECT id FROM compute_targets WHERE workspace_id = $1 AND name = $2")
            .bind(workspace)
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

/// Serializes admission per organization across `PostgreSQL` instances and `SQLite` connections.
/// The no-op update takes a row lock until the surrounding transaction commits.
pub async fn has_capacity(c: &mut sqlx::AnyConnection, org: &str, limit: i64) -> Result<Capacity> {
    let locked = sqlx::query("UPDATE orgs SET id = id WHERE id = $1")
        .bind(org)
        .execute(&mut *c)
        .await?;
    if locked.rows_affected() != 1 {
        return Err(crate::kernel::error::Error::Internal(
            "run organization is missing".into(),
        ));
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM runs WHERE org_id = $1 AND state IN ('queued', 'running')",
    )
    .bind(org)
    .fetch_one(&mut *c)
    .await?;
    if active >= limit {
        return Ok(Capacity::OrganizationFull);
    }
    Ok(Capacity::Room)
}

/// Whether a workspace is under its own cap, if it has one.
pub async fn workspace_has_room(c: &mut sqlx::AnyConnection, workspace: &str) -> Result<bool> {
    let Some(cap) = workspace_quota(&mut *c, workspace).await? else {
        return Ok(true);
    };
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM runs WHERE workspace_id = $1 AND state IN ('queued', 'running')",
    )
    .bind(workspace)
    .fetch_one(&mut *c)
    .await?;
    Ok(active < cap)
}

/// What an admission found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capacity {
    Room,
    OrganizationFull,
}

/// The workspace's own cap on active runs, if it has one.
pub async fn workspace_quota(c: &mut sqlx::AnyConnection, workspace: &str) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT max_active_runs FROM workspace_quotas WHERE workspace_id = $1",
    )
    .bind(workspace)
    .fetch_optional(&mut *c)
    .await?)
}

/// Sets the workspace's cap (None: it has none of its own).
pub async fn set_workspace_quota(
    c: &mut sqlx::AnyConnection,
    workspace: &str,
    cap: Option<i64>,
) -> Result<()> {
    sqlx::query("DELETE FROM workspace_quotas WHERE workspace_id = $1")
        .bind(workspace)
        .execute(&mut *c)
        .await?;
    if let Some(n) = cap {
        sqlx::query("INSERT INTO workspace_quotas (workspace_id, max_active_runs) VALUES ($1,$2)")
            .bind(workspace)
            .bind(n)
            .execute(&mut *c)
            .await?;
    }
    Ok(())
}

pub async fn insert_run<'e, E: Executor<'e, Database = Any>>(e: E, r: &Run) -> Result<()> {
    let spec: serde_json::Value = serde_json::from_str(&r.spec)
        .map_err(|e| Error::Internal(format!("a run has a spec that is not JSON: {e}")))?;
    let result_folder = if spec["export"] == true {
        spec["result_folder"]
            .as_str()
            .filter(|folder| !folder.is_empty())
            .ok_or_else(|| Error::Internal("an exported run has no result folder".into()))?
    } else {
        ""
    };
    sqlx::query(&format!("INSERT INTO runs ({RUN}, result_folder) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)"))
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
        .bind(result_folder)
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

/// Newest first. The result, output and spec are left out of a list (they can be large): ask for the one run to read them.
pub async fn runs_list<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    f: &RunFilter,
    limit: i64,
) -> Result<Vec<Run>> {
    let mut sql = format!("SELECT {RUN} FROM runs WHERE workspace_id = $1");
    let mut n = 1;
    let mut text: Vec<&str> = Vec::new();
    let mut number: Vec<i64> = Vec::new();
    // Text filters first, then numbers: the placeholders are numbered in the order they are bound.
    if let Some(b) = &f.by {
        n += 1;
        let _ = write!(sql, " AND (requested_by = ${n} OR on_behalf_of = ${n})");
        text.push(b);
    }
    for (value, column) in [
        (&f.kind, "kind"),
        (&f.target, "target_id"),
        (&f.state, "state"),
    ] {
        if let Some(v) = value {
            n += 1;
            let _ = write!(sql, " AND {column} = ${n}");
            text.push(v);
        }
    }
    if let Some(t) = f.since_ms {
        n += 1;
        let _ = write!(sql, " AND created_ms >= ${n}");
        number.push(t);
    }
    if let Some(t) = f.before_ms {
        n += 1;
        let _ = write!(sql, " AND created_ms < ${n}");
        number.push(t);
    }
    let _ = write!(sql, " ORDER BY created_ms DESC LIMIT ${}", n + 1);
    let mut q = sqlx::query(&sql).bind(workspace);
    for t in text {
        q = q.bind(t);
    }
    for t in number {
        q = q.bind(t);
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

/// Saves the SQL policy rewrite that the target is about to execute.
pub async fn update_leased_spec(
    e: impl Executor<'_, Database = Any>,
    id: &str,
    target: &str,
    spec: &str,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE runs SET spec = $3 WHERE id = $1 AND target_id = $2 AND state = 'running'",
    )
    .bind(id)
    .bind(target)
    .bind(spec)
    .execute(e)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(Error::Conflict("the run is no longer leased".into()));
    }
    Ok(())
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

/// Records that a run's whole result is kept in `folder` (its size), or no longer is (0 and empty).
pub async fn set_result<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    bytes: i64,
    folder: &str,
) -> Result<()> {
    sqlx::query("UPDATE runs SET result_bytes = $2, result_folder = $3 WHERE id = $1")
        .bind(id)
        .bind(bytes)
        .bind(folder)
        .execute(e)
        .await?;
    Ok(())
}

/// The folder a run's whole result is in and its size, if one is kept.
pub async fn result_folder<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
) -> Result<Option<(String, i64)>> {
    let r = sqlx::query("SELECT result_bytes, result_folder FROM runs WHERE id = $1")
        .bind(id)
        .fetch_optional(e)
        .await?;
    Ok(match r {
        Some(row) if row.try_get::<i64, _>("result_bytes")? > 0 => Some((
            row.try_get::<String, _>("result_folder")?,
            row.try_get::<i64, _>("result_bytes")?,
        )),
        _ => None,
    })
}

/// Runs whose result file is to go: it is a day old, or the run was erased (an erased run has no spec any more).
pub async fn results_to_remove<'e, E: Executor<'e, Database = Any>>(
    e: E,
    before_ms: i64,
    limit: i64,
) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "SELECT id, result_folder FROM runs WHERE result_folder <> '' AND ((finished_ms > 0 AND finished_ms < $1) OR spec = '') LIMIT $2",
    )
    .bind(before_ms)
    .bind(limit)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get::<String, _>("id")?,
                r.try_get::<String, _>("result_folder")?,
            ))
        })
        .collect()
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

/// When each target of a workspace last finished a run (0 for none), by target id.
pub async fn last_used<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<std::collections::HashMap<String, i64>> {
    let rows = sqlx::query(
        "SELECT target_id, MAX(finished_ms) AS at FROM runs WHERE workspace_id = $1 AND finished_ms > 0 GROUP BY target_id",
    )
    .bind(workspace)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get::<String, _>("target_id")?,
                r.try_get::<i64, _>("at")?,
            ))
        })
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

/// For each target of a workspace with a statement or job not yet finished: when the oldest of them was asked for.
pub async fn unfinished_since<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<std::collections::HashMap<String, i64>> {
    let rows = sqlx::query(
        "SELECT target_id, MIN(created_ms) AS at FROM runs WHERE workspace_id = $1 AND state IN ('queued','running') GROUP BY target_id",
    )
    .bind(workspace)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get::<String, _>("target_id")?,
                r.try_get::<i64, _>("at")?,
            ))
        })
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

/// How many compute targets the workspace has.
pub async fn count_targets<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM compute_targets WHERE workspace_id = $1",
        )
        .bind(workspace)
        .fetch_one(e)
        .await?,
    )
}

/// Runs waiting and running, by state.
pub async fn queue_depths<'e, E: Executor<'e, Database = Any>>(e: E) -> Result<Vec<(String, i64)>> {
    let rows = sqlx::query("SELECT state, COUNT(*) AS n FROM runs WHERE state IN ('queued','running') GROUP BY state ORDER BY state")
        .fetch_all(e)
        .await?;
    rows.iter()
        .map(|r| Ok((r.try_get::<String, _>("state")?, r.try_get::<i64, _>("n")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_run_records_result_folder_before_upload() {
        let db = loamz_lake::sql::Db::connect("sqlite::memory:")
            .await
            .unwrap();
        db.migrate(&crate::MIGRATIONS).await.unwrap();
        let run = Run {
            id: "run-1".into(),
            org_id: "org-1".into(),
            workspace_id: "workspace-1".into(),
            kind: "sql".into(),
            requested_by: "user:alice".into(),
            on_behalf_of: "user:alice".into(),
            target_id: "target-1".into(),
            state: "queued".into(),
            title: "query".into(),
            spec: r#"{"export":true,"result_folder":"results/run-1"}"#.into(),
            result: String::new(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -1,
            error: String::new(),
            cancel: false,
            lease_until: 0,
            created_ms: 1,
            started_ms: 0,
            finished_ms: 0,
        };
        insert_run(db.pool(), &run).await.unwrap();

        let row = sqlx::query("SELECT result_folder FROM runs WHERE id = $1")
            .bind(&run.id)
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(
            row.try_get::<String, _>("result_folder").unwrap(),
            "results/run-1"
        );
    }
}
