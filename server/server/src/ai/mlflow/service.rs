//! The gateway's decisions. Every request is authorized as the caller against the catalog's own grants before an upstream `MLflow`
//! server sees it, `MLflow`'s ids are trusted only through the id map, and what comes back is cut to what the caller may see.
//! (Proposal 0008, and `notes/0008-review.md`.)

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Method;
use serde_json::{Value, json};

use super::{
    model::{self, MAX_PAGES, MAX_SEARCH, Op, PAGE},
    store,
};
use crate::{
    governance::{
        audit::service as audit,
        catalogs::{
            model::{Draft, Kind, Securable},
            service as catalogs,
        },
        context::WsCtx,
        iam::service as iam,
    },
    kernel::{
        authz::{Privilege, Target},
        error::{Error, Result},
        state::App,
    },
};

/// The most a request body may be: tracking calls are small, and a batch of metrics well under this.
pub const MAX_BODY: usize = 2 * 1024 * 1024;

fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_mins(1))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default()
    })
}

fn missing() -> Error {
    Error::NotFound("not found".into())
}

/// One request to the upstream server, which only this module can reach. Nothing of the caller's is passed on: not their headers,
/// not their token. A refusal from `MLflow` is turned into one of ours; its own message is passed on only for a plain 4xx.
pub(super) async fn send(
    app: &App,
    method: Method,
    path: &str,
    query: &[(&str, &str)],
    body: Option<&Value>,
) -> Result<Value> {
    let base = app
        .mlflow_url
        .as_deref()
        .ok_or_else(|| Error::Unavailable("MLflow is not set up on this server".into()))?;
    let mut request = http()
        .request(method, format!("{}/{path}", base.trim_end_matches('/')))
        .query(query);
    if let Some(body) = body {
        request = request.json(body);
    }
    let reply = request.send().await.map_err(|e| {
        tracing::warn!(error = %e, "MLflow did not answer");
        Error::Unavailable("MLflow did not answer".into())
    })?;
    let status = reply.status();
    let text = reply.text().await.unwrap_or_default();
    // MLflow answers JSON, except `/version`, which is plain text.
    let json: Value = serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()));
    if status.is_success() {
        return Ok(json);
    }
    let said = json["message"]
        .as_str()
        .unwrap_or("MLflow refused the request");
    Err(match status.as_u16() {
        404 => Error::NotFound(said.to_owned()),
        400 | 422 => Error::Invalid(said.to_owned()),
        409 => Error::Conflict(said.to_owned()),
        _ => {
            tracing::warn!(status = status.as_u16(), "MLflow failed a request");
            Error::Unavailable("MLflow could not do that".into())
        }
    })
}

/// The experiment an `MLflow` id stands for in this workspace, with the caller's right to do `need` on it checked. An id that is not in
/// the map (another workspace's, or never made here) and one the caller may not see answer the same.
async fn experiment_for(
    app: &App,
    ctx: &WsCtx,
    experiment_id: &str,
    need: Privilege,
) -> Result<Securable> {
    let sid = store::securable_of(
        app.db.pool(),
        &ctx.workspace.id,
        "experiment",
        experiment_id,
    )
    .await?
    .ok_or_else(missing)?;
    secure(app, ctx, &sid, need).await
}

/// The experiment a run is in, with the same check.
async fn run_for(app: &App, ctx: &WsCtx, run_id: &str, need: Privilege) -> Result<Securable> {
    let sid = store::securable_of(app.db.pool(), &ctx.workspace.id, "run", run_id)
        .await?
        .ok_or_else(missing)?;
    secure(app, ctx, &sid, need).await
}

async fn secure(app: &App, ctx: &WsCtx, securable: &str, need: Privilege) -> Result<Securable> {
    let (entry, target) =
        catalogs::target_in(app, &ctx.workspace.id, &ctx.workspace.org_id, securable).await?;
    if entry.kind != Kind::Experiment {
        return Err(missing());
    }
    iam::require(
        app,
        &ctx.identity,
        &ctx.workspace.org_id,
        &ctx.sides,
        need,
        &target,
    )
    .await?;
    Ok(entry)
}

