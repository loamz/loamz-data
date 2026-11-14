use sqlx::{Any, Executor, Row};

use super::model::{Alert, Schedule, Task};
use crate::kernel::error::Result;

pub struct RunForErase {
    pub id: String,
    pub spec: String,
}

pub struct NewAlert<'a> {
    pub id: &'a str,
    pub org: &'a str,
    pub workspace: &'a str,
    pub schedule: &'a str,
    pub run: &'a str,
    pub destination: &'a str,
    pub created: i64,
}

const COLUMNS: &str = "id, org_id, kind, subject, requested_by, on_behalf_of, state, attempts, lease_until, created_ms, last_error, alert_ref";
const SCHEDULE_COLUMNS: &str = "id, org_id, workspace_id, name, owner, cadence_kind, cadence, definition, webhook_ref, state, next_run_ms, next_alert_ms, alert_lease_until, last_error, created_ms";

fn row(r: &sqlx::any::AnyRow) -> Result<Task> {
    Ok(Task {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        kind: r.try_get("kind")?,
        subject: r.try_get("subject")?,
        requested_by: r.try_get("requested_by")?,
        on_behalf_of: r.try_get("on_behalf_of")?,
        state: r.try_get("state")?,
        attempts: r.try_get("attempts")?,
        lease_until: r.try_get("lease_until")?,
        created_ms: r.try_get("created_ms")?,
        last_error: r.try_get("last_error")?,
        alert_ref: r.try_get("alert_ref")?,
    })
}

pub async fn insert<'e, E: Executor<'e, Database = Any>>(e: E, t: &Task) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO tasks ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"
    ))
    .bind(&t.id)
    .bind(&t.org_id)
    .bind(&t.kind)
    .bind(&t.subject)
    .bind(&t.requested_by)
    .bind(&t.on_behalf_of)
    .bind(&t.state)
    .bind(t.attempts)
    .bind(t.lease_until)
    .bind(t.created_ms)
    .bind(&t.last_error)
    .bind(&t.alert_ref)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn get<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
) -> Result<Option<Task>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM tasks WHERE org_id = $1 AND id = $2"
    ))
    .bind(org)
    .bind(id)
    .fetch_optional(e)
    .await?;
    r.as_ref().map(row).transpose()
}

/// Takes the oldest task that is due, if there is one. The update only succeeds if nobody else took it first: that is
/// what makes two workers safe.
pub async fn lease(c: &mut sqlx::AnyConnection, now: i64, until: i64) -> Result<Option<Task>> {
    let due = sqlx::query("SELECT id FROM tasks WHERE state = 'pending' AND lease_until < $1 ORDER BY created_ms LIMIT 1").bind(now).fetch_optional(&mut *c).await?;
    let Some(due) = due else { return Ok(None) };
    let id: String = due.try_get("id")?;
    let taken = sqlx::query("UPDATE tasks SET lease_until = $3, attempts = attempts + 1 WHERE id = $1 AND state = 'pending' AND lease_until < $2").bind(&id).bind(now).bind(until).execute(&mut *c).await?;
    if taken.rows_affected() != 1 {
        return Ok(None);
    }
    let r = sqlx::query(&format!("SELECT {COLUMNS} FROM tasks WHERE id = $1"))
        .bind(&id)
        .fetch_one(&mut *c)
        .await?;
    row(&r).map(Some)
}

