//! Compute: attaching it, asking it to run things, and the agents that do.
//!
//! The flow of a piece of work: a person asks (a SQL statement, or a job); everything it will read is checked against their rights
//! right then and the run is queued for a target. The target's agent, which connects out and asks for work, leases it and is given a
//! token that is the person's rights narrowed to what the work declared. The agent reads and writes only through the governed
//! file API with that token, and reports how it ended. The token dies with the run.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    model::{
        AGENT, DEFAULT_ROWS, LEASE_MS, Limits, MAX_OUTPUT, MAX_RESULT, MAX_ROWS, QUEUE_MS, Run,
        RunFilter, SERVERLESS, Target, cap, hash_secret, is_final_report, is_terminal, job_title,
        limits_for, mount_names, valid_target_name,
    },
    sql, store,
};
use crate::storage::tables::service as tables;
use crate::{
    governance::{
        audit::{model::Who, service::Uow},
        catalogs::{model::Kind, service as catalogs},
        iam::service as iam,
        orgs::service as orgs,
        workspaces::{
            model::Workspace,
            service::{self as workspaces, Standing},
        },
    },
    kernel::{
        auth::{Identity, Kind as WhoKind, OrgRef, Scope},
        authz::{Privilege, Side},
        error::{Denied, Error, Result},
        new_id, now_ms,
        state::App,
    },
};

// ---- targets (the workspace's) -------------------------------------------------------------------------------------

/// A target as it is first made: the secret is shown once, and only its hash is kept.
pub struct Enrolled {
    pub target: Target,
    pub secret: String,
}

/// The workspace's admins manage its compute; a service principal never does. Returns the workspace's organization.
async fn manage(
    app: &App,
    identity: &Identity,
    workspace: &str,
    action: &'static str,
) -> Result<String> {
    let (w, standing) = workspaces::open(app, identity, workspace).await?;
    if !standing.is_admin() || identity.confined_to.is_some() {
        return Err(workspaces::deny(app, identity, &w, action).await);
    }
    Ok(w.org_id)
}