pub(super) async fn quietly_may(
    app: &App,
    ctx: &WsCtx,
    need: Privilege,
    target: &Target,
) -> Result<bool> {
    iam::may(
        app,
        &ctx.identity,
        &ctx.workspace.org_id,
        &ctx.sides,
        need,
        target,
    )
    .await
}

pub(super) fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v[key].as_str().filter(|s| !s.is_empty())
}

pub(super) fn query_of<'a>(query: &'a [(String, String)], key: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// An experiment's name as a person sees it.
fn show_name(ctx: &WsCtx, experiment: &mut Value) -> Result<()> {
    let upstream = experiment["name"].as_str().unwrap_or_default();
    let public = model::public_name(&ctx.workspace.id, upstream)
        .ok_or_else(|| Error::Internal("an experiment of another workspace was returned".into()))?;
    experiment["name"] = json!(public);
    Ok(())
}

/// The run id a request names, in either field `MLflow` uses.
fn run_id_of(body: &Value) -> Result<&str> {
    let (a, b) = (text(body, "run_id"), text(body, "run_uuid"));
    match (a, b) {
        (Some(a), Some(b)) if a != b => Err(Error::Invalid("run_id and run_uuid differ".into())),
        (Some(a), _) | (None, Some(a)) => Ok(a),
        (None, None) => Err(Error::Invalid("name the run".into())),
    }
}