pub async fn finish<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<()> {
    sqlx::query("UPDATE tasks SET state = 'done', last_error = '' WHERE id = $1")
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

/// Leaves the task pending to be tried again at `retry_at`, or gives up.
pub async fn fail<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    error: &str,
    retry_at: i64,
    give_up: bool,
) -> Result<()> {
    let state = if give_up { "failed" } else { "pending" };
    sqlx::query("UPDATE tasks SET state = $2, lease_until = $3, last_error = $4 WHERE id = $1")
        .bind(id)
        .bind(state)
        .bind(retry_at)
        .bind(error)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn runs_for_erase(
    c: &mut sqlx::AnyConnection,
    org: &str,
    subject: &str,
) -> Result<Vec<RunForErase>> {
    let rows = sqlx::query(
        "SELECT id, spec FROM runs WHERE org_id = $1 AND (requested_by = $2 OR on_behalf_of = $2)",
    )
    .bind(org)
    .bind(subject)
    .fetch_all(&mut *c)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(RunForErase {
                id: r.try_get("id")?,
                spec: r.try_get("spec")?,
            })
        })
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn erase_person(
    e: &mut sqlx::AnyConnection,
    org: &str,
    principal: &str,
    subject: &str,
    tombstone: &str,
    now: i64,
) -> Result<()> {
    sqlx::query("UPDATE orgs SET created_by = $3 WHERE id = $1 AND created_by = $2")
        .bind(org)
        .bind(subject)
        .bind(tombstone)
        .execute(&mut *e)
        .await?;
    for (table, column) in [
        ("account_admins", "added_by"),
        ("workspaces", "created_by"),
        ("grants", "granted_by"),
        ("compute_targets", "created_by"),
        ("service_principals", "created_by"),
        ("storage_credentials", "created_by"),
        ("storage_locations", "created_by"),
    ] {
        sqlx::query(&format!(
            "UPDATE {table} SET {column} = $3 WHERE org_id = $1 AND {column} = $2"
        ))
        .bind(org)
        .bind(subject)
        .bind(tombstone)
        .execute(&mut *e)
        .await?;
    }

    sqlx::query(
        "UPDATE schedules SET state = 'deleted', name = 'erased-' || id, owner = $3, definition = '', webhook_ref = '', last_error = '' \
         WHERE org_id = $1 AND owner = $2",
    )
    .bind(org)
    .bind(principal)
    .bind(tombstone)
    .execute(&mut *e)
    .await?;
    sqlx::query(
        "UPDATE tasks SET state = 'done', lease_until = 0, alert_ref = '', \
         requested_by = CASE WHEN requested_by = $2 THEN $3 ELSE requested_by END, \
         on_behalf_of = CASE WHEN on_behalf_of = $4 THEN $3 ELSE on_behalf_of END \
         WHERE kind = 'schedule.run' AND subject IN (SELECT id FROM schedules WHERE org_id = $1 AND owner = $3 AND state = 'deleted')",
    )
    .bind(org)
    .bind(subject)
    .bind(tombstone)
    .bind(principal)
    .execute(&mut *e)
    .await?;
    sqlx::query(
        "UPDATE schedule_alerts SET destination_ref = '', state = CASE WHEN state = 'pending' THEN 'cancelled' ELSE state END \
         WHERE org_id = $1 AND schedule_id IN (SELECT id FROM schedules WHERE org_id = $1 AND owner = $2 AND state = 'deleted')",
    )
    .bind(org)
    .bind(tombstone)
    .execute(&mut *e)
    .await?;

    sqlx::query("UPDATE tasks SET subject = $3, requested_by = CASE WHEN requested_by = $4 THEN $5 ELSE requested_by END, on_behalf_of = CASE WHEN on_behalf_of = $4 THEN $5 ELSE on_behalf_of END WHERE org_id = $1 AND (subject = $2 OR requested_by = $4 OR on_behalf_of = $4)")
            .bind(org)
            .bind(principal)
            .bind(tombstone)
            .bind(subject)
            .bind(tombstone)
            .execute(&mut *e)
            .await?;
    sqlx::query("UPDATE tasks SET last_error = REPLACE(REPLACE(last_error, $2, ''), $3, '') WHERE org_id = $1")
        .bind(org)
        .bind(subject)
        .bind(principal)
        .execute(&mut *e)
        .await?;
    sqlx::query("UPDATE runs SET requested_by = CASE WHEN requested_by = $3 THEN $4 ELSE requested_by END, on_behalf_of = CASE WHEN on_behalf_of = $3 THEN $4 ELSE on_behalf_of END, title = '', spec = '', result = '', stdout = '', stderr = '', error = '', cancel = CASE WHEN state IN ('queued','running') THEN 1 ELSE cancel END, finished_ms = CASE WHEN state IN ('queued','running') THEN $5 ELSE finished_ms END, state = CASE WHEN state IN ('queued','running') THEN 'cancelled' ELSE state END WHERE org_id = $1 AND (requested_by = $2 OR on_behalf_of = $2)")
            .bind(org)
            .bind(subject)
            .bind(subject)
            .bind(tombstone)
            .bind(now)
            .execute(&mut *e)
            .await?;
    Ok(())
}

