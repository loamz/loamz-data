//! The Iceberg REST catalog (`/api/v1/workspaces/{ws}/iceberg/v1/…`), the way `DuckDB`, Spark and `PyIceberg` speak to a catalog, and the
//! calls the screens use. The "warehouse" a client names is a catalog, a "namespace" is a schema.

use std::collections::HashMap;

use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    model,
    service::{self, Loaded, NewTable},
};
use crate::{
    governance::context::WsCtx,
    kernel::{
        error::{Catalog, Error, Result},
        http::{Routes, json as body_json},
        state::Shared,
    },
    storage::volumes::service::Ask,
};

type Out<T> = std::result::Result<T, Catalog>;

fn ask(c: &WsCtx) -> Ask<'_> {
    Ask {
        identity: &c.identity,
        workspace: &c.workspace.id,
        org: c.org(),
        sides: &c.sides,
    }
}

fn param(c: &WsCtx, name: &str) -> Result<String> {
    let v = c
        .params
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Internal(format!("the route has no {name}")))?;
    // A multi-level namespace is joined with the unit separator; schemas here are one level.
    if v.contains('\u{1f}') {
        return Err(Error::NotFound("not found".into()));
    }
    Ok(v)
}

fn loaded(l: &Loaded, endpoint: Option<(&str, &str)>) -> Value {
    let mut config = serde_json::Map::new();
    if let (Some((c, _write)), Some((endpoint, _))) = (&l.credentials, endpoint) {
        config.insert("s3.access-key-id".into(), json!(c.key_id));
        config.insert("s3.secret-access-key".into(), json!(c.secret));
        config.insert("s3.session-token".into(), json!(c.session_token));
        config.insert("s3.endpoint".into(), json!(endpoint));
        config.insert("s3.region".into(), json!("auto"));
        config.insert("s3.path-style-access".into(), json!("true"));
    }
    json!({"metadata-location": l.metadata_location, "metadata": l.metadata, "config": config})
}

async fn config(c: WsCtx, Query(q): Query<HashMap<String, String>>) -> Out<Json<Value>> {
    let _ = &c;
    let warehouse = q
        .get("warehouse")
        .filter(|w| !w.is_empty())
        .ok_or_else(|| Error::Invalid("name the catalog as the warehouse".into()))?;
    Ok(Json(
        json!({"defaults": {}, "overrides": {"prefix": warehouse}}),
    ))
}

async fn list_namespaces(State(app): State<Shared>, c: WsCtx) -> Out<Json<Value>> {
    let names = service::namespaces(&app, &ask(&c), &param(&c, "prefix")?).await?;
    let list: Vec<Value> = names.iter().map(|n| json!([n])).collect();
    Ok(Json(json!({"namespaces": list})))
}

#[derive(Deserialize)]
struct NewNamespace {
    namespace: Vec<String>,
}