/// Does one request. `query` is the address's query string and `body` its JSON (null when there is none).
///
/// # Errors
/// Not allowed (or not there: the same answer), not valid, or `MLflow` could not do it.
#[allow(clippy::too_many_lines)]
pub async fn handle(
    app: &App,
    ctx: &WsCtx,
    op: Op,
    query: &[(String, String)],
    body: &Value,
) -> Result<Value> {
    let tracking = |name: &str| format!("api/2.0/mlflow/{name}");
    match op {
        Op::Version => send(app, Method::GET, "version", &[], None).await,
        Op::ServerInfo => send(app, Method::GET, "api/3.0/mlflow/server-info", &[], None).await,
        Op::ExperimentCreate => experiment_create(app, ctx, body).await,
        Op::ExperimentGet => {
            let id = query_of(query, "experiment_id")
                .ok_or_else(|| Error::Invalid("name the experiment".into()))?;
            experiment_for(app, ctx, id, Privilege::Select).await?;
            let mut v = send(
                app,
                Method::GET,
                &tracking("experiments/get"),
                &[("experiment_id", id)],
                None,
            )
            .await?;
            show_name(ctx, &mut v["experiment"])?;
            Ok(v)
        }
        Op::ExperimentGetByName => {
            let given = query_of(query, "experiment_name")
                .ok_or_else(|| Error::Invalid("name the experiment".into()))?;
            let parts = model::parse_full_name(given).ok_or_else(missing)?;
            let path: Vec<String> = parts.to_vec();
            let (entry, _) =
                catalogs::resolve(app, &ctx.workspace.id, &ctx.workspace.org_id, &path).await?;
            if entry.kind != Kind::Experiment {
                return Err(missing());
            }
            let sid = entry.id;
            secure(app, ctx, &sid, Privilege::Select).await?;
            let upstream = store::upstream_of(app.db.pool(), &ctx.workspace.id, "experiment", &sid)
                .await?
                .ok_or_else(missing)?;
            let kept = model::upstream_name(&ctx.workspace.id, given);
            let mut v = send(
                app,
                Method::GET,
                &tracking("experiments/get-by-name"),
                &[("experiment_name", kept.as_str())],
                None,
            )
            .await?;
            if v["experiment"]["experiment_id"].as_str() != Some(upstream.as_str()) {
                return Err(Error::Internal(
                    "the experiment map and MLflow disagree".into(),
                ));
            }
            show_name(ctx, &mut v["experiment"])?;
            Ok(v)
        }
        Op::ExperimentSearch => experiment_search(app, ctx, body).await,
        Op::ExperimentUpdate => {
            experiment_for(
                app,
                ctx,
                text(body, "experiment_id").unwrap_or_default(),
                Privilege::Modify,
            )
            .await?;
            // A new name would have to move the catalog object too: not yet.
            Err(Error::Invalid(
                "renaming an experiment is not supported: make a new one".into(),
            ))
        }
        Op::ExperimentDelete | Op::ExperimentRestore => {
            let id = text(body, "experiment_id").unwrap_or_default();
            let sec = experiment_for(app, ctx, id, Privilege::Manage).await?;
            let name = if op == Op::ExperimentDelete {
                "experiments/delete"
            } else {
                "experiments/restore"
            };
            let v = send(
                app,
                Method::POST,
                &tracking(name),
                &[],
                Some(&json!({"experiment_id": id})),
            )
            .await?;
            audit::record(
                app,
                &ctx.workspace.org_id,
                &crate::governance::audit::model::Who::of(&ctx.identity),
                if op == Op::ExperimentDelete {
                    "mlflow.experiment.delete"
                } else {
                    "mlflow.experiment.restore"
                },
                &format!("experiment {}", sec.id),
                "ok",
            )
            .await?;
            Ok(v)
        }
        Op::ExperimentSetTag | Op::ExperimentDeleteTag => {
            let id = text(body, "experiment_id").unwrap_or_default();
            experiment_for(app, ctx, id, Privilege::Modify).await?;
            let name = if op == Op::ExperimentSetTag {
                "experiments/set-experiment-tag"
            } else {
                "experiments/delete-experiment-tag"
            };
            send(app, Method::POST, &tracking(name), &[], Some(body)).await
        }
        Op::RunCreate => run_create(app, ctx, body).await,
        Op::RunGet | Op::MetricHistory => {
            let id = query_of(query, "run_id")
                .or_else(|| query_of(query, "run_uuid"))
                .ok_or_else(|| Error::Invalid("name the run".into()))?;
            run_for(app, ctx, id, Privilege::Select).await?;
            let name = if op == Op::RunGet {
                "runs/get"
            } else {
                "metrics/get-history"
            };
            let mut q: Vec<(&str, &str)> = vec![("run_id", id)];
            if let Some(k) = query_of(query, "metric_key") {
                q.push(("metric_key", k));
            }
            // A page token from MLflow only moves through what this run already holds.
            if let Some(t) = query_of(query, "page_token") {
                q.push(("page_token", t));
            }
            if let Some(m) = query_of(query, "max_results") {
                q.push(("max_results", m));
            }
            send(app, Method::GET, &tracking(name), &q, None).await
        }
        Op::RunUpdate
        | Op::RunDelete
        | Op::RunRestore
        | Op::RunLogMetric
        | Op::RunLogParameter
        | Op::RunLogBatch
        | Op::RunLogInputs
        | Op::RunSetTag
        | Op::RunDeleteTag => {
            run_for(app, ctx, run_id_of(body)?, Privilege::Modify).await?;
            let name = match op {
                Op::RunUpdate => "runs/update",
                Op::RunDelete => "runs/delete",
                Op::RunRestore => "runs/restore",
                Op::RunLogMetric => "runs/log-metric",
                Op::RunLogParameter => "runs/log-parameter",
                Op::RunLogBatch => "runs/log-batch",
                Op::RunLogInputs => "runs/log-inputs",
                Op::RunSetTag => "runs/set-tag",
                _ => "runs/delete-tag",
            };
            send(app, Method::POST, &tracking(name), &[], Some(body)).await
        }
        Op::RunSearch => run_search(app, ctx, body).await,
        _ => super::registry::handle(app, ctx, op, query, body).await,
    }
}