pub async fn lock_org(c: &mut sqlx::AnyConnection, org: &str) -> Result<bool> {
    let updated = sqlx::query("UPDATE orgs SET id = id WHERE id = $1")
        .bind(org)
        .execute(&mut *c)
        .await?;
    Ok(updated.rows_affected() == 1)
}

pub async fn pause_schedules_for_person(
    c: &mut sqlx::AnyConnection,
    org: &str,
    principal: &str,
) -> Result<Vec<String>> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM schedules WHERE org_id = $1 AND owner = $2 AND state = 'active'",
    )
    .bind(org)
    .bind(principal)
    .fetch_all(&mut *c)
    .await?;
    let mut paused = Vec::new();
    for id in ids {
        let updated = sqlx::query(
            "UPDATE schedules SET state = 'paused', last_error = 'owner erasure pending' WHERE id = $1 AND state = 'active'",
        )
        .bind(&id)
        .execute(&mut *c)
        .await?;
        if updated.rows_affected() == 1 {
            paused.push(id);
        }
    }
    Ok(paused)
}

fn schedule_row(r: &sqlx::any::AnyRow) -> Result<Schedule> {
    Ok(Schedule {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        owner: r.try_get("owner")?,
        cadence_kind: r.try_get("cadence_kind")?,
        cadence: r.try_get("cadence")?,
        definition: r.try_get("definition")?,
        webhook_ref: r.try_get("webhook_ref")?,
        state: r.try_get("state")?,
        next_run_ms: r.try_get("next_run_ms")?,
        next_alert_ms: r.try_get("next_alert_ms")?,
        alert_lease_until: r.try_get("alert_lease_until")?,
        last_error: r.try_get("last_error")?,
        created_ms: r.try_get("created_ms")?,
    })
}

fn alert_row(r: &sqlx::any::AnyRow) -> Result<Alert> {
    Ok(Alert {
        id: r.try_get("id")?,
        schedule_id: r.try_get("schedule_id")?,
        run_id: r.try_get("run_id")?,
        state: r.try_get("state")?,
        attempts: r.try_get("attempts")?,
        created_ms: r.try_get("created_ms")?,
        sent_ms: r.try_get("sent_ms")?,
        last_error: r.try_get("last_error")?,
    })
}

pub async fn insert_schedule<'e, E: Executor<'e, Database = Any>>(
    e: E,
    s: &Schedule,
) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO schedules ({SCHEDULE_COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)"
    ))
    .bind(&s.id)
    .bind(&s.org_id)
    .bind(&s.workspace_id)
    .bind(&s.name)
    .bind(&s.owner)
    .bind(&s.cadence_kind)
    .bind(&s.cadence)
    .bind(&s.definition)
    .bind(&s.webhook_ref)
    .bind(&s.state)
    .bind(s.next_run_ms)
    .bind(s.next_alert_ms)
    .bind(s.alert_lease_until)
    .bind(&s.last_error)
    .bind(s.created_ms)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn schedule_name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    name: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM schedules WHERE workspace_id = $1 AND name = $2 AND state <> 'deleted'",
    )
    .bind(workspace)
    .bind(name)
    .fetch_one(e)
    .await?
        > 0)
}

