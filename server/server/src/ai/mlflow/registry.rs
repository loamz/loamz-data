//! Registered models and prompts. `MLflow` keeps a prompt as a registered model with a tag; here each is a catalog object (`model` or
//! `prompt`) named `catalog.schema.name`, so grants, hiding and drop work as for a table. `MLflow`'s own name is made unique per
//! workspace with a prefix, and every name that comes back is shown without it. Only prompts have versions for now: a model version
//! points at files, and those come with the artifact API.

use reqwest::Method;
use serde_json::{Value, json};

use super::{
    model::{self, MAX_PAGES, MAX_SEARCH, Op, PAGE},
    service::{query_of, quietly_may, send, text},
    store,
};
use crate::{
    governance::{
        catalogs::{
            model::{Draft, Kind, Securable},
            service as catalogs,
        },
        context::WsCtx,
        iam::service as iam,
    },
    kernel::{
        authz::Privilege,
        error::{Error, Result},
        state::App,
    },
};

fn missing() -> Error {
    Error::NotFound("not found".into())
}

fn tracking(name: &str) -> String {
    format!("api/2.0/mlflow/{name}")
}

/// `MLflow`'s name for a person's: made unique per workspace.
fn kept(ctx: &WsCtx, given: &str) -> Result<String> {
    model::parse_full_name(given).ok_or_else(|| {
        Error::Invalid(
            "name it as catalog.schema.name, in lower case with digits and underscores".into(),
        )
    })?;
    Ok(model::upstream_name(&ctx.workspace.id, given))
}