async fn experiment_create(app: &App, ctx: &WsCtx, body: &Value) -> Result<Value> {
    let given = text(body, "name").ok_or_else(|| Error::Invalid("name the experiment".into()))?;
    let parts = model::parse_full_name(given).ok_or_else(|| {
        Error::Invalid(
            "name an experiment as catalog.schema.name, in lower case with digits and underscores"
                .into(),
        )
    })?;
    if text(body, "artifact_location").is_some() {
        return Err(Error::Invalid(
            "an experiment's files are kept where Loamz puts them: leave artifact_location out"
                .into(),
        ));
    }
    let org = &ctx.workspace.org_id;
    let schema_path = vec![parts[0].clone(), parts[1].clone()];
    let (schema, target) = catalogs::resolve(app, &ctx.workspace.id, org, &schema_path).await?;
    if schema.kind != Kind::Schema {
        return Err(missing());
    }
    iam::require(
        app,
        &ctx.identity,
        org,
        &ctx.sides,
        Privilege::CreateExperiment,
        &target,
    )
    .await?;
    let full: Vec<String> = parts.to_vec();
    if catalogs::resolve(app, &ctx.workspace.id, org, &full)
        .await
        .is_ok()
    {
        return Err(Error::Conflict(format!("{given} already exists")));
    }
    let mut upstream_body = json!({"name": model::upstream_name(&ctx.workspace.id, given)});
    if let Some(tags) = body.get("tags") {
        upstream_body["tags"] = tags.clone();
    }
    let made = send(
        app,
        Method::POST,
        "api/2.0/mlflow/experiments/create",
        &[],
        Some(&upstream_body),
    )
    .await?;
    let id = text(&made, "experiment_id")
        .ok_or_else(|| Error::Internal("MLflow made an experiment without an id".into()))?
        .to_owned();
    let draft = Draft {
        kind: Kind::Experiment,
        parent_id: schema.id,
        name: parts[2].clone(),
        classification: String::new(),
        placement: String::new(),
        consistency: String::new(),
        location: None,
        table: None,
    };
    let made_entry = catalogs::create(
        app,
        &ctx.identity,
        &ctx.workspace.id,
        org,
        &ctx.sides,
        draft,
    )
    .await;
    match made_entry {
        Ok(entry) => {
            store::insert(
                app.db.pool(),
                &ctx.workspace.id,
                "experiment",
                &id,
                &entry.id,
                &ctx.identity.subject,
            )
            .await?;
            Ok(json!({"experiment_id": id}))
        }
        Err(e) => {
            // The catalog would not take it: take it back from MLflow, so nothing is left that nobody can see.
            let _ = send(
                app,
                Method::POST,
                "api/2.0/mlflow/experiments/delete",
                &[],
                Some(&json!({"experiment_id": id})),
            )
            .await;
            Err(e)
        }
    }
}

/// The experiments the caller may read, as many as asked for and no more. There is no total and no next-page token: either would count
/// what the caller may not see. The upstream search is narrowed to this workspace first, and each result is checked one by one.
async fn experiment_search(app: &App, ctx: &WsCtx, body: &Value) -> Result<Value> {
    for refused in ["filter", "order_by", "page_token"] {
        let present = match &body[refused] {
            Value::Null => false,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            _ => true,
        };
        if present {
            return Err(Error::Invalid(format!(
                "searching experiments by {refused} is not supported"
            )));
        }
    }
    let want = usize::try_from(body["max_results"].as_u64().unwrap_or(1000))
        .unwrap_or(MAX_SEARCH)
        .clamp(1, MAX_SEARCH);
    let view = match body["view_type"].as_str() {
        Some(v @ ("ACTIVE_ONLY" | "DELETED_ONLY" | "ALL")) => v,
        None | Some("") => "ACTIVE_ONLY",
        Some(_) => {
            return Err(Error::Invalid(
                "view_type is ACTIVE_ONLY, DELETED_ONLY or ALL".into(),
            ));
        }
    };
    let mut out: Vec<Value> = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut page = json!({
            "max_results": PAGE, "view_type": view,
            "filter": format!("name LIKE '{}.%'", ctx.workspace.id),
        });
        if let Some(t) = &token {
            page["page_token"] = json!(t);
        }
        let got = send(
            app,
            Method::POST,
            "api/2.0/mlflow/experiments/search",
            &[],
            Some(&page),
        )
        .await?;
        for mut e in got["experiments"].as_array().cloned().unwrap_or_default() {
            let Some(id) = e["experiment_id"].as_str() else {
                continue;
            };
            let Some(sid) =
                store::securable_of(app.db.pool(), &ctx.workspace.id, "experiment", id).await?
            else {
                continue;
            };
            let Ok((_, target)) =
                catalogs::target_in(app, &ctx.workspace.id, &ctx.workspace.org_id, &sid).await
            else {
                continue;
            };
            if !quietly_may(app, ctx, Privilege::Select, &target).await? {
                continue;
            }
            show_name(ctx, &mut e)?;
            out.push(e);
            if out.len() == want {
                return Ok(json!({"experiments": out}));
            }
        }
        token = got["next_page_token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        if token.is_none() {
            break;
        }
    }
    Ok(json!({"experiments": out}))
}

