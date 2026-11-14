//! Durable lifecycle work: dropping and erasing data, firing schedules, and delivering failure alerts.

use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr,
    time::Duration,
};

use super::{
    model::{LEASE_MS, MAX_ATTEMPTS, Schedule, Task, TaskKind, next_fire_ms, retry_after_ms},
    store,
};
use crate::{
    ai::mlflow::service as mlflow,
    compute::service as compute,
    governance::{
        audit::{
            model::{Who, tombstone},
            service::Uow,
        },
        catalogs::{model::Kind, service as catalogs},
        context::WsCtx,
        iam::service as iam,
        orgs::{model::valid_principal, service as orgs},
        workspaces::service as workspaces,
    },
    kernel::{
        auth::{Identity, Kind as IdentityKind, OrgRef},
        authz::{Privilege, Side},
        error::{Error, Result},
        new_id, now_ms,
        secrets::SecretRef,
        state::App,
    },
    storage::volumes::service as volumes,
};

fn new_task(org: &str, identity: &Identity, kind: TaskKind, subject: &str) -> Task {
    let who = Who::of(identity);
    Task {
        id: new_id(),
        org_id: org.to_owned(),
        kind: kind.as_str().into(),
        subject: subject.to_owned(),
        requested_by: who.actor,
        on_behalf_of: who.on_behalf_of,
        state: "pending".into(),
        attempts: 0,
        lease_until: 0,
        created_ms: now_ms(),
        last_error: String::new(),
        alert_ref: String::new(),
    }
}

/// Starts dropping an entry and everything inside it. It disappears from view at once; the bytes go when the task runs.
///
/// # Errors
/// Not found or not allowed (`MANAGE` is needed).
pub async fn request_drop(
    app: &App,
    identity: &Identity,
    workspace: &str,
    org: &str,
    sides: &[Side],
    id: &str,
) -> Result<Task> {
    let (entry, target) = catalogs::target_in(app, workspace, org, id).await?;
    iam::require(app, identity, org, sides, Privilege::Manage, &target).await?;
    let task = new_task(org, identity, TaskKind::Drop, &entry.id);
    let mut uow = Uow::begin(app).await?;
    catalogs::mark_dropping(&mut uow, org, &entry.id).await?;
    store::insert(&mut *uow.tx, &task).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "drop.requested",
        &target.label,
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(task)
}

