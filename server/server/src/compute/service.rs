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
        DEFAULT_ROWS, LEASE_MS, Limits, MAX_OUTPUT, MAX_RESULT, MAX_ROWS, QUEUE_MS, Run, Target,
        cap, hash_secret, is_final_report, is_terminal, job_title, limits_for, mount_names,
        valid_target_name,
    },
    sql, store,
};
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

fn secret(prefix: &str) -> String {
    format!(
        "{prefix}{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

// ---- targets (the account's) -------------------------------------------------------------------------------------

/// A target as it is first made: the secret is shown once, and only its hash is kept.
pub struct Enrolled {
    pub target: Target,
    pub secret: String,
}

/// # Errors
/// Not an account admin, or a bad or taken name.
pub async fn add_target(
    app: &App,
    identity: &Identity,
    org: &str,
    name: &str,
    max_memory_mb: i64,
    max_cpu_millis: i64,
) -> Result<Enrolled> {
    orgs::require_admin(app, identity, org).await?;
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
        org_id: org.to_owned(),
        name: name.to_owned(),
        max_memory_mb,
        max_cpu_millis,
        created_by: identity.subject.clone(),
        created_ms: now_ms(),
        last_seen_ms: 0,
    };
    let secret = secret("agt_");
    let mut uow = Uow::begin(app).await?;
    if store::name_taken(&mut *uow.tx, org, name).await? {
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

/// Gives a target a new secret; the agent using the old one is turned away.
///
/// # Errors
/// Not an account admin, or no such target.
pub async fn new_secret(app: &App, identity: &Identity, org: &str, id: &str) -> Result<Enrolled> {
    orgs::require_admin(app, identity, org).await?;
    let target = store::target_by_id(app.db.pool(), org, id)
        .await?
        .ok_or_else(|| Error::NotFound("no such compute".into()))?;
    let secret = secret("agt_");
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

/// Removes a target. What was waiting or running on it fails.
///
/// # Errors
/// Not an account admin, or no such target.
pub async fn remove_target(app: &App, identity: &Identity, org: &str, id: &str) -> Result<()> {
    orgs::require_admin(app, identity, org).await?;
    store::target_by_id(app.db.pool(), org, id)
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

/// The organization's targets, with whether each is answering. Anyone who belongs to the organization may look.
///
/// # Errors
/// Not part of the organization.
pub async fn targets(app: &App, identity: &Identity, org: &str) -> Result<Vec<Target>> {
    orgs::get(app, identity, org).await?;
    store::targets_in(app.db.pool(), org).await
}

// ---- asking for work (a person's) -------------------------------------------------------------------------------

/// Who is asking, from where, and as what.
pub struct Ctx<'a> {
    pub identity: &'a Identity,
    pub workspace: &'a Workspace,
    pub standing: Standing,
    pub sides: &'a [Side],
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
    /// The volume that receives what the script leaves in `/workspace`, under `runs/<run id>/`.
    pub output: String,
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
    org: &str,
    asked: Option<&str>,
    memory: Option<i64>,
    cpu: Option<i64>,
) -> Result<Target> {
    let now = now_ms();
    let all = store::targets_in(app.db.pool(), org).await?;
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
        if !t.online(now) {
            return Err(Error::Conflict(format!("{} is not answering", t.name)));
        }
        return if big_enough(t) {
            Ok(t.clone())
        } else {
            Err(too_big(&[t]))
        };
    }
    let online: Vec<&Target> = all.iter().filter(|t| t.online(now)).collect();
    if online.is_empty() {
        return Err(Error::Conflict(
            "no compute is answering. Attach a machine in the account console, and start its agent"
                .into(),
        ));
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
    store::insert_run(&mut *uow.tx, run).await?;
    uow.audit(
        &run.org_id,
        &Who::of(ctx.identity),
        action,
        &format!("run {}", run.id),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// Reads a table the statement names, if the person may. Anything else, missing or not theirs to read, is the same answer.
async fn readable_table(app: &App, ctx: &Ctx<'_>, rel: &sql::Relation) -> Result<(String, String)> {
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
    Ok((entry.id, rel.join(".")))
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
    for rel in &analysis.relations {
        let (id, name) = readable_table(app, ctx, rel).await?;
        tables.push(json!({"id": id, "name": name}));
    }
    let limit = req.limit.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS);
    let target = pick(
        app,
        &ctx.workspace.org_id,
        req.target.as_deref(),
        None,
        None,
    )
    .await?;
    let limits = limits_for(&target, None, None, None, 120).map_err(Error::Invalid)?;
    let spec = json!({"as": As::of(ctx.identity), "sql": req.sql, "limit": limit, "tables": tables, "memory_mb": limits.memory_mb, "cpu_millis": limits.cpu_millis, "timeout_s": limits.timeout_s});
    let run = new_run(ctx, "sql", &req.sql, &target, &spec);
    queue(app, ctx, &run, "sql.submit").await?;
    Ok(run)
}

/// Asks for a job (a Python script) to be run.
///
/// # Errors
/// Not allowed, not valid, or no compute is answering.
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
    let (out, out_target) = catalogs::target_in(app, &ctx.workspace.id, org, &req.output).await?;
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
    let target = pick(
        app,
        org,
        req.target.as_deref(),
        req.memory_mb,
        req.cpu_millis,
    )
    .await?;
    let limits = limits_for(&target, req.memory_mb, req.cpu_millis, req.timeout_s, 600)
        .map_err(Error::Invalid)?;
    let title = job_title(&req.title, &req.script);
    let spec = json!({"as": As::of(ctx.identity), "script": req.script, "inputs": inputs, "output": out.id, "memory_mb": limits.memory_mb, "cpu_millis": limits.cpu_millis, "timeout_s": limits.timeout_s});
    let run = new_run(ctx, "job", &title, &target, &spec);
    queue(app, ctx, &run, "job.submit").await?;
    Ok(run)
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

/// The workspace's runs, newest first: the person's own, or everyone's for an admin.
///
/// # Errors
/// The database could not be read.
pub async fn list(app: &App, ctx: &Ctx<'_>, kind: Option<&str>, limit: i64) -> Result<Vec<Run>> {
    let by = if ctx.standing.is_admin() {
        None
    } else {
        Some(ctx.identity.subject.as_str())
    };
    store::runs_list(
        app.db.pool(),
        &ctx.workspace.id,
        by,
        kind,
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
        tokio::time::sleep(Duration::from_millis(150)).await;
        run = get(app, ctx, &run.id).await?;
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
}

/// Hands the target its next queued run, with the token that run's files are read and written with. None if nothing is waiting.
///
/// # Errors
/// The database could not be reached.
pub async fn lease(app: &App, target: &Target) -> Result<Option<Lease>> {
    let now = now_ms();
    let leased = {
        let mut conn = app.db.pool().acquire().await?;
        store::lease_one(&mut conn, &target.id, now, now + LEASE_MS).await?
    };
    let Some(run) = leased else { return Ok(None) };
    let spec: Value = serde_json::from_str(&run.spec)
        .map_err(|e| Error::Internal(format!("a run has a spec that is not JSON: {e}")))?;
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
    let (reads, writes) = if run.kind == "sql" {
        (strings(&spec["tables"]), vec![])
    } else {
        (
            strings(&spec["inputs"]),
            vec![spec["output"].as_str().unwrap_or_default().to_owned()],
        )
    };
    let token = secret("jt_");
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
    Ok(Some(Lease { run, token, limits }))
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
    uow.commit().await
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
