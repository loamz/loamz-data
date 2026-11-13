//! Drop and erase. Asking for one writes a task; a worker leases the task, does the work in steps that are safe to
//! repeat, checks that the data is really gone, and only then records it as done. If the server stops half way, the
//! lease runs out and the task is taken again.

use super::{
    model::{LEASE_MS, MAX_ATTEMPTS, Task, TaskKind, retry_after_ms},
    store,
};
use crate::{
    governance::{
        audit::{
            model::{Who, tombstone},
            service::Uow,
        },
        catalogs::service as catalogs,
        iam::service as iam,
        orgs::{model::valid_principal, service as orgs},
        workspaces::service as workspaces,
    },
    kernel::{
        auth::Identity,
        authz::{Privilege, Side},
        error::{Error, Result},
        new_id, now_ms,
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

async fn run(app: &App, task: &Task) -> Result<()> {
    let org = &task.org_id;
    match TaskKind::parse(&task.kind) {
        Some(TaskKind::Drop) => remove_subtree(app, org, &task.subject).await,
        Some(TaskKind::Erase) => erase(app, task).await,
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
        .stores
        .list(folder)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
        .into_iter()
        .filter(|entry| entry.path.starts_with(&prefix))
    {
        app.stores
            .delete(folder, &entry.path)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    if app
        .stores
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
    let leased = {
        let mut conn = app.db.pool().acquire().await?;
        store::lease(&mut conn, now, now + LEASE_MS).await?
    };
    let Some(task) = leased else { return Ok(false) };
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