pub async fn schedule<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    id: &str,
) -> Result<Option<Schedule>> {
    let row = sqlx::query(&format!(
        "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workspace_id = $1 AND id = $2 AND state <> 'deleted'"
    ))
    .bind(workspace)
    .bind(id)
    .fetch_optional(e)
    .await?;
    row.as_ref().map(schedule_row).transpose()
}

pub async fn schedule_by_id<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
) -> Result<Option<Schedule>> {
    let row = sqlx::query(&format!(
        "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(e)
    .await?;
    row.as_ref().map(schedule_row).transpose()
}

pub async fn schedules_for<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<Vec<Schedule>> {
    let rows = sqlx::query(&format!(
        "SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workspace_id = $1 AND state <> 'deleted' ORDER BY created_ms, id"
    ))
    .bind(workspace)
    .fetch_all(e)
    .await?;
    rows.iter().map(schedule_row).collect::<Result<Vec<_>>>()
}

pub async fn set_schedule_state<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    state: &str,
    next_run_ms: i64,
    last_error: &str,
) -> Result<bool> {
    let updated = sqlx::query("UPDATE schedules SET state = $2, next_run_ms = $3, last_error = $4 WHERE id = $1 AND state <> 'deleted'")
        .bind(id)
        .bind(state)
        .bind(next_run_ms)
        .bind(last_error)
        .execute(e)
        .await?;
    Ok(updated.rows_affected() == 1)
}

pub async fn due_schedule_id<'e, E: Executor<'e, Database = Any>>(
    e: E,
    now: i64,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT s.id FROM schedules s WHERE s.state = 'active' AND s.next_run_ms <= $1 \
         AND NOT EXISTS (SELECT 1 FROM tasks t WHERE t.kind = 'schedule.run' AND t.subject = s.id AND t.state = 'pending') \
         AND NOT EXISTS (SELECT 1 FROM tasks t JOIN runs r ON r.schedule_task_id = t.id \
         WHERE t.kind = 'schedule.run' AND t.subject = s.id AND r.state IN ('queued','running')) \
         ORDER BY s.next_run_ms, s.id LIMIT 1",
    )
    .bind(now)
    .fetch_optional(e)
    .await?)
}

pub async fn advance_schedule(
    c: &mut sqlx::AnyConnection,
    id: &str,
    due: i64,
    next: i64,
) -> Result<bool> {
    Ok(sqlx::query(
        "UPDATE schedules SET next_run_ms = $3 WHERE id = $1 AND state = 'active' AND next_run_ms = $2",
    )
    .bind(id)
    .bind(due)
    .bind(next)
    .execute(&mut *c)
    .await?
    .rows_affected()
        == 1)
}

pub async fn run_for_schedule_task<'e, E: Executor<'e, Database = Any>>(
    e: E,
    task_id: &str,
) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM runs WHERE schedule_task_id = $1")
            .bind(task_id)
            .fetch_optional(e)
            .await?,
    )
}

pub async fn failed_scheduled_runs<'e, E: Executor<'e, Database = Any>>(
    e: E,
) -> Result<Vec<(String, String, String, String, i64)>> {
    let rows = sqlx::query(
        "SELECT r.id, r.org_id, t.subject, t.alert_ref, r.finished_ms FROM runs r \
         JOIN tasks t ON t.id = r.schedule_task_id \
         WHERE t.kind = 'schedule.run' AND r.state IN ('failed','timed_out') \
         AND NOT EXISTS (SELECT 1 FROM schedule_alerts a WHERE a.run_id = r.id)",
    )
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get("id")?,
                r.try_get("org_id")?,
                r.try_get("subject")?,
                r.try_get("alert_ref")?,
                r.try_get("finished_ms")?,
            ))
        })
        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn insert_alert<'e, E: Executor<'e, Database = Any>>(
    e: E,
    alert: &NewAlert<'_>,
) -> Result<bool> {
    let state = if alert.destination.is_empty() {
        "recorded"
    } else {
        "pending"
    };
    let result = sqlx::query(
        "INSERT INTO schedule_alerts (id, org_id, workspace_id, schedule_id, run_id, destination_ref, state, attempts, next_try_ms, created_ms, sent_ms, last_error) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,0,$8,$8,0,'') ON CONFLICT (run_id) DO NOTHING",
    )
    .bind(alert.id)
    .bind(alert.org)
    .bind(alert.workspace)
    .bind(alert.schedule)
    .bind(alert.run)
    .bind(alert.destination)
    .bind(state)
    .bind(alert.created)
    .execute(e)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn alerts_for<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    schedule: &str,
) -> Result<Vec<Alert>> {
    let rows = sqlx::query(
        "SELECT id, schedule_id, run_id, state, attempts, created_ms, sent_ms, last_error FROM schedule_alerts \
         WHERE workspace_id = $1 AND schedule_id = $2 ORDER BY created_ms DESC LIMIT 100",
    )
    .bind(workspace)
    .bind(schedule)
    .fetch_all(e)
    .await?;
    rows.iter().map(alert_row).collect::<Result<Vec<_>>>()
}