/// # Errors
/// Not an admin of the workspace, or a bad or taken name.
pub async fn add_target(
    app: &App,
    identity: &Identity,
    workspace: &str,
    name: &str,
    max_memory_mb: i64,
    max_cpu_millis: i64,
) -> Result<Enrolled> {
    let org = &manage(app, identity, workspace, "attach compute").await?;
    if !valid_target_name(name) {
        return Err(Error::Invalid(
            "a name is words, digits, spaces and dashes, up to 64 characters".into(),
        ));
    }
    if !(256..=262_144).contains(&max_memory_mb) || !(100..=64_000).contains(&max_cpu_millis) {
        return Err(Error::Invalid(
            "memory must be 256 MB to 256 GB, and cpu 100 to 64000 thousandths of a CPU".into(),
        ));
    }
    let target = Target {
        id: new_id(),
        org_id: org.clone(),
        workspace_id: workspace.to_owned(),
        name: name.to_owned(),
        kind: AGENT.into(),
        endpoint: String::new(),
        max_memory_mb,
        max_cpu_millis,
        created_by: identity.subject.clone(),
        created_ms: now_ms(),
        last_seen_ms: 0,
    };
    let secret = crate::kernel::new_secret("agt_");
    let mut uow = Uow::begin(app).await?;
    if store::name_taken(&mut *uow.tx, workspace, name).await? {
        return Err(Error::Conflict(format!(
            "there is already compute called {name}"
        )));
    }
    store::insert_target(&mut *uow.tx, &target, &hash_secret(&secret)).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "compute.attach",
        &format!("compute {}", target.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(Enrolled { target, secret })
}

/// Registers a provider endpoint that starts when work arrives and stops when idle. The server calls it (outbound, https); it
/// never reaches back, and it is given only short-lived addresses for the files of one run.
///
/// # Errors
/// Not an admin of the workspace, a bad name or address, or the server has no token for serverless compute.
pub async fn add_serverless(
    app: &App,
    identity: &Identity,
    workspace: &str,
    name: &str,
    endpoint: Option<&str>,
    max_memory_mb: i64,
    max_cpu_millis: i64,
) -> Result<Target> {
    let org = &manage(app, identity, workspace, "attach compute").await?;
    if app.serverless_token.is_none() {
        return Err(Error::Invalid(
            "this server has no serverless token configured (LOAMZ_DATA_SERVERLESS_TOKEN_FILE)"
                .into(),
        ));
    }
    if !valid_target_name(name) {
        return Err(Error::Invalid(
            "a name is words, digits, spaces and dashes, up to 64 characters".into(),
        ));
    }
    let endpoint = endpoint
        .filter(|e| !e.is_empty())
        .or(app.serverless_endpoint.as_deref())
        .ok_or_else(|| Error::Invalid("name the endpoint: this server has none configured (LOAMZ_DATA_SERVERLESS_ENDPOINT)".into()))?;
    let url = reqwest::Url::parse(endpoint)
        .map_err(|_| Error::Invalid("the address is not a URL".into()))?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty() {
        return Err(Error::Invalid(
            "the address must be an https URL without credentials".into(),
        ));
    }
    if !(256..=262_144).contains(&max_memory_mb) || !(100..=64_000).contains(&max_cpu_millis) {
        return Err(Error::Invalid(
            "memory must be 256 MB to 256 GB, and cpu 100 to 64000 thousandths of a CPU".into(),
        ));
    }
    let target = Target {
        id: new_id(),
        org_id: org.clone(),
        workspace_id: workspace.to_owned(),
        name: name.to_owned(),
        kind: SERVERLESS.into(),
        endpoint: url.to_string(),
        max_memory_mb,
        max_cpu_millis,
        created_by: identity.subject.clone(),
        created_ms: now_ms(),
        last_seen_ms: 0,
    };
    let mut uow = Uow::begin(app).await?;
    if store::name_taken(&mut *uow.tx, workspace, name).await? {
        return Err(Error::Conflict(format!(
            "there is already compute called {name}"
        )));
    }
    // Nothing connects out with a secret here, so the stored one is random and never given to anyone.
    store::insert_target(
        &mut *uow.tx,
        &target,
        &hash_secret(&crate::kernel::new_secret("none_")),
    )
    .await?;
    uow.audit(
        org,
        &Who::of(identity),
        "compute.attach",
        &format!("compute {}", target.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(target)
}

/// A kind of compute this server can create for a workspace.
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub available: bool,
    pub max_memory_mb: i64,
    pub max_cpu_millis: i64,
    pub idle_stop_s: i64,
}

/// What compute can be created here: a serverless SQL warehouse (when the server has its endpoint and token), and a machine of
/// the organization's own that connects out (always).
#[must_use]
pub fn providers(app: &App) -> Vec<Provider> {
    vec![
        Provider {
            id: "serverless",
            name: "Serverless SQL warehouse",
            description: "Runs when you start it and stops after a minute with nothing to do; nothing starts it for you. Statements over data classified normal only.",
            available: app.serverless_token.is_some() && app.serverless_endpoint.is_some(),
            max_memory_mb: 2048,
            max_cpu_millis: 1000,
            idle_stop_s: 60,
        },
        Provider {
            id: "machine",
            name: "Your own machine",
            description: "A server you run connects out to this one and takes work in a sandbox. Nothing is opened on it.",
            available: true,
            max_memory_mb: 262_144,
            max_cpu_millis: 64_000,
            idle_stop_s: 0,
        },
    ]
}

/// For each target with work not yet finished: when the oldest of it was asked for.
///
/// # Errors
/// The database could not be read.
pub async fn unfinished_since(
    app: &App,
    workspace: &str,
) -> Result<std::collections::HashMap<String, i64>> {
    store::unfinished_since(app.db.pool(), workspace).await
}

/// When each of a workspace's compute last finished some work, by target id.
///
/// # Errors
/// The database could not be read.
pub async fn last_used(
    app: &App,
    workspace: &str,
) -> Result<std::collections::HashMap<String, i64>> {
    store::last_used(app.db.pool(), workspace).await
}

/// Every serverless target (for the provider worker).
///
/// # Errors
/// The database could not be read.
pub async fn serverless_targets(app: &App) -> Result<Vec<Target>> {
    store::serverless_targets(app.db.pool()).await
}

/// Gives a target a new secret; the agent using the old one is turned away.
///
/// # Errors
/// Not an account admin, or no such target.
pub async fn new_secret(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
) -> Result<Enrolled> {
    let org = &manage(app, identity, workspace, "renew compute's secret").await?;
    let target = store::target_by_id(app.db.pool(), workspace, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such compute".into()))?;
    let secret = crate::kernel::new_secret("agt_");
    let mut uow = Uow::begin(app).await?;
    store::set_secret(&mut *uow.tx, id, &hash_secret(&secret)).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "compute.secret",
        &format!("compute {id}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(Enrolled {
        target: Target {
            last_seen_ms: 0,
            ..target
        },
        secret,
    })
}

/// Starts (`start`) or stops (`stop`) serverless compute now. A machine is started and stopped on the machine itself.
///
/// # Errors
/// Not an account admin, no such compute, it is a machine, or the endpoint could not be reached.
pub async fn power(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
    start: bool,
) -> Result<()> {
    let org = &manage(app, identity, workspace, "start or stop compute").await?;
    let target = store::target_by_id(app.db.pool(), workspace, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such compute".into()))?;
    if !target.serverless() {
        return Err(Error::Invalid(
            "a machine is started and stopped on the machine itself".into(),
        ));
    }
    let action = if start { "warm" } else { "stop" };
    let outcome = super::serverless::signal(app, &target, action).await;
    if outcome.is_ok() {
        // For serverless compute `last_seen_ms` says when it was last started (positive) or stopped (negative) by hand.
        let now = now_ms();
        store::touch(app.db.pool(), id, if start { now } else { -now }).await?;
    }
    crate::governance::audit::service::record(
        app,
        org,
        &Who::of(identity),
        if start {
            "compute.start"
        } else {
            "compute.stop"
        },
        &format!("compute {id}"),
        if outcome.is_ok() { "ok" } else { "failed" },
    )
    .await?;
    outcome
}

/// Removes a target. What was waiting or running on it fails.
///
/// # Errors
/// Not an account admin, or no such target.
pub async fn remove_target(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let org = &manage(app, identity, workspace, "remove compute").await?;
    store::target_by_id(app.db.pool(), workspace, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such compute".into()))?;
    let open = store::open_on_target(app.db.pool(), id).await?;
    let mut uow = Uow::begin(app).await?;
    let now = now_ms();
    for (run, state) in &open {
        store::fail(
            &mut *uow.tx,
            run,
            state,
            "failed",
            "the compute was removed",
            now,
        )
        .await?;
        store::tokens_delete_for_run(&mut *uow.tx, run).await?;
    }
    store::delete_target(&mut *uow.tx, id).await?;
    catalogs::forget_compute(&mut uow, id).await?;
    uow.audit(
        org,
        &Who::of(identity),
        "compute.remove",
        &format!("compute {id}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// The workspace's targets, with whether each is answering. Anyone who is in the workspace may look.
///
/// # Errors
/// Not in the workspace.
pub async fn targets(app: &App, identity: &Identity, workspace: &str) -> Result<Vec<Target>> {
    workspaces::open(app, identity, workspace).await?;
    store::targets_in(app.db.pool(), workspace).await
}

/// How many compute targets a workspace has. A workspace that has any cannot be deleted.
///
/// # Errors
/// The database could not be read.
pub async fn count_in_workspace(app: &App, workspace: &str) -> Result<i64> {
    store::count_targets(app.db.pool(), workspace).await
}

// ---- asking for work (a person's) -------------------------------------------------------------------------------

/// Who is asking, from where, and as what.
pub struct Ctx<'a> {
    pub identity: &'a Identity,
    pub workspace: &'a Workspace,
    pub standing: Standing,
    pub sides: &'a [Side],
    pub schedule_task_id: Option<&'a str>,
}

#[derive(Deserialize)]
pub struct SqlRequest {
    pub sql: String,
    pub limit: Option<i64>,
    pub target: Option<String>,
}

#[derive(Deserialize)]
pub struct JobRequest {
    #[serde(default)]
    pub title: String,
    pub script: String,
    /// Tables and volumes the script may read, by id. Each is mounted read-only at `/inputs/<name>`.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// The volume that receives what the script leaves in `/workspace`, under `runs/<run id>/`. Empty: the job leaves no files.
    #[serde(default)]
    pub output: String,
    /// Tables the script may write, as `catalog.schema.table`: one that exists (needs `MODIFY`) or one to create (needs `CREATE TABLE`
    /// on its schema). The sandbox reaches them through two narrow doors; see proposal 0005.
    #[serde(default)]
    pub table_writes: Vec<String>,
    /// An address the runner downloads into `/inputs/fetched/data` before the sandbox starts. Set only by the server's own import.
    #[serde(skip)]
    pub fetch: Option<String>,
    pub target: Option<String>,
    pub memory_mb: Option<i64>,
    pub cpu_millis: Option<i64>,
    pub timeout_s: Option<i64>,
}

/// The person a run is for, as it will be rebuilt when its token is used. The groups are the ones they had when they asked.
#[derive(Serialize, Deserialize)]
struct As {
    subject: String,
    service: bool,
    name: String,
    email: String,
    groups: Vec<String>,
    actor: Option<String>,
}

impl As {
    fn of(i: &Identity) -> Self {
        Self {
            subject: i.subject.clone(),
            service: i.kind == WhoKind::Service,
            name: i.name.clone(),
            email: i.email.clone(),
            groups: i.groups.clone(),
            actor: i.actor.clone(),
        }
    }

    fn identity(self, org: &str, scope: Scope) -> Identity {
        Identity {
            subject: self.subject,
            kind: if self.service {
                WhoKind::Service
            } else {
                WhoKind::Person
            },
            client: "run".into(),
            name: self.name,
            email: self.email,
            groups: self.groups,
            // A personal space is the person's own and needs no organization; for an organization the id is its name.
            orgs: if org.starts_with("personal:") {
                vec![]
            } else {
                vec![OrgRef {
                    id: org.to_owned(),
                    alias: org.to_owned(),
                }]
            },
            roles: vec![],
            actor: self.actor,
            scope: Some(scope),
            confined_to: None,
        }
    }
}

async fn entitled(app: &App, ctx: &Ctx<'_>, what: &str) -> Result<()> {
    if workspaces::entitled(app, ctx.identity, &ctx.workspace.id, ctx.standing, what).await? {
        return Ok(());
    }
    crate::governance::audit::service::record(
        app,
        &ctx.workspace.org_id,
        &Who::of(ctx.identity),
        what,
        &format!("workspace {}", ctx.workspace.id),
        "denied",
    )
    .await?;
    Err(Error::Denied(Denied {
        action: "use this",
        target: format!("{what} in {}", ctx.workspace.name),
    }))
}

/// Picks the compute to run on: the one asked for, or the first that is answering and big enough.
async fn pick(
    app: &App,
    workspace: &str,
    asked: Option<&str>,
    memory: Option<i64>,
    cpu: Option<i64>,
    own_only: bool,
    policies: &[Vec<String>],
) -> Result<Target> {
    let now = now_ms();
    let mut all = store::targets_in(app.db.pool(), workspace).await?;
    let used = store::last_used(app.db.pool(), workspace).await?;
    let up = |t: &Target| t.running(used.get(&t.id).copied().unwrap_or(0), now);
    // The organization's own machines come first; a provider's is used when asked for or when nothing else answers.
    all.sort_by_key(Target::serverless);
    // Each catalog the work reads may name the compute allowed to read it (none named: any); every one must allow the compute.
    let allowed = |t: &Target| policies.iter().all(|p| p.is_empty() || p.contains(&t.id));
    let big_enough = |t: &Target| {
        memory.is_none_or(|m| m <= t.max_memory_mb) && cpu.is_none_or(|c| c <= t.max_cpu_millis)
    };
    let too_big = |online: &[&Target]| {
        let (m, c) = online.iter().fold((0, 0), |(m, c), t| {
            (m.max(t.max_memory_mb), c.max(t.max_cpu_millis))
        });
        Error::Invalid(format!(
            "that is more than the compute allows: at most {m} MB and {c} thousandths of a CPU"
        ))
    };
    if let Some(id) = asked {
        let t = all
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| Error::NotFound("no such compute".into()))?;
        if !allowed(t) {
            return Err(Error::NotFound("no such compute".into()));
        }
        if !up(t) {
            return Err(Error::Conflict(if t.serverless() {
                format!("{} is stopped. Start it first.", t.name)
            } else {
                format!("{} is not answering", t.name)
            }));
        }
        if own_only && t.serverless() {
            return Err(Error::Invalid(format!(
                "{} is run by a provider, which only takes statements over data classified normal",
                t.name
            )));
        }
        return if big_enough(t) {
            Ok(t.clone())
        } else {
            Err(too_big(&[t]))
        };
    }
    let online: Vec<&Target> = all
        .iter()
        .filter(|t| up(t) && !(own_only && t.serverless()) && allowed(t))
        .collect();
    if online.is_empty() {
        let any_up = all.iter().any(|t| up(t) && !(own_only && t.serverless()));
        return Err(Error::Conflict(if any_up {
            "no running compute is allowed to read every catalog this reads".into()
        } else {
            "no compute is running. Start a SQL warehouse, or attach a machine in this workspace's Compute and start its agent"
                .into()
        }));
    }
    online
        .iter()
        .find(|t| big_enough(t))
        .map(|t| (*t).clone())
        .ok_or_else(|| too_big(&online))
}

fn new_run(ctx: &Ctx<'_>, kind: &str, title: &str, target: &Target, spec: &Value) -> Run {
    let who = Who::of(ctx.identity);
    Run {
        id: new_id(),
        org_id: ctx.workspace.org_id.clone(),
        workspace_id: ctx.workspace.id.clone(),
        kind: kind.into(),
        requested_by: who.actor,
        on_behalf_of: who.on_behalf_of,
        target_id: target.id.clone(),
        state: "queued".into(),
        title: cap(title, 200),
        spec: spec.to_string(),
        result: String::new(),
        stdout: String::new(),
        stderr: String::new(),
        exit_code: -1,
        error: String::new(),
        cancel: false,
        lease_until: 0,
        created_ms: now_ms(),
        started_ms: 0,
        finished_ms: 0,
    }
}

async fn queue(app: &App, ctx: &Ctx<'_>, run: &Run, action: &str) -> Result<()> {
    let mut uow = Uow::begin(app).await?;
    let org_full = store::has_capacity(&mut uow.tx, &run.org_id, app.max_active_runs).await?
        == store::Capacity::OrganizationFull;
    let workspace_full =
        !org_full && !store::workspace_has_room(&mut uow.tx, &run.workspace_id).await?;
    if org_full || workspace_full {
        drop(uow);
        crate::governance::audit::service::record(
            app,
            &run.org_id,
            &Who::of(ctx.identity),
            action,
            &format!("workspace {}", run.workspace_id),
            "denied",
        )
        .await?;
        return Err(Error::TooManyRequests(
            if org_full {
                "this organization has reached its active run limit; try again when a run finishes"
            } else {
                "this workspace has reached its active run limit; try again when a run finishes"
            }
            .into(),
        ));
    }
    if let Some(task_id) = ctx.schedule_task_id {
        // The row lock serializes this run against pause/delete on other server instances.
        if orgs::is_erasing_in(
            &mut uow.tx,
            &run.org_id,
            &format!("user:{}", ctx.identity.subject),
        )
        .await?
        {
            return Err(Error::Conflict("the schedule owner is being erased".into()));
        }
        let active = sqlx::query(
            "UPDATE schedules SET last_error = last_error WHERE id = \
             (SELECT subject FROM tasks WHERE id = $1 AND kind = 'schedule.run') AND state = 'active'",
        )
        .bind(task_id)
        .execute(&mut *uow.tx)
        .await?;
        if active.rows_affected() != 1 {
            return Err(Error::Conflict("the schedule is no longer active".into()));
        }
    }
    store::insert_run(&mut *uow.tx, run).await?;
    if let Some(task_id) = ctx.schedule_task_id {
        sqlx::query("UPDATE runs SET schedule_task_id = $2 WHERE id = $1")
            .bind(&run.id)
            .bind(task_id)
            .execute(&mut *uow.tx)
            .await?;
    }
    uow.audit(
        &run.org_id,
        &Who::of(ctx.identity),
        action,
        &format!("run {}", run.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    // One wake for the serverless dispatcher, and one for every agent waiting for work.
    app.queued.notify_one();
    app.queued.notify_waiters();
    Ok(())
}

/// Reads a table the statement names, if the person may. Anything else, missing or not theirs to read, is the same answer.
async fn readable_table(
    app: &App,
    ctx: &Ctx<'_>,
    rel: &sql::Relation,
) -> Result<(String, String, String)> {
    let nope = || {
        Error::NotFound(format!(
            "table {} was not found, or you may not read it",
            rel.join(".")
        ))
    };
    let (entry, target) = catalogs::resolve(app, &ctx.workspace.id, &ctx.workspace.org_id, rel)
        .await
        .map_err(|_| nope())?;
    if entry.kind != Kind::Table {
        return Err(nope());
    }
    iam::require(
        app,
        ctx.identity,
        &ctx.workspace.org_id,
        ctx.sides,
        Privilege::Select,
        &target,
    )
    .await
    .map_err(|_| nope())?;
    if entry.classification == "sealed" {
        return Err(Error::Invalid(
            "sealed data cannot be used by compute".into(),
        ));
    }
    Ok((entry.id, rel.join("."), entry.classification))
}

/// Asks for a SQL statement to be run. Everything it reads is checked now, against the person's rights.
///
/// # Errors
/// Not allowed, the statement is refused, or no compute is answering.
pub async fn submit_sql(app: &App, ctx: &Ctx<'_>, req: SqlRequest) -> Result<Run> {
    entitled(app, ctx, "sql-access").await?;
    if req.sql.len() > 100_000 {
        return Err(Error::Invalid("the statement is too long".into()));
    }
    let analysis = sql::analyze(&req.sql).map_err(Error::Invalid)?;
    let mut tables = Vec::new();
    // Data classified above normal never leaves the organization's own machines.
    let mut private = false;
    for rel in &analysis.relations {
        let (id, name, class) = readable_table(app, ctx, rel).await?;
        private |= class != "normal";
        tables.push(json!({"id": id, "name": name}));
    }
    // A table with row or column rules is read through them: the statement itself is rewritten here, once, so every kind of
    // compute applies them.
    let mut rules = std::collections::HashMap::new();
    for (rel, t) in analysis.relations.iter().zip(&tables) {
        let id = t["id"].as_str().unwrap_or_default();
        let found =
            catalogs::rules_for(app, ctx.identity, &ctx.workspace.org_id, ctx.sides, id).await?;
        if let Some(r) = found {
            rules.insert(rel.clone(), r);
        }
    }
    let statement = sql::guard(&req.sql, &rules).map_err(Error::Invalid)?;
    let limit = req.limit.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS);
    let mut policies = Vec::new();
    for t in &tables {
        let id = t["id"].as_str().unwrap_or_default();
        policies.push(catalogs::compute_policy_of(app, &ctx.workspace.org_id, id).await?);
    }
    let target = pick(
        app,
        &ctx.workspace.id,
        req.target.as_deref(),
        None,
        None,
        private,
        &policies,
    )
    .await?;
    let limits = limits_for(&target, None, None, None, 120).map_err(Error::Invalid)?;
    let spec = json!({"as": As::of(ctx.identity), "source_sql": req.sql, "sql": statement, "limit": limit, "tables": tables, "memory_mb": limits.memory_mb, "cpu_millis": limits.cpu_millis, "timeout_s": limits.timeout_s});
    let run = new_run(ctx, "sql", &req.sql, &target, &spec);
    queue(app, ctx, &run, "sql.submit").await?;
    Ok(run)
}

/// Adds what a job's declared table writes give its token: the table itself, the catalog and schema above it to look at (`see:`), and
/// for a table it will create, the schema to create it in (`create:`). Used when the token is made and when the lease is rechecked, so
/// the two can never differ.
fn add_write_scope(spec: &Value, reads: &mut Vec<String>, writes: &mut Vec<String>) {
    for w in spec["table_writes"].as_array().into_iter().flatten() {
        for key in ["catalog_id", "schema_id"] {
            if let Some(id) = w[key].as_str() {
                reads.push(format!("see:{id}"));
            }
        }
        match (w["id"].as_str(), w["schema_id"].as_str()) {
            (Some(id), _) => {
                reads.push(id.to_owned());
                writes.push(id.to_owned());
            }
            (None, Some(schema)) => writes.push(format!("create:{schema}")),
            _ => {}
        }
    }
}

/// The tables a job asks to write, checked now against the person's rights: each as `{name, schema_id, catalog_id, id?}`, where `id` is
/// there when the table exists (and needs `MODIFY`) and absent when the job will create it (needs `CREATE TABLE` on the schema).
async fn checked_table_writes(app: &App, ctx: &Ctx<'_>, names: &[String]) -> Result<Vec<Value>> {
    if names.len() > 10 {
        return Err(Error::Invalid("a job writes at most 10 tables".into()));
    }
    let org = &ctx.workspace.org_id;
    let mut out = Vec::new();
    for full in names {
        let parts: Vec<String> = full.split('.').map(str::to_owned).collect();
        let [catalog, schema, table] = <[String; 3]>::try_from(parts)
            .map_err(|_| Error::Invalid("name a table to write as catalog.schema.table".into()))?;
        let gone = || {
            Error::NotFound(format!(
                "{catalog}.{schema} was not found, or you may not use it"
            ))
        };
        let (schema_entry, schema_target) = catalogs::resolve(
            app,
            &ctx.workspace.id,
            org,
            &[catalog.clone(), schema.clone()],
        )
        .await
        .map_err(|_| gone())?;
        if schema_entry.kind != Kind::Schema {
            return Err(gone());
        }
        let existing = catalogs::resolve(
            app,
            &ctx.workspace.id,
            org,
            &[catalog.clone(), schema.clone(), table.clone()],
        )
        .await;
        let mut item = json!({
            "name": full, "schema_id": schema_entry.id, "catalog_id": schema_entry.parent_id,
        });
        if let Ok((entry, target)) = existing {
            if entry.kind != Kind::Table {
                return Err(Error::Invalid(format!("{full} is not a table")));
            }
            iam::require(
                app,
                ctx.identity,
                org,
                ctx.sides,
                Privilege::Modify,
                &target,
            )
            .await?;
            // Rules on a table are applied by SQL only; a job writing it could read or replace its raw files.
            if catalogs::is_restricted(app, ctx.identity, org, ctx.sides, &entry.id).await? {
                return Err(Error::Invalid(format!(
                    "{full} has row or column rules, so it cannot be written by a job"
                )));
            }
            if entry.classification == "sealed" {
                return Err(Error::Invalid("compute cannot write to sealed data".into()));
            }
            item["id"] = json!(entry.id);
        } else {
            iam::require(
                app,
                ctx.identity,
                org,
                ctx.sides,
                Privilege::CreateTable,
                &schema_target,
            )
            .await?;
        }
        out.push(item);
    }
    Ok(out)
}

/// What a person asks for to bring a file in as a table. The file is a volume's file (`volume` and `path`), or an address that the
/// machine downloads (`url`): exactly one of the two.
#[derive(Deserialize)]
pub struct ImportRequest {
    /// The table to make or add to, as `catalog.schema.table`.
    pub target: String,
    /// The volume that holds the file, and the file's path inside it.
    #[serde(default)]
    pub volume: String,
    #[serde(default)]
    pub path: String,
    /// An `https://` address to download instead. A signed address (for a file in a bucket) works: it is never shown back.
    #[serde(default)]
    pub url: String,
    /// `csv` or `parquet`; left out, it is read from the file's extension.
    #[serde(default)]
    pub format: String,
    /// `create` (the default: a new table), `append` (add rows to a table that exists) or `replace` (swap a table's rows for the
    /// file's: the table keeps its columns).
    #[serde(default)]
    pub mode: String,
    pub target_compute: Option<String>,
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// The program a file import runs, with its few values filled in: it is ours, never the person's.
const IMPORT_JOB: &str = include_str!("import_job.py");

/// A volume file's path, checked.
fn checked_file_path(raw: &str) -> Result<&str> {
    let path = raw.trim_start_matches('/');
    if path.is_empty()
        || path.len() > 1024
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || path.contains(['\\', '\0'])
    {
        return Err(Error::Invalid("the file's path is not valid".into()));
    }
    Ok(path)
}

/// Where an import's file comes from, as far as the server can check; the machine that downloads it checks the rest.
enum Source<'a> {
    Volume(&'a str),
    Address(reqwest::Url),
}

impl<'a> Source<'a> {
    fn of(req: &'a ImportRequest) -> Result<Self> {
        match (req.volume.is_empty() && req.path.is_empty(), req.url.is_empty()) {
            (false, true) if !req.volume.is_empty() && !req.path.is_empty() => {
                Ok(Self::Volume(checked_file_path(&req.path)?))
            }
            (true, false) => {
                let url = reqwest::Url::parse(&req.url)
                    .map_err(|_| Error::Invalid("the address is not valid".into()))?;
                if req.url.len() > 4096
                    || url.scheme() != "https"
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.host_str().is_none()
                    || url.port().is_some_and(|p| p != 443)
                {
                    return Err(Error::Invalid(
                        "the address must be an https:// address with a host, and no user name or password".into(),
                    ));
                }
                Ok(Self::Address(url))
            }
            _ => Err(Error::Invalid(
                "name the file by a volume and path, or by a url, not both".into(),
            )),
        }
    }

    /// What to read the file's extension from: never the query (it may be a signature).
    fn name(&self) -> String {
        match self {
            Self::Volume(path) => (*path).to_owned(),
            Self::Address(url) => format!("{}{}", url.host_str().unwrap_or_default(), url.path()),
        }
    }

    /// What titles say about the file: a volume's path, or only the host of an address (its path may carry a secret too).
    fn label(&self) -> String {
        match self {
            Self::Volume(path) => (*path).to_owned(),
            Self::Address(url) => url.host_str().unwrap_or_default().to_owned(),
        }
    }
}

/// Asks for a file to be brought in as a table. It is an ordinary job with a table to write and, for a volume file, one volume to
/// read, run by a program the server wrote: the person needs `READ` on the volume and `CREATE TABLE` (or `MODIFY` to add to or
/// replace one) on the target. An address is downloaded by the machine before the sandbox starts, which has no network.
///
/// # Errors
/// Not allowed, the request is not valid, or no compute is answering.
pub async fn submit_import(app: &App, ctx: &Ctx<'_>, req: ImportRequest) -> Result<Run> {
    let source = Source::of(&req)?;
    let name = source.name();
    let format = if req.format.is_empty() {
        name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase()
    } else {
        req.format.to_ascii_lowercase()
    };
    if !matches!(format.as_str(), "csv" | "parquet") {
        return Err(Error::Invalid(
            "a file is imported as csv or parquet".into(),
        ));
    }
    let mode = if req.mode.is_empty() {
        "create"
    } else {
        req.mode.as_str()
    };
    if !matches!(mode, "create" | "append" | "replace") {
        return Err(Error::Invalid("mode is create, append or replace".into()));
    }
    let parts: Vec<&str> = req.target.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| !crate::governance::catalogs::model::valid_name(p))
    {
        return Err(Error::Invalid(
            "name the table as catalog.schema.table, in lower case with digits and underscores"
                .into(),
        ));
    }
    let spec = json!({
        "target": req.target, "format": format, "mode": mode, "workspace": ctx.workspace.id,
        "source": match &source { Source::Volume(path) => json!({"path": path}), Source::Address(_) => json!({"fetched": true}) },
    });
    let script = IMPORT_JOB.replace("__SPEC__", &hex_encode(spec.to_string().as_bytes()));
    let job = JobRequest {
        title: format!("Import {} into {}", source.label(), req.target),
        script,
        inputs: match &source {
            Source::Volume(_) => vec![req.volume.clone()],
            Source::Address(_) => vec![],
        },
        output: String::new(),
        table_writes: vec![req.target.clone()],
        fetch: match &source {
            Source::Volume(_) => None,
            Source::Address(url) => Some(url.to_string()),
        },
        target: req.target_compute,
        memory_mb: None,
        cpu_millis: None,
        timeout_s: Some(600),
    };
    // Add and replace need the table to exist, and a create needs it not to: the person's mistake is better said now than by a
    // failed run.
    let exists = catalogs::resolve(
        app,
        &ctx.workspace.id,
        &ctx.workspace.org_id,
        &parts.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>(),
    )
    .await
    .is_ok();
    if mode == "create" && exists {
        return Err(Error::Conflict(format!(
            "{} already exists: use mode append to add to it or replace to swap its rows",
            req.target
        )));
    }
    if mode != "create" && !exists {
        return Err(Error::NotFound(format!("{} was not found", req.target)));
    }
    submit_job(app, ctx, job).await
}

/// Asks for a job (a Python script) to be run.
///
/// # Errors
/// Not allowed, not valid, or no compute is answering.
#[allow(clippy::too_many_lines)]
pub async fn submit_job(app: &App, ctx: &Ctx<'_>, req: JobRequest) -> Result<Run> {
    entitled(app, ctx, "compute-create").await?;
    if req.script.trim().is_empty() || req.script.len() > 1_000_000 {
        return Err(Error::Invalid("the script is empty or too long".into()));
    }
    if req.inputs.len() > 20 {
        return Err(Error::Invalid("a job reads at most 20 inputs".into()));
    }
    let org = &ctx.workspace.org_id;
    let mut inputs = Vec::new();
    let mut reads = Vec::new();
    let mut names = Vec::new();
    let mut has_personal = false;
    for id in &req.inputs {
        let (entry, target) = catalogs::target_in(app, &ctx.workspace.id, org, id).await?;
        let need = match entry.kind {
            Kind::Table => Privilege::Select,
            Kind::Volume => Privilege::ReadVolume,
            _ => {
                return Err(Error::Invalid(format!(
                    "{} is not a table or a volume",
                    entry.name
                )));
            }
        };
        iam::require(app, ctx.identity, org, ctx.sides, need, &target).await?;
        if entry.kind == Kind::Table
            && catalogs::is_restricted(app, ctx.identity, org, ctx.sides, &entry.id).await?
        {
            return Err(Error::Invalid(format!(
                "{} has row or column rules, so it can only be read with SQL, not in a job",
                entry.name
            )));
        }
        if entry.classification == "sealed" {
            return Err(Error::Invalid(
                "sealed data cannot be used by compute".into(),
            ));
        }
        has_personal |= entry.classification == "personal";
        names.push(entry.name.clone());
        inputs.push((entry.id.clone(), entry.kind));
        reads.push(entry.id);
    }
    let mounts = mount_names(&names);
    let inputs: Vec<Value> = inputs
        .iter()
        .zip(&mounts)
        .map(|((id, kind), name)| json!({"id": id, "kind": kind.as_str(), "name": name}))
        .collect();
    let mut output_id = String::new();
    if !req.output.is_empty() {
        let (out, out_target) =
            catalogs::target_in(app, &ctx.workspace.id, org, &req.output).await?;
        if out.kind != Kind::Volume {
            return Err(Error::Invalid("the output must be a volume".into()));
        }
        iam::require(
            app,
            ctx.identity,
            org,
            ctx.sides,
            Privilege::WriteVolume,
            &out_target,
        )
        .await?;
        if out.classification == "sealed" {
            return Err(Error::Invalid("compute cannot write to sealed data".into()));
        }
        if has_personal && out.classification != "personal" {
            return Err(Error::Invalid(
                "personal inputs require a personal output volume".into(),
            ));
        }
        output_id = out.id;
    }
    let table_writes = checked_table_writes(app, ctx, &req.table_writes).await?;
    if req.output.is_empty() && table_writes.is_empty() {
        return Err(Error::Invalid(
            "a job needs somewhere to leave its results: an output volume or a table to write"
                .into(),
        ));
    }
    let mut policies = Vec::new();
    for id in &req.inputs {
        policies.push(catalogs::compute_policy_of(app, org, id).await?);
    }
    for w in &table_writes {
        let id = w["schema_id"].as_str().unwrap_or_default();
        policies.push(catalogs::compute_policy_of(app, org, id).await?);
    }
    let target = pick(
        app,
        &ctx.workspace.id,
        req.target.as_deref(),
        req.memory_mb,
        req.cpu_millis,
        // A job runs code, and only SQL goes to a provider.
        true,
        &policies,
    )
    .await?;
    let limits = limits_for(&target, req.memory_mb, req.cpu_millis, req.timeout_s, 600)
        .map_err(Error::Invalid)?;
    let title = job_title(&req.title, &req.script);
    let mut spec = json!({"as": As::of(ctx.identity), "script": req.script, "inputs": inputs, "output": output_id, "table_writes": table_writes, "memory_mb": limits.memory_mb, "cpu_millis": limits.cpu_millis, "timeout_s": limits.timeout_s});
    if let Some(url) = &req.fetch {
        spec["fetch"] = json!({"url": url});
    }
    let run = new_run(ctx, "job", &title, &target, &spec);
    queue(app, ctx, &run, "job.submit").await?;
    Ok(run)
}

async fn refresh_groups(app: &App, who: &mut As) -> Result<()> {
    if who.service {
        return Ok(());
    }
    if let Some(directory) = &app.directory {
        who.groups = directory.groups(&who.subject).await?;
    } else if !who.groups.is_empty() {
        return Err(Error::Unavailable(
            "current group membership cannot be checked".into(),
        ));
    }
    Ok(())
}

/// A run, if the person may see it: their own, or any in a workspace they administer.
///
/// # Errors
/// `NotFound`.
pub async fn get(app: &App, ctx: &Ctx<'_>, id: &str) -> Result<Run> {
    let run = store::run_get(app.db.pool(), &ctx.workspace.id, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such run".into()))?;
    if ctx.standing.is_admin()
        || run.requested_by == ctx.identity.subject
        || run.on_behalf_of == ctx.identity.subject
    {
        Ok(run)
    } else {
        Err(Error::NotFound("no such run".into()))
    }
}

/// A run can contain rows and output from its inputs. Recheck the viewer's access before exposing its details.
/// Status and cancellation are available separately so a revoked user can still stop their own run.
pub async fn require_detail(app: &App, ctx: &Ctx<'_>, run: &Run) -> Result<()> {
    let hidden = || Error::NotFound("no such run".into());
    let spec: Value = serde_json::from_str(&run.spec).map_err(|_| hidden())?;
    let (items, entitlement) = match run.kind.as_str() {
        "sql" => (spec["tables"].as_array().ok_or_else(hidden)?, "sql-access"),
        "job" => (
            spec["inputs"].as_array().ok_or_else(hidden)?,
            "compute-create",
        ),
        _ => return Err(hidden()),
    };
    let mut viewer = ctx.identity.clone();
    if viewer.kind == WhoKind::Person {
        let Some(directory) = &app.directory else {
            if !viewer.groups.is_empty() {
                return Err(hidden());
            }
            return require_detail_with_identity(app, &viewer, run, &spec, items, entitlement)
                .await;
        };
        viewer.groups = directory
            .groups(&viewer.subject)
            .await
            .map_err(|_| hidden())?;
    }
    require_detail_with_identity(app, &viewer, run, &spec, items, entitlement).await
}

async fn require_detail_with_identity(
    app: &App,
    identity: &Identity,
    run: &Run,
    spec: &Value,
    items: &[Value],
    entitlement: &str,
) -> Result<()> {
    let hidden = || Error::NotFound("no such run".into());
    let (workspace, standing) = workspaces::open(app, identity, &run.workspace_id)
        .await
        .map_err(|_| hidden())?;
    if workspace.org_id != run.org_id
        || !workspaces::entitled(app, identity, &workspace.id, standing, entitlement)
            .await
            .map_err(|_| hidden())?
    {
        return Err(hidden());
    }
    let sides = orgs::sides(app, identity, &run.org_id)
        .await
        .map_err(|_| hidden())?;
    for item in items {
        let id = item["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(hidden)?;
        let (entry, target) = catalogs::target_in(app, &workspace.id, &run.org_id, id)
            .await
            .map_err(|_| hidden())?;
        let need = match (run.kind.as_str(), entry.kind) {
            ("sql", Kind::Table) => Privilege::Select,
            ("job", Kind::Table) if item["kind"] == "table" => Privilege::Select,
            ("job", Kind::Volume) if item["kind"] == "volume" => Privilege::ReadVolume,
            _ => return Err(hidden()),
        };
        if entry.classification == "sealed" {
            return Err(hidden());
        }
        iam::require(app, identity, &run.org_id, &sides, need, &target)
            .await
            .map_err(|_| hidden())?;
    }

    let mut who: As = serde_json::from_value(spec["as"].clone()).map_err(|_| hidden())?;
    refresh_groups(app, &mut who).await.map_err(|_| hidden())?;
    let reads = items
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    let writes = if run.kind == "job" {
        vec![spec["output"].as_str().ok_or_else(hidden)?.to_owned()]
    } else {
        vec![]
    };
    let subject = who.identity(
        &run.org_id,
        Scope {
            run: run.id.clone(),
            reads,
            writes,
        },
    );
    if run.kind == "sql" {
        let current_sql = guarded_sql(app, &subject, run, spec)
            .await
            .map_err(|_| hidden())?;
        if spec["sql"].as_str() != Some(&current_sql) {
            return Err(hidden());
        }
    } else {
        let mut policy_identity = subject.clone();
        policy_identity.scope = None;
        let sides = orgs::sides(app, &subject, &run.org_id)
            .await
            .map_err(|_| hidden())?;
        for item in items {
            if item["kind"] == "table" {
                let id = item["id"].as_str().ok_or_else(hidden)?;
                if catalogs::is_restricted(app, &policy_identity, &run.org_id, &sides, id)
                    .await
                    .map_err(|_| hidden())?
                {
                    return Err(hidden());
                }
            }
        }
    }
    Ok(())
}

/// The workspace's runs, newest first: the person's own, or everyone's for an admin (who may narrow it to one person).
///
/// # Errors
/// The database could not be read.
pub async fn list(app: &App, ctx: &Ctx<'_>, mut filter: RunFilter, limit: i64) -> Result<Vec<Run>> {
    if !ctx.standing.is_admin() {
        filter.by = Some(ctx.identity.subject.clone());
    }
    store::runs_list(
        app.db.pool(),
        &ctx.workspace.id,
        &filter,
        limit.clamp(1, 200),
    )
    .await
}

/// Waits for a run to end, up to `wait`, and returns it as it is then.
///
/// # Errors
/// The database could not be read.
pub async fn wait(app: &App, ctx: &Ctx<'_>, run: Run, wait: Duration) -> Result<Run> {
    let deadline = tokio::time::Instant::now() + wait;
    let mut run = run;
    while !is_terminal(&run.state) && tokio::time::Instant::now() < deadline {
        // Ask to be woken before looking, so an end between the look and the wait is not missed.
        let ended = app.finished.notified();
        tokio::pin!(ended);
        ended.as_mut().enable();
        run = get(app, ctx, &run.id).await?;
        if is_terminal(&run.state) {
            break;
        }
        // Woken the moment any run ends; the short sleep is the safety net for an end nobody announced (a sweep).
        tokio::select! {
            () = &mut ended => {}
            () = tokio::time::sleep(Duration::from_millis(150)) => {}
        }
    }
    Ok(run)
}

/// Stops a run: at once if it has not started, otherwise when its agent next checks in.
///
/// # Errors
/// Not the person's run (or an admin), or it has already ended.
pub async fn cancel(app: &App, ctx: &Ctx<'_>, id: &str) -> Result<Run> {
    let run = get(app, ctx, id).await?;
    if is_terminal(&run.state) {
        return Err(Error::Conflict("this run has already ended".into()));
    }
    let mut uow = Uow::begin(app).await?;
    if run.state == "queued" {
        store::fail(
            &mut *uow.tx,
            id,
            "queued",
            "cancelled",
            "cancelled before it started",
            now_ms(),
        )
        .await?;
    } else {
        store::request_cancel(&mut *uow.tx, id).await?;
    }
    uow.audit(
        &run.org_id,
        &Who::of(ctx.identity),
        "run.cancel",
        &format!("run {id}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    app.finished.notify_waiters();
    get(app, ctx, id).await
}

// ---- the agent's ------------------------------------------------------------------------------------------------

/// The target an agent's secret belongs to, noting that it was heard from.
///
/// # Errors
/// The secret is not one we know.
pub async fn authenticate_agent(app: &App, secret: &str) -> Result<Target> {
    if !secret.starts_with("agt_") {
        return Err(Error::Unauthorized);
    }
    let target = store::target_by_secret(app.db.pool(), &hash_secret(secret))
        .await?
        .ok_or(Error::Unauthorized)?;
    let now = now_ms();
    store::touch(app.db.pool(), &target.id, now).await?;
    Ok(Target {
        last_seen_ms: now,
        ..target
    })
}

/// What an agent is handed to run.
pub struct Lease {
    pub run: Run,
    pub token: String,
    pub limits: Limits,
    /// Where and how to read the run's tables: short-lived read-only credentials for exactly their folders. None if it reads none.
    pub storage: Option<Value>,
}

/// The tables a run reads, by id.
fn table_ids(run: &Run, spec: &Value) -> Vec<String> {
    let field = |v: &Value| v["id"].as_str().map(str::to_owned);
    if run.kind == "sql" {
        spec["tables"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(field)
            .collect()
    } else {
        spec["inputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|i| i["kind"] == "table")
            .filter_map(field)
            .collect()
    }
}

async fn guarded_sql(app: &App, identity: &Identity, run: &Run, spec: &Value) -> Result<String> {
    let invalid = || Error::Internal("invalid queued SQL specification".into());
    let source = spec["source_sql"].as_str().ok_or_else(invalid)?;
    let tables = spec["tables"].as_array().ok_or_else(invalid)?;
    let analysis = sql::analyze(source).map_err(Error::Internal)?;
    if analysis.relations.len() != tables.len() {
        return Err(invalid());
    }
    let mut policy_identity = identity.clone();
    policy_identity.scope = None;
    let sides = orgs::sides(app, &policy_identity, &run.org_id).await?;
    let mut rules = std::collections::HashMap::new();
    for (rel, table) in analysis.relations.iter().zip(tables) {
        let id = table["id"].as_str().ok_or_else(invalid)?;
        if table["name"].as_str() != Some(&rel.join(".")) {
            return Err(invalid());
        }
        if let Some(found) =
            catalogs::rules_for(app, &policy_identity, &run.org_id, &sides, id).await?
        {
            rules.insert(rel.clone(), found);
        }
    }
    sql::guard(source, &rules).map_err(Error::Internal)
}

/// What a machine needs to read some tables itself: credentials limited to those tables' folders and to reading, good for the
/// run's time and a little more, and where each table's current metadata is.
async fn storage_for(
    app: &App,
    org: &str,
    ids: &[String],
    spec: &Value,
    timeout_s: i64,
) -> Result<Value> {
    let ttl = Duration::from_secs(u64::try_from(timeout_s).unwrap_or(120) + 300);
    let mut located = serde_json::Map::new();
    let mut folders = Vec::with_capacity(ids.len());
    for id in ids {
        let (uri, rel, folder) = tables::locate(app, org, id).await?;
        located.insert(
            id.clone(),
            json!({"uri": uri, "metadata": rel, "folder": folder}),
        );
        folders.push(folder);
    }
    let read_prefixes: Vec<String> = folders.iter().map(|f| format!("{f}/")).collect();
    // A job that writes tables is also given the folders it may write: an existing table's own, and for a table it will create, its
    // catalog's root (the new table's folder is not known yet; the storage door narrows it to the folder the catalog gives).
    let mut write_prefixes: Vec<String> = Vec::new();
    let mut create_roots: Vec<String> = Vec::new();
    for w in spec["table_writes"].as_array().into_iter().flatten() {
        if let Some(id) = w["id"].as_str() {
            let (uri, rel, folder) = tables::locate(app, org, id).await?;
            located.insert(
                id.to_owned(),
                json!({"uri": uri, "metadata": rel, "folder": folder}),
            );
            write_prefixes.push(format!("{folder}/"));
            folders.push(folder);
        } else {
            let catalog = w["catalog_id"]
                .as_str()
                .ok_or_else(|| Error::Internal("a write has no catalog".into()))?;
            let (root, _) = catalogs::target(app, org, catalog).await?;
            create_roots.push(format!("{}/", root.folder));
            folders.push(root.folder);
        }
    }
    let writing = !write_prefixes.is_empty() || !create_roots.is_empty();
    let refs: Vec<&str> = folders.iter().map(String::as_str).collect();
    let creds = tables::vend(app, &refs, writing, ttl).await?;
    let (endpoint, bucket) = app
        .stores
        .bucket_place()
        .ok_or_else(|| Error::Unavailable("this server's storage is not a bucket".into()))?;
    Ok(json!({
        "endpoint": endpoint, "bucket": bucket, "key_id": creds.key_id, "secret": creds.secret,
        "session_token": creds.session_token, "tables": located,
        "mode": if writing { "write" } else { "read" },
        "read_prefixes": read_prefixes, "write_prefixes": write_prefixes, "create_roots": create_roots,
    }))
}

/// Check the current workspace, entitlements, object grants and table rules before handing a run to a worker.
#[allow(clippy::too_many_lines)]
async fn authorize_lease(app: &App, target: &Target, run: &Run, spec: &Value) -> Result<()> {
    let invalid = || Error::Internal("invalid queued run specification".into());
    let (items, entitlement) = match run.kind.as_str() {
        "sql" => (spec["tables"].as_array().ok_or_else(invalid)?, "sql-access"),
        "job" => (
            spec["inputs"].as_array().ok_or_else(invalid)?,
            "compute-create",
        ),
        _ => return Err(invalid()),
    };
    let mut reads = Vec::with_capacity(items.len());
    for item in items {
        reads.push(
            item["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(invalid)?
                .to_owned(),
        );
    }
    let mut writes = if run.kind == "job" {
        spec["output"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(|id| vec![id.to_owned()])
            .unwrap_or_default()
    } else {
        vec![]
    };
    if run.kind == "job" {
        add_write_scope(spec, &mut reads, &mut writes);
    }
    let who: As = serde_json::from_value(spec["as"].clone()).map_err(|_| invalid())?;
    let identity = who.identity(
        &run.org_id,
        Scope {
            run: run.id.clone(),
            reads,
            writes,
        },
    );
    let (workspace, standing) = workspaces::open(app, &identity, &run.workspace_id).await?;
    if workspace.org_id != run.org_id
        || !workspaces::entitled(app, &identity, &workspace.id, standing, entitlement).await?
    {
        return Err(Error::Unauthorized);
    }
    let sides = orgs::sides(app, &identity, &run.org_id).await?;
    let mut has_personal = false;
    for item in items {
        let id = item["id"].as_str().ok_or_else(invalid)?;
        let (entry, securable) =
            catalogs::target_in(app, &run.workspace_id, &run.org_id, id).await?;
        let need = match (run.kind.as_str(), entry.kind) {
            ("sql", Kind::Table) => Privilege::Select,
            ("job", Kind::Table) if item["kind"] == "table" => Privilege::Select,
            ("job", Kind::Volume) if item["kind"] == "volume" => Privilege::ReadVolume,
            _ => return Err(invalid()),
        };
        iam::require(app, &identity, &run.org_id, &sides, need, &securable).await?;
        if run.kind == "job" && entry.kind == Kind::Table && {
            let mut policy_identity = identity.clone();
            policy_identity.scope = None;
            catalogs::is_restricted(app, &policy_identity, &run.org_id, &sides, &entry.id).await?
        } {
            return Err(Error::Unauthorized);
        }
        // The catalog's own say over which compute may read it holds when the work is handed out, not only when it was asked for.
        let policy = catalogs::compute_policy_of(app, &run.org_id, &entry.id).await?;
        if !policy.is_empty() && !policy.contains(&target.id) {
            return Err(Error::Unauthorized);
        }
        if entry.classification == "sealed"
            || (target.serverless() && entry.classification != "normal")
        {
            return Err(Error::Unauthorized);
        }
        has_personal |= entry.classification == "personal";
    }
    if run.kind == "job" {
        for w in spec["table_writes"].as_array().into_iter().flatten() {
            let schema = w["schema_id"].as_str().ok_or_else(invalid)?;
            if let Some(id) = w["id"].as_str() {
                let (entry, securable) =
                    catalogs::target_in(app, &run.workspace_id, &run.org_id, id).await?;
                iam::require(
                    app,
                    &identity,
                    &run.org_id,
                    &sides,
                    Privilege::Modify,
                    &securable,
                )
                .await?;
                let mut policy_identity = identity.clone();
                policy_identity.scope = None;
                if entry.kind != Kind::Table
                    || catalogs::is_restricted(
                        app,
                        &policy_identity,
                        &run.org_id,
                        &sides,
                        &entry.id,
                    )
                    .await?
                {
                    return Err(Error::Unauthorized);
                }
            } else {
                let (_, securable) =
                    catalogs::target_in(app, &run.workspace_id, &run.org_id, schema).await?;
                iam::require(
                    app,
                    &identity,
                    &run.org_id,
                    &sides,
                    Privilege::CreateTable,
                    &securable,
                )
                .await?;
            }
        }
    }
    if run.kind == "job" && spec["output"].as_str().is_some_and(|o| !o.is_empty()) {
        let id = spec["output"].as_str().ok_or_else(invalid)?;
        let (entry, securable) =
            catalogs::target_in(app, &run.workspace_id, &run.org_id, id).await?;
        if entry.kind != Kind::Volume
            || entry.classification == "sealed"
            || (has_personal && entry.classification != "personal")
        {
            return Err(Error::Unauthorized);
        }
        iam::require(
            app,
            &identity,
            &run.org_id,
            &sides,
            Privilege::WriteVolume,
            &securable,
        )
        .await?;
    }
    Ok(())
}

async fn fail_unauthorized_lease(app: &App, target: &Target, run: &Run) -> Result<()> {
    complete(
        app,
        target,
        &run.id,
        Report {
            state: "failed".into(),
            exit_code: -1,
            stdout: String::new(),
            stderr: String::new(),
            result: String::new(),
            error: "the run could not be authorized when it started".into(),
        },
    )
    .await
}

/// Hands the target its next queued run, with the token that run's files are read and written with. None if nothing is waiting.
///
/// # Errors
/// The database could not be reached.
#[allow(clippy::too_many_lines)]
pub async fn lease(app: &App, target: &Target) -> Result<Option<Lease>> {
    let now = now_ms();
    let leased = {
        let mut conn = app.db.pool().acquire().await?;
        store::lease_one(&mut conn, &target.id, now, now + LEASE_MS).await?
    };
    let Some(mut run) = leased else {
        return Ok(None);
    };
    let mut spec: Value = serde_json::from_str(&run.spec)
        .map_err(|e| Error::Internal(format!("a run has a spec that is not JSON: {e}")))?;
    let refresh = async {
        let mut who: As = serde_json::from_value(spec["as"].clone())
            .map_err(|_| Error::Internal("invalid queued run specification".into()))?;
        refresh_groups(app, &mut who).await?;
        let items = match run.kind.as_str() {
            "sql" => spec["tables"].as_array(),
            "job" => spec["inputs"].as_array(),
            _ => None,
        }
        .ok_or_else(|| Error::Internal("invalid queued run specification".into()))?;
        let reads = items
            .iter()
            .map(|item| {
                item["id"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::Internal("invalid queued run specification".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let writes = if run.kind == "job" {
            vec![
                spec["output"]
                    .as_str()
                    .ok_or_else(|| Error::Internal("invalid queued run specification".into()))?
                    .to_owned(),
            ]
        } else {
            vec![]
        };
        spec["as"] = serde_json::to_value(&who)
            .map_err(|_| Error::Internal("invalid queued run specification".into()))?;
        let identity = who.identity(
            &run.org_id,
            Scope {
                run: run.id.clone(),
                reads,
                writes,
            },
        );
        if run.kind == "sql" {
            let statement = guarded_sql(app, &identity, &run, &spec).await?;
            spec["sql"] = json!(statement);
        }
        Ok::<(), Error>(())
    }
    .await;
    if let Err(error) = refresh {
        tracing::warn!(run = %run.id, error = ?error, "queued work could not refresh its identity");
        fail_unauthorized_lease(app, target, &run).await?;
        return Ok(None);
    }
    if let Err(error) = authorize_lease(app, target, &run, &spec).await {
        tracing::warn!(run = %run.id, error = ?error, "queued work could not be authorized");
        fail_unauthorized_lease(app, target, &run).await?;
        return Ok(None);
    }
    store::update_leased_spec(app.db.pool(), &run.id, &target.id, &spec.to_string()).await?;
    let mut worker_spec = spec.clone();
    if let Some(fields) = worker_spec.as_object_mut() {
        fields.remove("source_sql");
    }
    run.spec = worker_spec.to_string();
    let limits = Limits {
        memory_mb: spec["memory_mb"].as_i64().unwrap_or(1024),
        cpu_millis: spec["cpu_millis"].as_i64().unwrap_or(1000),
        timeout_s: spec["timeout_s"].as_i64().unwrap_or(120),
    };
    let strings = |v: &Value| {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x["id"].as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let (mut reads, mut writes) = if run.kind == "sql" {
        (strings(&spec["tables"]), vec![])
    } else {
        let output = spec["output"].as_str().unwrap_or_default();
        (
            strings(&spec["inputs"]),
            if output.is_empty() {
                vec![]
            } else {
                vec![output.to_owned()]
            },
        )
    };
    // The tables a job declared it writes (see `add_write_scope`).
    add_write_scope(&spec, &mut reads, &mut writes);
    // A statement reads its tables with credentials, not with a token; only a job's files go through the API.
    let token = if run.kind == "sql" {
        String::new()
    } else {
        let token = crate::kernel::new_secret("jt_");
        store::insert_token(
            app.db.pool(),
            &hash_secret(&token),
            &store::TokenRow {
                run_id: run.id.clone(),
                org_id: run.org_id.clone(),
                workspace_id: run.workspace_id.clone(),
                identity: spec["as"].to_string(),
                reads: json!(reads).to_string(),
                writes: json!(writes).to_string(),
                // It outlives the run's own time limit a little, and is deleted when the run ends.
                expires_ms: now + (limits.timeout_s + 300) * 1000,
            },
        )
        .await?;
        token
    };
    let ids = table_ids(&run, &spec);
    // A server whose storage is a folder cannot give any machine access to data: the machine says so when it needs it.
    let writing = spec["table_writes"]
        .as_array()
        .is_some_and(|a| !a.is_empty());
    let storage = if (ids.is_empty() && !writing) || app.issuer.is_none() {
        None
    } else {
        // The credentials come from the storage provider over the network: one more try before giving up on a slow answer.
        let mut got = storage_for(app, &run.org_id, &ids, &spec, limits.timeout_s).await;
        if got.is_err() {
            got = storage_for(app, &run.org_id, &ids, &spec, limits.timeout_s).await;
        }
        match got {
            Ok(s) => Some(s),
            Err(e) => {
                // The run cannot be given its data; say so now, and why, rather than let it time out.
                tracing::warn!(run = %run.id, error = ?e, "could not prepare a run's data access");
                let why = match e {
                    Error::Internal(_) | Error::Unavailable(_) => {
                        "the storage service did not answer in time, so this statement's data could not be prepared: try again"
                            .to_owned()
                    }
                    other => other.to_string(),
                };
                let report = Report {
                    state: "failed".into(),
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: String::new(),
                    result: String::new(),
                    error: why,
                };
                complete(app, target, &run.id, report).await?;
                return Ok(None);
            }
        }
    };
    Ok(Some(Lease {
        run,
        token,
        limits,
        storage,
    }))
}

/// Renews a run's lease. Returns whether the agent should stop, because the run was cancelled; None if this is not its run any more.
///
/// # Errors
/// The database could not be reached.
pub async fn heartbeat(app: &App, target: &Target, id: &str) -> Result<Option<bool>> {
    store::heartbeat(app.db.pool(), id, &target.id, now_ms() + LEASE_MS).await
}

/// How an agent says a run ended.
#[derive(Deserialize)]
pub struct Report {
    pub state: String,
    #[serde(default)]
    pub exit_code: i64,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    /// A statement's rows, as JSON text.
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub error: String,
}

/// # Errors
/// Not a valid report, or this is not the target's running run.
pub async fn complete(app: &App, target: &Target, id: &str, report: Report) -> Result<()> {
    if !is_final_report(&report.state) {
        return Err(Error::Invalid(
            "state must be succeeded, failed, cancelled or timed_out".into(),
        ));
    }
    if report.result.len() > MAX_RESULT {
        return Err(Error::Invalid("the result is too large".into()));
    }
    let run = store::run_by_id(app.db.pool(), id)
        .await?
        .filter(|r| r.target_id == target.id)
        .ok_or_else(|| Error::NotFound("no such run".into()))?;
    let (stdout, stderr, error) = (
        cap(&report.stdout, MAX_OUTPUT),
        cap(&report.stderr, MAX_OUTPUT),
        cap(&report.error, 4000),
    );
    let mut uow = Uow::begin(app).await?;
    let outcome = store::Outcome {
        state: &report.state,
        exit_code: report.exit_code,
        stdout: &stdout,
        stderr: &stderr,
        result: &report.result,
        error: &error,
    };
    if !store::finish(&mut *uow.tx, id, &target.id, &outcome, now_ms()).await? {
        return Err(Error::Conflict(
            "this run is not running on this compute".into(),
        ));
    }
    store::tokens_delete_for_run(&mut *uow.tx, id).await?;
    uow.audit(
        &run.org_id,
        &Who::system(&run.on_behalf_of),
        "run.finished",
        &format!("run {id}"),
        &report.state,
    )
    .await?;
    uow.commit().await?;
    app.finished.notify_waiters();
    Ok(())
}

/// The person a job's token stands for, narrowed to what it declared, and the workspace it is for.
///
/// # Errors
/// The token is not one we know, has expired, or its run has ended.
pub async fn identity_for_token(app: &App, token: &str) -> Result<(Identity, String)> {
    let row = store::token_get(app.db.pool(), &hash_secret(token))
        .await?
        .ok_or(Error::Unauthorized)?;
    if row.expires_ms < now_ms() {
        return Err(Error::Unauthorized);
    }
    let run = store::run_by_id(app.db.pool(), &row.run_id)
        .await?
        .ok_or(Error::Unauthorized)?;
    if run.state != "running" {
        return Err(Error::Unauthorized);
    }
    let who: As = serde_json::from_str(&row.identity)
        .map_err(|e| Error::Internal(format!("a token has an identity that is not JSON: {e}")))?;
    let ids = |s: &str| serde_json::from_str::<Vec<String>>(s).unwrap_or_default();
    let scope = Scope {
        run: row.run_id,
        reads: ids(&row.reads),
        writes: ids(&row.writes),
    };
    Ok((who.identity(&row.org_id, scope), row.workspace_id))
}

// ---- keeping time ----------------------------------------------------------------------------------------------

/// Fails what can no longer finish: a run whose agent stopped answering, and one nobody took in time. Safe to run often.
///
/// # Errors
/// The database could not be reached.
pub async fn reap(app: &App, now: i64) -> Result<usize> {
    let stale = store::stale(app.db.pool(), now, now - QUEUE_MS).await?;
    let mut n = 0;
    for (id, org, state) in stale {
        let why = if state == "running" {
            "the compute stopped answering while this was running"
        } else {
            "no compute picked this up in time"
        };
        let mut uow = Uow::begin(app).await?;
        if store::fail(&mut *uow.tx, &id, &state, "failed", why, now).await? {
            store::tokens_delete_for_run(&mut *uow.tx, &id).await?;
            uow.audit(
                &org,
                &Who::system(""),
                "run.finished",
                &format!("run {id}"),
                "failed",
            )
            .await?;
            n += 1;
        }
        uow.commit().await?;
    }
    store::tokens_delete_expired(app.db.pool(), now).await?;
    Ok(n)
}

/// Runs forever, tidying every few seconds. Started once by the server.
pub async fn work_forever(app: crate::kernel::state::Shared) {
    loop {
        if let Err(e) = reap(&app, now_ms()).await {
            tracing::error!(error = %e, "the compute reaper could not reach the database");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Which compute may read a catalog, by id (empty: any compute of the workspace).
///
/// # Errors
/// Not allowed to see the catalog, or not a catalog.
pub async fn catalog_policy(app: &App, ctx: &Ctx<'_>, catalog: &str) -> Result<Vec<String>> {
    catalogs::compute_policy(
        app,
        ctx.identity,
        &ctx.workspace.id,
        &ctx.workspace.org_id,
        ctx.sides,
        catalog,
    )
    .await
}

/// Says which of the workspace's compute may read a catalog (none named: any). Needs `MANAGE` on the catalog.
///
/// # Errors
/// Not allowed, not a catalog, or a name is not this workspace's compute.
pub async fn set_catalog_policy(
    app: &App,
    ctx: &Ctx<'_>,
    catalog: &str,
    targets: &[String],
) -> Result<()> {
    let mine = store::targets_in(app.db.pool(), &ctx.workspace.id).await?;
    if let Some(bad) = targets.iter().find(|t| !mine.iter().any(|m| &m.id == *t)) {
        return Err(Error::Invalid(format!(
            "{bad} is not compute of this workspace"
        )));
    }
    catalogs::set_compute_policy(
        app,
        ctx.identity,
        &ctx.workspace.id,
        &ctx.workspace.org_id,
        ctx.sides,
        catalog,
        targets,
    )
    .await
}

/// How many runs are waiting and running, by state (for monitoring).
///
/// # Errors
/// The database could not be read.
pub async fn queue_depths(app: &App) -> Result<Vec<(String, i64)>> {
    store::queue_depths(app.db.pool()).await
}

/// The workspace's own cap on queued and running runs, if it has one (the organization's cap applies besides).
///
/// # Errors
/// Not in the workspace.
pub async fn quota(app: &App, ctx: &Ctx<'_>) -> Result<Option<i64>> {
    let mut c = app.db.pool().acquire().await?;
    store::workspace_quota(&mut c, &ctx.workspace.id).await
}

/// Sets the workspace's cap on queued and running runs (None: none of its own). Account admins only, because it limits what the
/// workspace's own admins may do.
///
/// # Errors
/// Not an account admin, or the cap is not positive.
pub async fn set_quota(app: &App, ctx: &Ctx<'_>, cap: Option<i64>) -> Result<()> {
    orgs::require_admin(app, ctx.identity, &ctx.workspace.org_id).await?;
    if cap.is_some_and(|n| n < 1) {
        return Err(Error::Invalid(
            "the cap is a positive number, or null for none".into(),
        ));
    }
    let mut uow = Uow::begin(app).await?;
    store::set_workspace_quota(&mut uow.tx, &ctx.workspace.id, cap).await?;
    uow.audit(
        &ctx.workspace.org_id,
        &Who::of(ctx.identity),
        "workspace.quota.set",
        &format!("workspace {}", ctx.workspace.id),
        "ok",
    )
    .await?;
    uow.commit().await
}