async fn create_namespace(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Out<Json<Value>> {
    let req: NewNamespace = body_json(&body)?;
    let [name] = req.namespace.as_slice() else {
        return Err(Error::Invalid("a namespace is one name".into()).into());
    };
    service::create_namespace(&app, &ask(&c), &param(&c, "prefix")?, name).await?;
    Ok(Json(json!({"namespace": [name], "properties": {}})))
}

async fn get_namespace(State(app): State<Shared>, c: WsCtx) -> Out<Json<Value>> {
    let (prefix, ns) = (param(&c, "prefix")?, param(&c, "ns")?);
    service::namespace_exists(&app, &ask(&c), &prefix, &ns).await?;
    Ok(Json(json!({"namespace": [ns], "properties": {}})))
}

async fn list_tables(State(app): State<Shared>, c: WsCtx) -> Out<Json<Value>> {
    let (prefix, ns) = (param(&c, "prefix")?, param(&c, "ns")?);
    let names = service::tables(&app, &ask(&c), &prefix, &ns).await?;
    let ids: Vec<Value> = names
        .iter()
        .map(|n| json!({"namespace": [ns], "name": n}))
        .collect();
    Ok(Json(json!({"identifiers": ids})))
}

#[derive(Deserialize)]
struct CreateTable {
    name: String,
    schema: Value,
    #[serde(rename = "partition-spec")]
    spec: Option<Value>,
    #[serde(rename = "write-order")]
    order: Option<Value>,
    #[serde(default)]
    properties: Value,
    #[serde(rename = "stage-create", default)]
    stage_create: bool,
}

async fn create_table(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Out<Json<Value>> {
    let req: CreateTable = body_json(&body)?;
    let made = service::create(
        &app,
        &ask(&c),
        &param(&c, "prefix")?,
        &param(&c, "ns")?,
        NewTable {
            name: req.name,
            schema: req.schema,
            spec: req.spec,
            order: req.order,
            properties: if req.properties.is_object() {
                req.properties
            } else {
                json!({})
            },
            classification: String::new(),
            stage: req.stage_create,
        },
    )
    .await?;
    Ok(Json(loaded(&made, app.stores.bucket_place())))
}

async fn load_table(State(app): State<Shared>, c: WsCtx) -> Out<Json<Value>> {
    let l = service::load(
        &app,
        &ask(&c),
        &param(&c, "prefix")?,
        &param(&c, "ns")?,
        &param(&c, "t")?,
    )
    .await?;
    Ok(Json(loaded(&l, app.stores.bucket_place())))
}

#[derive(Deserialize)]
struct Commit {
    #[serde(default)]
    requirements: Vec<Value>,
    #[serde(default)]
    updates: Vec<Value>,
}

async fn commit_table(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Out<Json<Value>> {
    let req: Commit = body_json(&body)?;
    let (location, metadata) = service::commit(
        &app,
        &ask(&c),
        &param(&c, "prefix")?,
        &param(&c, "ns")?,
        &param(&c, "t")?,
        &req.requirements,
        &req.updates,
    )
    .await?;
    Ok(Json(
        json!({"metadata-location": location, "metadata": metadata}),
    ))
}

#[derive(Deserialize)]
struct Change {
    identifier: Identifier,
    #[serde(default)]
    requirements: Vec<Value>,
    #[serde(default)]
    updates: Vec<Value>,
}

#[derive(Deserialize)]
struct Identifier {
    namespace: Vec<String>,
    name: String,
}

#[derive(Deserialize)]
struct Transaction {
    #[serde(rename = "table-changes")]
    changes: Vec<Change>,
}

/// Several tables in one commit are not supported (they would need one atomic move across tables): one table, as clients send
/// for a single statement.
async fn commit_transaction(State(app): State<Shared>, c: WsCtx, body: Bytes) -> Out<StatusCode> {
    let req: Transaction = body_json(&body)?;
    let [change] = req.changes.as_slice() else {
        return Err(Error::Invalid("commit one table at a time".into()).into());
    };
    let [ns] = change.identifier.namespace.as_slice() else {
        return Err(Error::Invalid("a namespace is one name".into()).into());
    };
    service::commit(
        &app,
        &ask(&c),
        &param(&c, "prefix")?,
        ns,
        &change.identifier.name,
        &change.requirements,
        &change.updates,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- what the screens use ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Column {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    required: bool,
}

#[derive(Deserialize)]
struct MakeTable {
    parent_id: String,
    name: String,
    columns: Vec<Column>,
    #[serde(default)]
    classification: String,
}

async fn make_table(
    State(app): State<Shared>,
    c: WsCtx,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let req: MakeTable = body_json(&body)?;
    let columns: Vec<(String, String, bool)> = req
        .columns
        .into_iter()
        .map(|c| (c.name, c.ty, c.required))
        .collect();
    let schema = model::schema_from_columns(&columns).map_err(Error::Invalid)?;
    let made = service::create_under(
        &app,
        &ask(&c),
        &req.parent_id,
        NewTable {
            name: req.name,
            schema,
            spec: None,
            order: None,
            properties: json!({}),
            classification: req.classification,
            stage: false,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({"id": made.id}))))
}

async fn column_stats(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    Ok(Json(
        service::column_stats(&app, &ask(&c), &param(&c, "id")?).await?,
    ))
}

async fn describe(State(app): State<Shared>, c: WsCtx) -> Result<Json<Value>> {
    Ok(Json(
        service::describe(&app, &ask(&c), &param(&c, "id")?).await?,
    ))
}

#[must_use]
pub fn routes() -> Routes {
    let base = "/api/v1/workspaces/{ws}/iceberg/v1";
    Routes::new()
        .get(&format!("{base}/config"), config)
        .get(&format!("{base}/{{prefix}}/namespaces"), list_namespaces)
        .post(&format!("{base}/{{prefix}}/namespaces"), create_namespace)
        .get(
            &format!("{base}/{{prefix}}/namespaces/{{ns}}"),
            get_namespace,
        )
        .get(
            &format!("{base}/{{prefix}}/namespaces/{{ns}}/tables"),
            list_tables,
        )
        .post(
            &format!("{base}/{{prefix}}/namespaces/{{ns}}/tables"),
            create_table,
        )
        .get(
            &format!("{base}/{{prefix}}/namespaces/{{ns}}/tables/{{t}}"),
            load_table,
        )
        .post(
            &format!("{base}/{{prefix}}/namespaces/{{ns}}/tables/{{t}}"),
            commit_table,
        )
        .post(
            &format!("{base}/{{prefix}}/transactions/commit"),
            commit_transaction,
        )
        .post("/api/v1/workspaces/{ws}/tables", make_table)
        .get("/api/v1/workspaces/{ws}/tables/{id}", describe)
        .get("/api/v1/workspaces/{ws}/tables/{id}/stats", column_stats)
}