pub async fn claim_alert_schedule(
    c: &mut sqlx::AnyConnection,
    now: i64,
    until: i64,
) -> Result<Option<String>> {
    let schedule: Option<String> = sqlx::query_scalar(
        "SELECT s.id FROM schedules s WHERE s.next_alert_ms <= $1 AND s.alert_lease_until < $1 \
         AND EXISTS (SELECT 1 FROM schedule_alerts a WHERE a.schedule_id = s.id AND a.state = 'pending' AND a.next_try_ms <= $1) \
         ORDER BY s.next_alert_ms, s.id LIMIT 1",
    )
    .bind(now)
    .fetch_optional(&mut *c)
    .await?;
    let Some(schedule) = schedule else {
        return Ok(None);
    };
    let claimed = sqlx::query(
        "UPDATE schedules SET alert_lease_until = $3 WHERE id = $1 AND alert_lease_until < $2 AND next_alert_ms <= $2",
    )
    .bind(&schedule)
    .bind(now)
    .bind(until)
    .execute(&mut *c)
    .await?;
    Ok((claimed.rows_affected() == 1).then_some(schedule))
}

pub async fn pending_alerts<'e, E: Executor<'e, Database = Any>>(
    e: E,
    schedule: &str,
    now: i64,
) -> Result<Vec<(Alert, String)>> {
    let rows = sqlx::query(
        "SELECT id, schedule_id, run_id, state, attempts, created_ms, sent_ms, last_error, destination_ref \
         FROM schedule_alerts WHERE schedule_id = $1 AND state = 'pending' AND next_try_ms <= $2 \
         ORDER BY created_ms, id LIMIT 1",
    )
    .bind(schedule)
    .bind(now)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|row| Ok((alert_row(row)?, row.try_get("destination_ref")?)))
        .collect::<Result<Vec<_>>>()
}

pub async fn alert_sent<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    sent_ms: i64,
) -> Result<()> {
    sqlx::query("UPDATE schedule_alerts SET state = 'sent', sent_ms = $2, last_error = '' WHERE id = $1 AND state = 'pending'")
        .bind(id)
        .bind(sent_ms)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn alert_retry<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    attempts: i64,
    next_try_ms: i64,
) -> Result<()> {
    let state = if attempts >= 8 { "failed" } else { "pending" };
    sqlx::query("UPDATE schedule_alerts SET state = $4, attempts = $2, next_try_ms = $3, last_error = 'delivery failed' WHERE id = $1 AND state = 'pending'")
        .bind(id)
        .bind(attempts)
        .bind(next_try_ms)
        .bind(state)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn finish_alert_lease<'e, E: Executor<'e, Database = Any>>(
    e: E,
    schedule: &str,
    next: i64,
) -> Result<()> {
    sqlx::query("UPDATE schedules SET next_alert_ms = $2, alert_lease_until = 0 WHERE id = $1")
        .bind(schedule)
        .bind(next)
        .execute(e)
        .await?;
    Ok(())
}
