//! Drop and erase. Asking for one writes a task; a worker leases the task, does the work in steps that are safe to
//! repeat, checks that the data is really gone, and only then records it as done. If the server stops half way, the
//! lease runs out and the task is taken again.

use super::{
    model::{LEASE_MS, MAX_ATTEMPTS, Task, TaskKind, retry_after_ms},
    store,
};
use crate::{
    governance::{
        audit::{model::Who, service as audit},
        catalogs::{model::Kind, service as catalogs},
        iam::service as iam,
    },
    kernel::{
        auth::Caller,
        authz::{Privilege, root},
        error::{Error, Result},
        new_id, now_ms,
        state::App,
    },
    storage::volumes::service as volumes,
};

async fn enqueue(app: &App, caller: &Caller, kind: TaskKind, subject: &str) -> Result<Task> {
    let who = Who::of(caller);
    let task = Task {
        id: new_id(),
        tenant: caller.tenant.clone(),
        kind: kind.as_str().into(),
        subject: subject.to_owned(),
        requested_by: who.actor,
        on_behalf_of: who.on_behalf_of,
        state: "pending".into(),
        attempts: 0,
        lease_until: 0,
        created_ms: now_ms(),
        last_error: String::new(),
    };
    store::insert(&app.db, &task).await?;
    Ok(task)
}

/// Starts dropping an entry and everything inside it. It disappears from view at once; the bytes go when the task runs.
///
/// # Errors
/// Not found or not allowed (`manage` is needed).
pub async fn request_drop(app: &App, caller: &Caller, id: &str) -> Result<Task> {
    let (entry, target) = catalogs::target(app, &caller.tenant, id).await?;
    iam::require(app, caller, Privilege::Manage, &target).await?;
    catalogs::mark_dropping(app, &caller.tenant, &entry.id).await?;
    let task = enqueue(app, caller, TaskKind::Drop, &entry.id).await?;
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        "drop.requested",
        &target.label,
        "ok",
    )
    .await?;
    Ok(task)
}

/// Starts erasing everything a person owns in this tenant, and every grant they hold. Needs `manage` on the tenant.
///
/// # Errors
/// Not allowed or no subject.
pub async fn request_erase(app: &App, caller: &Caller, subject: &str) -> Result<Task> {
    if subject.is_empty() {
        return Err(Error::Invalid("subject is required".into()));
    }
    iam::require(app, caller, Privilege::Manage, &root(&caller.tenant)).await?;
    let task = enqueue(app, caller, TaskKind::Erase, subject).await?;
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        "erase.requested",
        &format!("subject {subject}"),
        "ok",
    )
    .await?;
    Ok(task)
}

/// # Errors
/// Not found, or not allowed (`manage` on the tenant).
pub async fn task(app: &App, caller: &Caller, id: &str) -> Result<Task> {
    iam::require(app, caller, Privilege::Manage, &root(&caller.tenant)).await?;
    store::get(&app.db, &caller.tenant, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such task".into()))
}

/// Removes one entry's files, grants and row. Every step can be run again.
async fn remove_subtree(app: &App, tenant: &str, id: &str) -> Result<()> {
    let entries = catalogs::subtree(app, tenant, id).await?;
    for entry in entries.iter().rev() {
        if entry.kind == Kind::Volume {
            volumes::erase_files(app, &entry.id).await?;
        }
        iam::forget_securable(app, tenant, &entry.id).await?;
        catalogs::remove_row(app, tenant, &entry.id).await?;
    }
    // The check: nothing of it may remain.
    for entry in &entries {
        if catalogs::exists(app, tenant, &entry.id).await?
            || (entry.kind == Kind::Volume && volumes::has_files(app, &entry.id).await?)
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
    match TaskKind::parse(&task.kind) {
        Some(TaskKind::Drop) => remove_subtree(app, &task.tenant, &task.subject).await,
        Some(TaskKind::Erase) => {
            for entry in catalogs::owned_by(app, &task.tenant, &task.subject).await? {
                remove_subtree(app, &task.tenant, &entry.id).await?;
            }
            iam::forget_principal(app, &task.tenant, &task.subject).await?;
            if !catalogs::owned_by(app, &task.tenant, &task.subject)
                .await?
                .is_empty()
                || iam::count_for(app, &task.tenant, &task.subject).await? != 0
            {
                return Err(Error::Internal(
                    "something of the subject remains after the erase".into(),
                ));
            }
            Ok(())
        }
        None => Err(Error::Internal(format!("unknown task kind {}", task.kind))),
    }
}

/// Takes one due task and works on it. Returns whether there was one. `now` is passed in so a test can move the clock.
///
/// # Errors
/// The database could not be reached; a task that fails is retried, not returned.
pub async fn run_once(app: &App, now: i64) -> Result<bool> {
    let Some(task) = store::lease(&app.db, now, now + LEASE_MS).await? else {
        return Ok(false);
    };
    match run(app, &task).await {
        Ok(()) => {
            store::finish(&app.db, &task.id).await?;
            let what = format!(
                "{} {}",
                task.kind,
                if task.kind == "erase" {
                    format!("subject {}", task.subject)
                } else {
                    task.subject.clone()
                }
            );
            audit::record(
                app,
                &task.tenant,
                &Who::system(&task.on_behalf_of),
                &format!("{}.done", task.kind),
                &what,
                "ok",
            )
            .await?;
        }
        Err(e) => {
            let give_up = task.attempts >= MAX_ATTEMPTS;
            tracing::warn!(task = task.id, attempts = task.attempts, error = %e, "task failed");
            store::fail(
                &app.db,
                &task.id,
                &e.to_string(),
                now + retry_after_ms(task.attempts),
                give_up,
            )
            .await?;
            if give_up {
                audit::record(
                    app,
                    &task.tenant,
                    &Who::system(&task.on_behalf_of),
                    &format!("{}.failed", task.kind),
                    &task.subject,
                    "failed",
                )
                .await?;
            }
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