/// Starts erasing everything a principal owns in this organization, and every grant, assignment and admin right they hold. Needs
/// an account admin.
///
/// # Errors
/// Not allowed, or not a valid principal.
pub async fn request_erase(
    app: &App,
    identity: &Identity,
    org: &str,
    subject: &str,
) -> Result<Task> {
    orgs::require_admin_hidden(app, identity, org).await?;
    if !valid_principal(subject)
        || !subject.starts_with("user:")
        || subject.chars().any(char::is_whitespace)
    {
        return Err(Error::Invalid(
            "subject must be a person such as user:<id>".into(),
        ));
    }
    let task = new_task(org, identity, TaskKind::Erase, subject);
    let mut uow = Uow::begin(app).await?;
    if !store::lock_org(&mut uow.tx, org).await? {
        return Err(Error::NotFound("not found".into()));
    }
    for schedule in store::pause_schedules_for_person(&mut uow.tx, org, subject).await? {
        uow.audit(
            org,
            &Who::of(identity),
            "schedule.paused",
            &format!("schedule {schedule}"),
            "erase_pending",
        )
        .await?;
    }
    store::insert(&mut *uow.tx, &task).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "erase.requested",
        &format!("subject {subject}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(task)
}

fn schedule_owner(identity: &Identity) -> String {
    format!("user:{}", identity.subject)
}

fn schedule_view(schedule: &Schedule) -> serde_json::Value {
    serde_json::json!({
        "id": schedule.id,
        "name": schedule.name,
        "state": schedule.state,
        "cadence_kind": schedule.cadence_kind,
        "cadence": schedule.cadence,
        "next_run_ms": schedule.next_run_ms,
        "alerts_enabled": !schedule.webhook_ref.is_empty(),
        "last_error": schedule.last_error,
    })
}

fn can_manage_schedule(
    identity: &Identity,
    standing: workspaces::Standing,
    schedule: &Schedule,
) -> bool {
    standing.is_admin() || schedule.owner == schedule_owner(identity)
}

async fn check_webhook(app: &App, reference: Option<&str>) -> Result<String> {
    let Some(reference) = reference.filter(|value| !value.is_empty()) else {
        return Ok(String::new());
    };
    let reference: SecretRef = reference
        .parse()
        .map_err(|_| Error::Invalid("webhook_ref must name a vault secret".into()))?;
    let vault = app.vault.as_deref().ok_or_else(|| {
        Error::Unavailable("this installation has no vault for webhook secrets".into())
    })?;
    let value = vault
        .get(&reference)
        .await
        .map_err(|_| Error::Unavailable("the webhook secret could not be read".into()))?;
    parse_webhook_url(&value)
        .map_err(|_| Error::Invalid("the webhook secret must contain a public HTTPS URL".into()))?;
    Ok(reference.to_string())
}

#[derive(serde::Deserialize)]
pub struct ScheduleDraft {
    pub name: String,
    pub cron: Option<String>,
    pub interval_seconds: Option<i64>,
    pub sql: Option<String>,
    pub job: Option<serde_json::Value>,
    pub maintenance: Option<compute::ScheduledMaintenance>,
    pub webhook_ref: Option<String>,
}

fn schedule_cadence(draft: &ScheduleDraft) -> Result<(&'static str, String)> {
    match (draft.cron.as_deref(), draft.interval_seconds) {
        (Some(cron), None) => Ok(("cron", cron_expression(cron)?)),
        (None, Some(seconds)) => {
            let seconds = seconds.to_string();
            next_fire_ms("interval", &seconds, now_ms())
                .map_err(|message| Error::Invalid(message.into()))?;
            Ok(("interval", seconds))
        }
        _ => Err(Error::Invalid(
            "give exactly one of cron or interval_seconds".into(),
        )),
    }
}

fn schedule_definition(draft: &ScheduleDraft) -> Result<(String, &'static str)> {
    match (&draft.sql, &draft.job, &draft.maintenance) {
        (Some(sql), None, None) if !sql.trim().is_empty() && sql.len() <= 100_000 => {
            Ok((serde_json::json!({"sql": sql}).to_string(), "sql-access"))
        }
        (None, Some(job), None) => {
            let _: compute::JobRequest = serde_json::from_value(job.clone())
                .map_err(|_| Error::Invalid("the job definition is invalid".into()))?;
            Ok((job.to_string(), "compute-create"))
        }
        (None, None, Some(maintenance)) => Ok((
            serde_json::to_string(&serde_json::json!({"maintenance": maintenance})).map_err(
                |_| Error::Internal("the maintenance definition could not be saved".into()),
            )?,
            "compute-create",
        )),
        _ => Err(Error::Invalid(
            "give exactly one of sql, job, or maintenance".into(),
        )),
    }
}

/// Saves an immutable statement or job to run as the person who made it.
///
/// # Errors
/// The caller is not a person with the matching compute entitlement, or the definition is invalid.
pub async fn create_schedule(
    app: &App,
    ctx: &WsCtx,
    draft: ScheduleDraft,
) -> Result<serde_json::Value> {
    if ctx.identity.kind != IdentityKind::Person || ctx.identity.actor.is_some() {
        return Err(Error::NotFound("not found".into()));
    }
    if !crate::governance::workspaces::model::valid_name(&draft.name) {
        return Err(Error::Invalid(
            "a schedule name is words, spaces, underscores and dashes, up to 64 characters".into(),
        ));
    }
    let (cadence_kind, cadence) = schedule_cadence(&draft)?;
    let (definition, entitlement) = schedule_definition(&draft)?;
    if !workspaces::entitled(
        app,
        &ctx.identity,
        &ctx.workspace.id,
        ctx.standing,
        entitlement,
    )
    .await?
    {
        crate::governance::audit::service::record(
            app,
            &ctx.workspace.org_id,
            &Who::of(&ctx.identity),
            "schedule.create",
            &format!("workspace {}", ctx.workspace.id),
            "denied",
        )
        .await?;
        return Err(Error::Denied(crate::kernel::error::Denied {
            action: "schedule work",
            target: format!("{entitlement} in {}", ctx.workspace.name),
        }));
    }
    if let Some(maintenance) = &draft.maintenance {
        let compute_ctx = compute::Ctx {
            identity: &ctx.identity,
            workspace: &ctx.workspace,
            standing: ctx.standing,
            sides: &ctx.sides,
            schedule_task_id: None,
        };
        compute::prepare_maintenance(
            app,
            &compute_ctx,
            &maintenance.table_id,
            &maintenance.action,
            maintenance.older_than,
        )
        .await?;
    }
    let webhook_ref = check_webhook(app, draft.webhook_ref.as_deref()).await?;
    let now = now_ms();
    let next_run_ms = next_fire_ms(cadence_kind, &cadence, now)
        .map_err(|message| Error::Invalid(message.into()))?;
    let schedule = Schedule {
        id: new_id(),
        org_id: ctx.workspace.org_id.clone(),
        workspace_id: ctx.workspace.id.clone(),
        name: draft.name,
        owner: schedule_owner(&ctx.identity),
        cadence_kind: cadence_kind.into(),
        cadence,
        definition,
        webhook_ref,
        state: "active".into(),
        next_run_ms,
        next_alert_ms: 0,
        alert_lease_until: 0,
        last_error: String::new(),
        created_ms: now,
    };
    let mut uow = Uow::begin(app).await?;
    if !store::lock_org(&mut uow.tx, &schedule.org_id).await? {
        return Err(Error::NotFound("not found".into()));
    }
    if orgs::is_erasing_in(&mut uow.tx, &schedule.org_id, &schedule.owner).await? {
        return Err(Error::Conflict("the schedule owner is being erased".into()));
    }
    if store::schedule_name_taken(&mut *uow.tx, &schedule.workspace_id, &schedule.name).await? {
        return Err(Error::Conflict(
            "a schedule with that name already exists".into(),
        ));
    }
    store::insert_schedule(&mut *uow.tx, &schedule).await?;
    uow.audit(
        &schedule.org_id,
        &Who::of(&ctx.identity),
        "schedule.create",
        &format!("schedule {}", schedule.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(schedule_view(&schedule))
}

/// Lists schedules the caller owns, or all schedules if they administer the workspace.
///
/// # Errors
/// The caller cannot open the workspace.
pub async fn schedules(app: &App, ctx: &WsCtx) -> Result<Vec<serde_json::Value>> {
    Ok(store::schedules_for(app.db.pool(), &ctx.workspace.id)
        .await?
        .iter()
        .filter(|schedule| can_manage_schedule(&ctx.identity, ctx.standing, schedule))
        .map(schedule_view)
        .collect())
}

async fn change_schedule(app: &App, ctx: &WsCtx, id: &str, state: &str) -> Result<()> {
    let schedule = store::schedule(app.db.pool(), &ctx.workspace.id, id)
        .await?
        .filter(|schedule| can_manage_schedule(&ctx.identity, ctx.standing, schedule))
        .ok_or_else(|| Error::NotFound("no such schedule".into()))?;
    let next_run = if state == "active" {
        next_fire_ms(&schedule.cadence_kind, &schedule.cadence, now_ms())
            .map_err(|message| Error::Invalid(message.into()))?
    } else {
        schedule.next_run_ms
    };
    let mut uow = Uow::begin(app).await?;
    if !store::lock_org(&mut uow.tx, &schedule.org_id).await? {
        return Err(Error::NotFound("no such schedule".into()));
    }
    if state == "active"
        && orgs::is_erasing_in(&mut uow.tx, &schedule.org_id, &schedule.owner).await?
    {
        return Err(Error::Conflict("the schedule owner is being erased".into()));
    }
    if !store::set_schedule_state(&mut *uow.tx, id, state, next_run, "").await? {
        return Err(Error::NotFound("no such schedule".into()));
    }
    uow.audit(
        &schedule.org_id,
        &Who::of(&ctx.identity),
        &format!("schedule.{state}"),
        &format!("schedule {id}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

pub async fn pause_schedule(app: &App, ctx: &WsCtx, id: &str) -> Result<()> {
    change_schedule(app, ctx, id, "paused").await
}

pub async fn resume_schedule(app: &App, ctx: &WsCtx, id: &str) -> Result<()> {
    change_schedule(app, ctx, id, "active").await
}

pub async fn delete_schedule(app: &App, ctx: &WsCtx, id: &str) -> Result<()> {
    change_schedule(app, ctx, id, "deleted").await
}

pub async fn schedule_alerts(app: &App, ctx: &WsCtx, id: &str) -> Result<Vec<serde_json::Value>> {
    let schedule = store::schedule(app.db.pool(), &ctx.workspace.id, id)
        .await?
        .filter(|schedule| can_manage_schedule(&ctx.identity, ctx.standing, schedule))
        .ok_or_else(|| Error::NotFound("no such schedule".into()))?;
    Ok(
        store::alerts_for(app.db.pool(), &ctx.workspace.id, &schedule.id)
            .await?
            .iter()
            .map(|alert| {
                serde_json::json!({
                    "id": alert.id,
                    "run_id": alert.run_id,
                    "state": alert.state,
                    "created_ms": alert.created_ms,
                    "sent_ms": alert.sent_ms,
                    "last_error": alert.last_error,
                })
            })
            .collect(),
    )
}

fn cron_expression(expression: &str) -> Result<String> {
    super::model::cron_expression(expression).map_err(|message| Error::Invalid(message.into()))
}

fn parse_webhook_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)
        .map_err(|_| Error::Invalid("the webhook address is not a URL".into()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid(
            "the webhook address must be public HTTPS".into(),
        ));
    }
    if let Some(host) = url.host_str()
        && let Ok(ip) = IpAddr::from_str(host)
        && !public_ip(ip)
    {
        return Err(Error::Invalid(
            "the webhook address must use a public IP".into(),
        ));
    }
    Ok(url)
}

async fn webhook_client(raw: &str) -> Result<(reqwest::Url, reqwest::Client)> {
    let url = parse_webhook_url(raw)?;
    let Some(host) = url.host_str() else {
        return Err(Error::Invalid(
            "the webhook address must be public HTTPS".into(),
        ));
    };
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<SocketAddr> = if let Ok(ip) = IpAddr::from_str(host) {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::time::timeout(
            Duration::from_secs(2),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| Error::Unavailable("the webhook host lookup timed out".into()))?
        .map_err(|_| Error::Invalid("the webhook host could not be resolved".into()))?
        .collect()
    };
    if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
        return Err(Error::Invalid(
            "the webhook host must resolve only to public addresses".into(),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, addresses[0])
        .build()
        .map_err(|_| Error::Unavailable("the webhook client could not be created".into()))?;
    Ok((url, client))
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19 || b == 51))
                || (a == 203 && b == 0 && c == 113)
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] <= 0x01ff || s[1] == 0x0db8))
                && s[0] != 0x2002
        }
    }
}