async fn run_create(app: &App, ctx: &WsCtx, body: &Value) -> Result<Value> {
    let eid =
        text(body, "experiment_id").ok_or_else(|| Error::Invalid("name the experiment".into()))?;
    let sec = experiment_for(app, ctx, eid, Privilege::Modify).await?;
    let mut upstream = body.clone();
    // The run is the caller's: not whoever the client says.
    upstream["user_id"] = json!(ctx.identity.subject);
    let mut made = send(
        app,
        Method::POST,
        "api/2.0/mlflow/runs/create",
        &[],
        Some(&upstream),
    )
    .await?;
    let run_id = made["run"]["info"]["run_id"]
        .as_str()
        .ok_or_else(|| Error::Internal("MLflow made a run without an id".into()))?
        .to_owned();
    store::insert(
        app.db.pool(),
        &ctx.workspace.id,
        "run",
        &run_id,
        &sec.id,
        &ctx.identity.subject,
    )
    .await?;
    made["run"]["info"]["user_id"] = json!(ctx.identity.subject);
    Ok(made)
}

/// Runs of experiments the caller may read: every experiment named must be one, or the whole request is not there.
async fn run_search(app: &App, ctx: &WsCtx, body: &Value) -> Result<Value> {
    let ids: Vec<&str> = body["experiment_ids"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if ids.is_empty() || ids.len() > 100 {
        return Err(Error::Invalid("name between 1 and 100 experiments".into()));
    }
    for id in &ids {
        experiment_for(app, ctx, id, Privilege::Select).await?;
    }
    let mut upstream = json!({"experiment_ids": ids});
    for key in [
        "filter",
        "run_view_type",
        "max_results",
        "order_by",
        "page_token",
    ] {
        if !body[key].is_null() {
            upstream[key] = body[key].clone();
        }
    }
    send(
        app,
        Method::POST,
        "api/2.0/mlflow/runs/search",
        &[],
        Some(&upstream),
    )
    .await
}

/// Takes away what `MLflow` holds for a securable that has been dropped or erased: its experiment or model upstream, and the map rows. Safe to
/// run again (a durable task retries it): what is already gone is not an error.
///
/// # Errors
/// `MLflow` is needed and cannot be reached, or the database could not be written.
pub async fn forget(app: &App, securable_id: &str) -> Result<()> {
    let held = store::of_securable(app.db.pool(), securable_id).await?;
    for (kind, upstream) in &held {
        let (path, body) = match kind.as_str() {
            "experiment" => (
                "api/2.0/mlflow/experiments/delete",
                json!({"experiment_id": upstream}),
            ),
            "model" => (
                "api/2.0/mlflow/registered-models/delete",
                json!({"name": upstream}),
            ),
            _ => continue,
        };
        let method = if kind == "model" {
            Method::DELETE
        } else {
            Method::POST
        };
        match send(app, method, path, &[], Some(&body)).await {
            Ok(_) | Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    store::forget_securable(app.db.pool(), securable_id).await
}
