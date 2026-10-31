//! Drop and erase. Asking for one writes a task; a worker leases the task, does the work in steps that are safe to
//! repeat, checks that the data is really gone, and only then records it as done. If the server stops half way, the
//! lease runs out and the task is taken again.

use super::{
    model::{LEASE_MS, MAX_ATTEMPTS, Task, TaskKind, retry_after_ms},
    store,
};
use crate::{
    governance::{
        audit::{model::Who, service::Uow},
        catalogs::{model::Kind, service as catalogs},
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
    orgs::require_admin(app, identity, org).await?;
    if !valid_principal(subject) {
        return Err(Error::Invalid(
            "subject must be a principal such as user:<id>".into(),
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
    orgs::require_admin(app, identity, org).await?;
    store::get(app.db.pool(), org, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such task".into()))
}

/// Removes one entry's files, grants and row. Every step can be run again.
async fn remove_subtree(app: &App, org: &str, id: &str) -> Result<()> {
    let entries = catalogs::subtree(app, org, id).await?;
    for entry in entries.iter().rev() {
        if entry.kind.holds_files() {
            volumes::erase_files(app, &entry.id).await?;
        }
        let mut uow = Uow::begin(app).await?;
        iam::forget_securable(&mut uow, org, &entry.id).await?;
        if entry.kind == Kind::Catalog {
            workspaces::forget_catalog(&mut uow, &entry.id).await?;
        }
        catalogs::remove_row(&mut uow, org, &entry.id).await?;
        uow.commit().await?;
    }
    // The check: nothing of it may remain.
    for entry in &entries {
        if catalogs::exists(app, org, &entry.id).await?
            || (entry.kind.holds_files() && volumes::has_files(app, &entry.id).await?)
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
        Some(TaskKind::Erase) => {
            for entry in catalogs::owned_by(app, org, &task.subject).await? {
                remove_subtree(app, org, &entry.id).await?;
            }
            let mut uow = Uow::begin(app).await?;
            iam::forget_principal(&mut uow, org, &task.subject).await?;
            workspaces::forget_principal(&mut uow, org, &task.subject).await?;
            orgs::forget_principal(&mut uow, org, &task.subject).await?;
            uow.commit().await?;
            if !catalogs::owned_by(app, org, &task.subject)
                .await?
                .is_empty()
                || iam::count_for(app, org, &task.subject).await? != 0
                || workspaces::count_assignments(app, org, &task.subject).await? != 0
                || orgs::still_admin(app, org, &task.subject).await?
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
    let leased = {
        let mut conn = app.db.pool().acquire().await?;
        store::lease(&mut conn, now, now + LEASE_MS).await?
    };
    let Some(task) = leased else { return Ok(false) };
    let org = task.org_id.clone();
    match run(app, &task).await {
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