/// # Errors
/// Not found, or not an account admin.
pub async fn task(app: &App, identity: &Identity, org: &str, id: &str) -> Result<Task> {
    orgs::require_admin_hidden(app, identity, org).await?;
    store::get(app.db.pool(), org, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such task".into()))
}

/// Removes one entry's files, grants and row. Every step can be run again.
async fn remove_subtree(app: &App, org: &str, id: &str) -> Result<()> {
    let entries = catalogs::subtree(app, org, id).await?;
    for entry in entries.iter().rev() {
        if entry.kind.holds_files() {
            volumes::erase_files(app, &entry.folder).await?;
        }
        // An experiment, a model or a prompt also exists in MLflow: it goes there first, so the catalog never forgets what MLflow keeps.
        if matches!(entry.kind, Kind::Experiment | Kind::Model | Kind::Prompt) {
            mlflow::forget(app, &entry.id).await?;
        }
        let mut uow = Uow::begin(app).await?;
        iam::forget_securable(&mut uow, org, &entry.id).await?;
        catalogs::remove_row(&mut uow, org, &entry.id).await?;
        uow.commit().await?;
    }
    // The check: nothing of it may remain.
    for entry in &entries {
        if catalogs::exists(app, org, &entry.id).await?
            || (entry.kind.holds_files() && volumes::has_files(app, &entry.folder).await?)
        {
            return Err(Error::Internal(format!(
                "{} is not gone after the drop",
                entry.id
            )));
        }
    }
    Ok(())
}

async fn enqueue_due_schedule(app: &App, now: i64) -> Result<bool> {
    let Some(id) = store::due_schedule_id(app.db.pool(), now).await? else {
        return Ok(false);
    };
    let Some(schedule) = store::schedule_by_id(app.db.pool(), &id).await? else {
        return Ok(false);
    };
    let next = super::model::next_after_due(
        &schedule.cadence_kind,
        &schedule.cadence,
        schedule.next_run_ms,
        now,
    )
    .map_err(|message| Error::Internal(message.into()))?;
    let owner = schedule.owner.strip_prefix("user:").unwrap_or("");
    let task = Task {
        id: new_id(),
        org_id: schedule.org_id.clone(),
        kind: TaskKind::ScheduleRun.as_str().into(),
        subject: schedule.id.clone(),
        requested_by: owner.to_owned(),
        on_behalf_of: schedule.owner.clone(),
        state: "pending".into(),
        attempts: 0,
        lease_until: 0,
        created_ms: now,
        last_error: String::new(),
        alert_ref: schedule.webhook_ref.clone(),
    };
    let mut uow = Uow::begin(app).await?;
    if !store::advance_schedule(&mut uow.tx, &schedule.id, schedule.next_run_ms, next).await? {
        return Ok(false);
    }
    store::insert(&mut *uow.tx, &task).await?;
    uow.audit(
        &schedule.org_id,
        &Who::system(&schedule.owner),
        "schedule.fired",
        &format!("schedule {}", schedule.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(true)
}

fn scheduled_identity(schedule: &Schedule, subject: &str, groups: Vec<String>) -> Identity {
    Identity {
        subject: subject.to_owned(),
        kind: IdentityKind::Person,
        client: "schedule-worker".into(),
        name: subject.to_owned(),
        email: String::new(),
        groups,
        orgs: if schedule.org_id.starts_with("personal:") {
            vec![]
        } else {
            vec![OrgRef {
                id: schedule.org_id.clone(),
                alias: schedule.org_id.clone(),
            }]
        },
        roles: vec![],
        actor: None,
        scope: None,
        confined_to: None,
    }
}

async fn run_schedule(app: &App, task: &Task) -> Result<()> {
    if store::run_for_schedule_task(app.db.pool(), &task.id)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let Some(schedule) = store::schedule_by_id(app.db.pool(), &task.subject).await? else {
        return Ok(());
    };
    if schedule.state != "active" {
        return Ok(());
    }
    if orgs::is_erasing(app, &schedule.org_id, &schedule.owner).await? {
        return Ok(());
    }
    let Some(subject) = schedule.owner.strip_prefix("user:") else {
        return Err(Error::Internal("a schedule owner is not a person".into()));
    };
    let groups = if let Some(directory) = &app.directory {
        directory.groups(subject).await?
    } else {
        Vec::new()
    };
    let identity = scheduled_identity(&schedule, subject, groups);
    let (workspace, standing) = match workspaces::open(app, &identity, &schedule.workspace_id).await
    {
        Ok(opened) => opened,
        Err(Error::NotFound(_)) => {
            pause_for_lost_access(app, &schedule, task).await?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let sides = orgs::sides(app, &identity, &schedule.org_id).await?;
    let ctx = compute::Ctx {
        identity: &identity,
        workspace: &workspace,
        standing,
        sides: &sides,
        schedule_task_id: Some(&task.id),
    };
    let definition: serde_json::Value = serde_json::from_str(&schedule.definition)
        .map_err(|_| Error::Internal("a saved schedule definition is invalid".into()))?;
    let submitted = if let Some(sql) = definition.get("sql").and_then(serde_json::Value::as_str) {
        let request = compute::SqlRequest {
            sql: sql.to_owned(),
            limit: definition.get("limit").and_then(serde_json::Value::as_i64),
            target: definition
                .get("target")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            export: false,
            save: None,
            idempotency_key: None,
        };
        compute::submit_sql(app, &ctx, request).await.map(|_| ())
    } else if let Some(maintenance) = definition.get("maintenance") {
        let request: compute::ScheduledMaintenance = serde_json::from_value(maintenance.clone())
            .map_err(|_| Error::Internal("a saved maintenance definition is invalid".into()))?;
        compute::submit_maintenance(
            app,
            &ctx,
            &request.table_id,
            &request.action,
            request.older_than,
        )
        .await
        .map(|_| ())
    } else {
        let request: compute::JobRequest = serde_json::from_value(definition)
            .map_err(|_| Error::Internal("a saved schedule job is invalid".into()))?;
        compute::submit_job(app, &ctx, request).await.map(|_| ())
    };
    match submitted {
        Ok(()) => Ok(()),
        Err(Error::NotFound(_) | Error::Denied(_) | Error::Invalid(_)) => {
            pause_for_lost_access(app, &schedule, task).await
        }
        Err(error @ Error::Conflict(_)) => {
            let current = store::schedule_by_id(app.db.pool(), &schedule.id).await?;
            if current.is_some_and(|current| current.state != "active") {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

async fn pause_for_lost_access(app: &App, schedule: &Schedule, task: &Task) -> Result<()> {
    let mut uow = Uow::begin(app).await?;
    if !store::set_schedule_state(
        &mut *uow.tx,
        &schedule.id,
        "paused",
        schedule.next_run_ms,
        "access changed; schedule paused",
    )
    .await?
    {
        return Ok(());
    }
    uow.audit(
        &schedule.org_id,
        &Who::system(&task.on_behalf_of),
        "schedule.paused",
        &format!("schedule {}", schedule.id),
        "denied",
    )
    .await?;
    uow.commit().await
}

async fn record_failed_alerts(app: &App) -> Result<bool> {
    let failures = store::failed_scheduled_runs(app.db.pool()).await?;
    let mut recorded = false;
    for (run_id, org, schedule_id, destination, created) in failures {
        let Some(schedule) = store::schedule_by_id(app.db.pool(), &schedule_id).await? else {
            continue;
        };
        let mut uow = Uow::begin(app).await?;
        let alert_id = new_id();
        let alert = store::NewAlert {
            id: &alert_id,
            org: &org,
            workspace: &schedule.workspace_id,
            schedule: &schedule_id,
            run: &run_id,
            destination: &destination,
            created,
        };
        if store::insert_alert(&mut *uow.tx, &alert).await? {
            uow.audit(
                &org,
                &Who::system(&schedule.owner),
                "schedule.alert.recorded",
                &format!("run {run_id}"),
                "ok",
            )
            .await?;
            recorded = true;
        }
        uow.commit().await?;
    }
    Ok(recorded)
}

async fn deliver_alert(app: &App, alert: &super::model::Alert, reference: &str) -> bool {
    let Some(vault) = app.vault.as_deref() else {
        return false;
    };
    let Ok(reference) = reference.parse::<SecretRef>() else {
        return false;
    };
    let Ok(raw) = vault.get(&reference).await else {
        return false;
    };
    let Ok((url, client)) = webhook_client(&raw).await else {
        return false;
    };
    client
        .post(url)
        .header("Idempotency-Key", &alert.id)
        .json(&serde_json::json!({
            "id": alert.id,
            "schedule_id": alert.schedule_id,
            "run_id": alert.run_id,
            "state": "failed",
            "created_ms": alert.created_ms,
        }))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

async fn process_alerts(app: &App, now: i64) -> Result<bool> {
    let mut conn = app.db.pool().acquire().await?;
    let Some(schedule_id) = store::claim_alert_schedule(&mut conn, now, now + LEASE_MS).await?
    else {
        return Ok(false);
    };
    drop(conn);
    let Some((alert, reference)) = store::pending_alerts(app.db.pool(), &schedule_id, now)
        .await?
        .into_iter()
        .next()
    else {
        return Ok(false);
    };
    let delivered = deliver_alert(app, &alert, &reference).await;
    let mut uow = Uow::begin(app).await?;
    let action = if delivered {
        store::alert_sent(&mut *uow.tx, &alert.id, now).await?;
        "schedule.alert.sent"
    } else {
        let attempts = alert.attempts + 1;
        let delay = 30_000_i64.saturating_mul(1_i64 << attempts.min(7));
        store::alert_retry(&mut *uow.tx, &alert.id, attempts, now + delay).await?;
        "schedule.alert.retry"
    };
    store::finish_alert_lease(&mut *uow.tx, &schedule_id, now + 60_000).await?;
    if let Some(schedule) = store::schedule_by_id(&mut *uow.tx, &schedule_id).await? {
        uow.audit(
            &schedule.org_id,
            &Who::system(&schedule.owner),
            action,
            &format!("alert {}", alert.id),
            if delivered { "ok" } else { "failed" },
        )
        .await?;
    }
    uow.commit().await?;
    Ok(true)
}

async fn run(app: &App, task: &Task) -> Result<()> {
    let org = &task.org_id;
    match TaskKind::parse(&task.kind) {
        Some(TaskKind::Drop) => remove_subtree(app, org, &task.subject).await,
        Some(TaskKind::Erase) => erase(app, task).await,
        Some(TaskKind::ScheduleRun) => run_schedule(app, task).await,
        None => Err(Error::Internal(format!("unknown task kind {}", task.kind))),
    }
}

async fn run_output_folder(
    app: &App,
    org: &str,
    run: &store::RunForErase,
) -> Result<Option<String>> {
    let spec: serde_json::Value = serde_json::from_str(&run.spec)
        .map_err(|e| Error::Internal(format!("run {} has invalid spec JSON: {e}", run.id)))?;
    let Some(output) = spec.get("output").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    if output.is_empty() {
        return Ok(None);
    }
    let folder: Option<String> =
        sqlx::query_scalar("SELECT folder FROM securables WHERE id = $1 AND org_id = $2")
            .bind(output)
            .bind(org)
            .fetch_optional(app.db.pool())
            .await?;
    Ok(folder)
}

async fn erase_run_files(app: &App, run: &store::RunForErase, folder: &str) -> Result<()> {
    let prefix = format!("runs/{}/", run.id);
    for entry in app
        .places
        .list(folder)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
        .into_iter()
        .filter(|entry| entry.path.starts_with(&prefix))
    {
        app.places
            .delete(folder, &entry.path)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    if app
        .places
        .list(folder)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
        .iter()
        .any(|entry| entry.path.starts_with(&prefix))
    {
        return Err(Error::Internal(format!(
            "run output files for {} remain after erase",
            run.id
        )));
    }
    Ok(())
}

async fn erase(app: &App, task: &Task) -> Result<()> {
    let org = &task.org_id;
    let mut entries = Vec::new();
    for root in catalogs::owned_by(app, org, &task.subject).await? {
        entries.extend(catalogs::subtree(app, org, &root.id).await?);
    }
    let runs = {
        let mut conn = app.db.pool().acquire().await?;
        store::runs_for_erase(
            &mut conn,
            org,
            task.subject.strip_prefix("user:").unwrap_or(""),
        )
        .await?
    };
    let mut run_files = Vec::new();
    for run in &runs {
        if let Some(folder) = run_output_folder(app, org, run).await? {
            run_files.push((run, folder));
        }
    }
    let raw_subject = task.subject.strip_prefix("user:").unwrap_or("");
    let erased_as = tombstone(org, raw_subject);
    let mut uow = Uow::begin(app).await?;
    orgs::can_forget_principal_in(&mut uow.tx, org, &task.subject).await?;
    for (run, folder) in run_files {
        erase_run_files(app, run, &folder).await?;
    }
    for entry in entries.iter().rev() {
        if entry.kind.holds_files() {
            volumes::erase_files(app, &entry.folder).await?;
        }
        if matches!(entry.kind, Kind::Experiment | Kind::Model | Kind::Prompt) {
            mlflow::forget(app, &entry.id).await?;
        }
    }
    for entry in &entries {
        iam::forget_securable(&mut uow, org, &entry.id).await?;
    }
    for entry in entries.iter().rev() {
        catalogs::remove_row(&mut uow, org, &entry.id).await?;
    }
    iam::forget_principal(&mut uow, org, &task.subject).await?;
    workspaces::forget_principal(&mut uow, org, &task.subject).await?;
    orgs::forget_principal(&mut uow, org, &task.subject).await?;
    store::erase_person(
        &mut uow.tx,
        org,
        &task.subject,
        raw_subject,
        &erased_as,
        now_ms(),
    )
    .await?;
    for run in &runs {
        sqlx::query("DELETE FROM run_tokens WHERE run_id = $1")
            .bind(&run.id)
            .execute(&mut *uow.tx)
            .await?;
    }
    let remaining: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM securables WHERE org_id = $1 AND owner = $2) \
         + (SELECT COUNT(*) FROM grants WHERE org_id = $1 AND principal = $2) \
         + (SELECT COUNT(*) FROM account_admins WHERE org_id = $1 AND principal = $2) \
         + (SELECT COUNT(*) FROM workspace_members WHERE principal = $2 AND workspace_id IN (SELECT id FROM workspaces WHERE org_id = $1)) \
         + (SELECT COUNT(*) FROM entitlements WHERE principal = $2 AND workspace_id IN (SELECT id FROM workspaces WHERE org_id = $1))",
    )
    .bind(org)
    .bind(&task.subject)
    .fetch_one(&mut *uow.tx)
    .await?;
    if remaining != 0 {
        return Err(Error::Internal(
            "something of the subject remains after the erase".into(),
        ));
    }
    store::finish(&mut *uow.tx, &task.id).await?;
    uow.audit(
        org,
        &Who::system(&task.on_behalf_of),
        "erase.done",
        &format!("erase subject {}", task.subject),
        "ok",
    )
    .await?;
    crate::governance::audit::service::forget_person(&mut uow, org, raw_subject).await?;
    uow.commit().await?;
    Ok(())
}

/// Takes one due task and works on it. Returns whether there was one. `now` is passed in so a test can move the clock.
///
/// # Errors
/// The database could not be reached; a task that fails is retried, not returned.
pub async fn run_once(app: &App, now: i64) -> Result<bool> {
    let fired = enqueue_due_schedule(app, now).await?;
    let recorded = record_failed_alerts(app).await?;
    let delivered = process_alerts(app, now).await?;
    let leased = {
        let mut conn = app.db.pool().acquire().await?;
        store::lease(&mut conn, now, now + LEASE_MS).await?
    };
    let Some(task) = leased else {
        return Ok(fired || recorded || delivered);
    };
    let org = task.org_id.clone();
    match run(app, &task).await {
        Ok(()) if task.kind == "erase" => {}
        Ok(()) => {
            let what = format!(
                "{} {}",
                task.kind,
                if task.kind == "erase" {
                    format!("subject {}", task.subject)
                } else {
                    task.subject.clone()
                }
            );
            let mut uow = Uow::begin(app).await?;
            store::finish(&mut *uow.tx, &task.id).await?;
            uow.audit(
                &org,
                &Who::system(&task.on_behalf_of),
                &format!("{}.done", task.kind),
                &what,
                "ok",
            )
            .await?;
            uow.commit().await?;
        }
        Err(e) => {
            let give_up = task.attempts >= MAX_ATTEMPTS;
            tracing::warn!(task = task.id, attempts = task.attempts, error = %e, "task failed");
            let mut uow = Uow::begin(app).await?;
            store::fail(
                &mut *uow.tx,
                &task.id,
                &e.to_string(),
                now + retry_after_ms(task.attempts),
                give_up,
            )
            .await?;
            if give_up {
                uow.audit(
                    &org,
                    &Who::system(&task.on_behalf_of),
                    &format!("{}.failed", task.kind),
                    &task.subject,
                    "failed",
                )
                .await?;
            }
            uow.commit().await?;
        }
    }
    Ok(true)
}

/// Runs forever, looking for work every half second. Started once by the server.
pub async fn work_forever(app: crate::kernel::state::Shared) {
    loop {
        match run_once(&app, now_ms()).await {
            Ok(true) => {}
            Ok(false) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            Err(e) => {
                tracing::error!(error = %e, "the task worker could not reach the database");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn webhook_address_must_be_https_without_credentials_or_fragments() {
        assert!(parse_webhook_url("https://hooks.example.test/path").is_ok());
        for address in [
            "http://hooks.example.test/path",
            "https://user:pass@hooks.example.test/path",
            "https://hooks.example.test/path#fragment",
            "https://127.0.0.1/hook",
        ] {
            assert!(parse_webhook_url(address).is_err(), "{address}");
        }
    }

    #[test]
    fn webhook_dns_rejects_non_public_addresses() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "172.20.1.1",
            "192.168.1.1",
            "169.254.169.254",
            "::1",
            "fd00::1",
            "2001:10::1",
            "2001:db8::1",
            "2002:0808:0808::1",
        ] {
            assert!(!public_ip(address.parse().unwrap()), "{address}");
        }
        assert!(public_ip("93.184.216.34".parse().unwrap()));
        assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[tokio::test]
    async fn webhook_client_refuses_loopback_without_sending() {
        assert!(webhook_client("https://127.0.0.1/hook").await.is_err());
    }
}