/// Takes the workspace's prefix off every `name` in an answer, so a person sees the names they gave. A name from another workspace
/// would be a bug in what was asked of `MLflow`: it is refused, not shown.
fn show(ctx: &WsCtx, value: &mut Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if key == "name" {
                    if let Some(upstream) = inner.as_str() {
                        let public =
                            model::public_name(&ctx.workspace.id, upstream).ok_or_else(|| {
                                Error::Internal(
                                    "a registry entry of another workspace was returned".into(),
                                )
                            })?;
                        *inner = json!(public);
                    }
                } else {
                    show(ctx, inner)?;
                }
            }
        }
        Value::Array(items) => {
            for inner in items {
                show(ctx, inner)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The model or prompt a name stands for, with the caller's right to do `need` on it checked. Missing, another kind, and unseen answer alike.
async fn registered_for(app: &App, ctx: &WsCtx, given: &str, need: Privilege) -> Result<Securable> {
    let parts = model::parse_full_name(given).ok_or_else(missing)?;
    let path: Vec<String> = parts.to_vec();
    let (entry, target) =
        catalogs::resolve(app, &ctx.workspace.id, &ctx.workspace.org_id, &path).await?;
    if !matches!(entry.kind, Kind::Model | Kind::Prompt) {
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

fn name_in<'a>(query: &'a [(String, String)], body: &'a Value) -> Result<&'a str> {
    query_of(query, "name")
        .or_else(|| text(body, "name"))
        .ok_or_else(|| Error::Invalid("name it".into()))
}

pub(super) async fn handle(
    app: &App,
    ctx: &WsCtx,
    op: Op,
    query: &[(String, String)],
    body: &Value,
) -> Result<Value> {
    match op {
        Op::RegisteredModelCreate => create(app, ctx, body).await,
        Op::RegisteredModelSearch => search(app, ctx, query).await,
        Op::RegisteredModelDelete => {
            registered_for(app, ctx, name_in(query, body)?, Privilege::Manage).await?;
            // The catalog's own drop removes it here and in MLflow, as one durable task; doing half of that from here would leave the two apart.
            Err(Error::Invalid(
                "drop it from the catalog: that removes it from MLflow too".into(),
            ))
        }
        Op::ModelVersionCreate => version_create(app, ctx, body).await,
        Op::ModelVersionSearch => version_search(app, ctx, query).await,
        Op::RegisteredModelGet | Op::ModelVersionGet | Op::AliasGet => {
            let given = name_in(query, body)?;
            registered_for(app, ctx, given, Privilege::Select).await?;
            let (path, mut q) = match op {
                Op::RegisteredModelGet => ("registered-models/get", vec![]),
                Op::ModelVersionGet => (
                    "model-versions/get",
                    vec![("version", query_of(query, "version"))],
                ),
                _ => (
                    "registered-models/alias",
                    vec![("alias", query_of(query, "alias"))],
                ),
            };
            let upstream = kept(ctx, given)?;
            let mut pairs: Vec<(&str, &str)> = vec![("name", upstream.as_str())];
            for (k, v) in q.drain(..) {
                pairs.push((k, v.ok_or_else(|| Error::Invalid(format!("name the {k}")))?));
            }
            let mut v = send(app, Method::GET, &tracking(path), &pairs, None).await?;
            show(ctx, &mut v)?;
            Ok(v)
        }
        Op::RegisteredModelUpdate
        | Op::RegisteredModelSetTag
        | Op::AliasSet
        | Op::AliasDelete
        | Op::ModelVersionDelete
        | Op::ModelVersionSetTag => {
            let given = text(body, "name").ok_or_else(|| Error::Invalid("name it".into()))?;
            registered_for(app, ctx, given, Privilege::Modify).await?;
            let (method, path) = match op {
                Op::RegisteredModelUpdate => (Method::PATCH, "registered-models/update"),
                Op::RegisteredModelSetTag => (Method::POST, "registered-models/set-tag"),
                Op::AliasSet => (Method::POST, "registered-models/alias"),
                Op::AliasDelete => (Method::DELETE, "registered-models/alias"),
                Op::ModelVersionDelete => (Method::DELETE, "model-versions/delete"),
                _ => (Method::POST, "model-versions/set-tag"),
            };
            let mut upstream = body.clone();
            upstream["name"] = json!(kept(ctx, given)?);
            let mut v = send(app, method, &tracking(path), &[], Some(&upstream)).await?;
            show(ctx, &mut v)?;
            Ok(v)
        }
        _ => Err(missing()),
    }
}

async fn create(app: &App, ctx: &WsCtx, body: &Value) -> Result<Value> {
    let given = text(body, "name").ok_or_else(|| Error::Invalid("name it".into()))?;
    let upstream_name = kept(ctx, given)?;
    let parts = model::parse_full_name(given).ok_or_else(missing)?;
    let prompt = model::is_prompt(&body["tags"]);
    let (kind, need) = if prompt {
        (Kind::Prompt, Privilege::CreatePrompt)
    } else {
        (Kind::Model, Privilege::CreateModel)
    };
    let org = &ctx.workspace.org_id;
    let schema_path = vec![parts[0].clone(), parts[1].clone()];
    let (schema, target) = catalogs::resolve(app, &ctx.workspace.id, org, &schema_path).await?;
    if schema.kind != Kind::Schema {
        return Err(missing());
    }
    iam::require(app, &ctx.identity, org, &ctx.sides, need, &target).await?;
    let full: Vec<String> = parts.to_vec();
    if catalogs::resolve(app, &ctx.workspace.id, org, &full)
        .await
        .is_ok()
    {
        return Err(Error::Conflict(format!("{given} already exists")));
    }
    let mut upstream = body.clone();
    upstream["name"] = json!(upstream_name);
    let mut made = send(
        app,
        Method::POST,
        &tracking("registered-models/create"),
        &[],
        Some(&upstream),
    )
    .await?;
    let draft = Draft {
        kind,
        parent_id: schema.id,
        name: parts[2].clone(),
        classification: String::new(),
        placement: String::new(),
        consistency: String::new(),
        location: None,
        table: None,
    };
    match catalogs::create(
        app,
        &ctx.identity,
        &ctx.workspace.id,
        org,
        &ctx.sides,
        draft,
    )
    .await
    {
        Ok(entry) => {
            // The map lets a drop find what MLflow holds for this securable.
            store::insert(
                app.db.pool(),
                &ctx.workspace.id,
                "model",
                &upstream_name,
                &entry.id,
                &ctx.identity.subject,
            )
            .await?;
            show(ctx, &mut made)?;
            Ok(made)
        }
        Err(e) => {
            let _ = send(
                app,
                Method::DELETE,
                &tracking("registered-models/delete"),
                &[],
                Some(&json!({"name": upstream_name})),
            )
            .await;
            Err(e)
        }
    }
}

/// A new version. Only a prompt's: a model's points at files, which the artifact API brings. The source is the one `MLflow`'s own client
/// writes for a prompt, and nothing else (a path chosen by the client could point anywhere).
async fn version_create(app: &App, ctx: &WsCtx, body: &Value) -> Result<Value> {
    let given = text(body, "name").ok_or_else(|| Error::Invalid("name it".into()))?;
    let entry = registered_for(app, ctx, given, Privilege::Modify).await?;
    if entry.kind != Kind::Prompt {
        return Err(Error::Invalid(
            "versions of a model need the artifact store: not yet".into(),
        ));
    }
    if body["source"] != "dummy-source" || !body["run_id"].is_null() || !body["model_id"].is_null()
    {
        return Err(Error::Invalid(
            "a prompt version has no source, run or model".into(),
        ));
    }
    if !model::is_prompt(&body["tags"]) {
        return Err(Error::Invalid(
            "a prompt version is marked as a prompt".into(),
        ));
    }
    let mut upstream = body.clone();
    upstream["name"] = json!(kept(ctx, given)?);
    let mut v = send(
        app,
        Method::POST,
        &tracking("model-versions/create"),
        &[],
        Some(&upstream),
    )
    .await?;
    show(ctx, &mut v)?;
    Ok(v)
}

/// Versions of one model, as the client asks: `name='…'`.
async fn version_search(app: &App, ctx: &WsCtx, query: &[(String, String)]) -> Result<Value> {
    let filter = query_of(query, "filter")
        .ok_or_else(|| Error::Invalid("search versions of one model: filter name='…'".into()))?;
    let given = filter
        .strip_prefix("name='")
        .and_then(|rest| rest.strip_suffix('\''))
        .filter(|n| !n.contains('\''))
        .ok_or_else(|| Error::Invalid("search versions of one model: filter name='…'".into()))?;
    registered_for(app, ctx, given, Privilege::Select).await?;
    let upstream = kept(ctx, given)?;
    let filter = format!("name='{upstream}'");
    let mut pairs: Vec<(&str, &str)> = vec![("filter", filter.as_str())];
    for key in ["max_results", "page_token", "order_by"] {
        if let Some(v) = query_of(query, key) {
            pairs.push((key, v));
        }
    }
    let mut v = send(
        app,
        Method::GET,
        &tracking("model-versions/search"),
        &pairs,
        None,
    )
    .await?;
    show(ctx, &mut v)?;
    Ok(v)
}

/// The models or the prompts the caller may read: which, depends on whether the client asked for prompts. No total and no next-page
/// token, as with experiments.
async fn search(app: &App, ctx: &WsCtx, query: &[(String, String)]) -> Result<Value> {
    let filter = query_of(query, "filter").unwrap_or_default();
    let wants_prompts = filter.contains(model::PROMPT_TAG);
    let plain = filter.is_empty() || filter == format!("tags.`{}` = 'true'", model::PROMPT_TAG);
    if !plain || query_of(query, "order_by").is_some() || query_of(query, "page_token").is_some() {
        return Err(Error::Invalid(
            "searching registered models by that filter, order or page is not supported".into(),
        ));
    }
    let want = query_of(query, "max_results")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, MAX_SEARCH);
    let kind = if wants_prompts {
        Kind::Prompt
    } else {
        Kind::Model
    };
    // MLflow keeps prompts out of a registered-model search unless the filter names the prompt tag.
    let upstream_filter = if wants_prompts {
        format!(
            "name LIKE '{}.%' AND tags.`{}` = 'true'",
            ctx.workspace.id,
            model::PROMPT_TAG
        )
    } else {
        format!("name LIKE '{}.%'", ctx.workspace.id)
    };
    let mut out: Vec<Value> = Vec::new();
    let mut token: Option<String> = None;
    let page = PAGE.to_string();
    for _ in 0..MAX_PAGES {
        let mut pairs: Vec<(&str, &str)> = vec![
            ("filter", upstream_filter.as_str()),
            ("max_results", page.as_str()),
        ];
        if let Some(t) = &token {
            pairs.push(("page_token", t.as_str()));
        }
        let got = send(
            app,
            Method::GET,
            &tracking("registered-models/search"),
            &pairs,
            None,
        )
        .await?;
        for mut item in got["registered_models"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let Some(public) = item["name"]
                .as_str()
                .and_then(|n| model::public_name(&ctx.workspace.id, n))
            else {
                continue;
            };
            let Some(parts) = model::parse_full_name(&public) else {
                continue;
            };
            let path: Vec<String> = parts.to_vec();
            let Ok((entry, target)) =
                catalogs::resolve(app, &ctx.workspace.id, &ctx.workspace.org_id, &path).await
            else {
                continue;
            };
            if entry.kind != kind || !quietly_may(app, ctx, Privilege::Select, &target).await? {
                continue;
            }
            show(ctx, &mut item)?;
            out.push(item);
            if out.len() == want {
                return Ok(json!({"registered_models": out}));
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
    Ok(json!({"registered_models": out}))
}
