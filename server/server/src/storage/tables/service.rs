//! The Iceberg catalog: namespaces are schemas, tables are Iceberg tables. Every call finds the thing, asks IAM, then acts.
//!
//! The bytes of a table live in its own folder of the store. This service writes only the table's metadata files and moves its
//! pointer; the data and manifest files are written by the client with short-lived credentials limited to that folder.

use std::time::Duration;

use futures_util::StreamExt;
use loamz_lake::grants::Credentials;
use serde_json::{Value, json};

use super::model;
use crate::{
    governance::{
        audit::{model::Who, service as audit, service::Uow},
        catalogs::{
            model::{Draft, Kind, Securable},
            service as catalogs,
        },
        iam::service as iam,
    },
    kernel::{
        authz::{Privilege, Target},
        error::{Error, Result},
        new_id, now_ms,
        state::App,
    },
    storage::volumes::service::Ask,
};

/// Metadata files stay small; one bigger than this is refused rather than read into memory.
const MAX_METADATA: u64 = 16 * 1024 * 1024;
/// How long vended credentials last for a person's client.
const CLIENT_TTL: Duration = Duration::from_mins(15);

fn nope() -> Error {
    Error::NotFound("not found".into())
}

/// The catalog by the name a client gives (its warehouse), if the caller may see it.
async fn catalog(app: &App, ask: &Ask<'_>, name: &str) -> Result<Securable> {
    let (entry, _) = catalogs::resolve(app, ask.workspace, ask.org, &[name.to_owned()])
        .await
        .map_err(|_| nope())?;
    catalogs::get(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        &entry.id,
    )
    .await
    .map_err(|_| nope())
}

async fn schema(app: &App, ask: &Ask<'_>, catalog_name: &str, name: &str) -> Result<Securable> {
    let cat = catalog(app, ask, catalog_name).await?;
    let (entry, _) = catalogs::resolve(
        app,
        ask.workspace,
        ask.org,
        &[catalog_name.to_owned(), name.to_owned()],
    )
    .await
    .map_err(|_| nope())?;
    if entry.kind != Kind::Schema || entry.parent_id != cat.id {
        return Err(nope());
    }
    catalogs::get(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        &entry.id,
    )
    .await
    .map_err(|_| nope())
}

async fn table(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    namespace: &str,
    name: &str,
) -> Result<(Securable, Target)> {
    let (entry, target) = catalogs::resolve(
        app,
        ask.workspace,
        ask.org,
        &[
            catalog_name.to_owned(),
            namespace.to_owned(),
            name.to_owned(),
        ],
    )
    .await
    .map_err(|_| nope())?;
    if entry.kind != Kind::Table {
        return Err(nope());
    }
    Ok((entry, target))
}

/// The schemas of a catalog that the caller may see.
///
/// # Errors
/// The catalog is not there for this caller.
pub async fn namespaces(app: &App, ask: &Ask<'_>, catalog_name: &str) -> Result<Vec<String>> {
    let cat = catalog(app, ask, catalog_name).await?;
    Ok(catalogs::list(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        &cat.id,
    )
    .await?
    .into_iter()
    .filter(|e| e.kind == Kind::Schema)
    .map(|e| e.name)
    .collect())
}

/// # Errors
/// Not allowed, or the name is taken or not valid.
pub async fn create_namespace(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    name: &str,
) -> Result<()> {
    let cat = catalog(app, ask, catalog_name).await?;
    catalogs::create(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        Draft {
            kind: Kind::Schema,
            parent_id: cat.id,
            name: name.to_owned(),
            classification: String::new(),
            placement: String::new(),
            consistency: String::new(),
            table: None,
        },
    )
    .await
    .map(|_| ())
}

/// # Errors
/// The namespace is not there for this caller.
pub async fn namespace_exists(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    name: &str,
) -> Result<()> {
    schema(app, ask, catalog_name, name).await.map(|_| ())
}

/// The tables of a schema that the caller may see.
///
/// # Errors
/// The namespace is not there for this caller.
pub async fn tables(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    namespace: &str,
) -> Result<Vec<String>> {
    let s = schema(app, ask, catalog_name, namespace).await?;
    Ok(
        catalogs::list(app, ask.identity, ask.workspace, ask.org, ask.sides, &s.id)
            .await?
            .into_iter()
            .filter(|e| e.kind == Kind::Table)
            .map(|e| e.name)
            .collect(),
    )
}

async fn read_metadata(app: &App, id: &str, rel: &str) -> Result<Value> {
    let (mut stream, size) = app.stores.open(id, rel).await?;
    if size > MAX_METADATA {
        return Err(Error::Internal("a metadata file is too large".into()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(
            &chunk.map_err(|e| Error::Internal(format!("reading metadata: {e}")))?,
        );
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| Error::Internal(format!("a metadata file is not JSON: {e}")))
}

async fn write_metadata(app: &App, id: &str, rel: &str, meta: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(meta).map_err(|e| Error::Internal(e.to_string()))?;
    let mut up = app.stores.begin_put(id, rel, MAX_METADATA).await?;
    up.write(&bytes).await?;
    up.commit().await?;
    Ok(())
}

/// What a client is given to read a table.
pub struct Loaded {
    pub id: String,
    pub metadata_location: String,
    pub metadata: Value,
    /// Short-lived credentials for the table's folder, if this storage can issue them; and whether they may write.
    pub credentials: Option<(Credentials, bool)>,
}

/// Credentials for some tables' folders, for machines that read (or write) the data themselves.
///
/// # Errors
/// The storage has no issuer, or the provider refused.
pub async fn vend(app: &App, ids: &[&str], write: bool, ttl: Duration) -> Result<Credentials> {
    let issuer = app.issuer.as_ref().ok_or_else(|| {
        Error::Unavailable("this server's storage cannot give other machines access to data".into())
    })?;
    Ok(issuer.issue(ids, write, ttl).await?)
}

async fn vend_for_client(app: &App, id: &str, write: bool) -> Result<Option<(Credentials, bool)>> {
    if app.issuer.is_none() {
        return Ok(None);
    }
    Ok(Some((vend(app, &[id], write, CLIENT_TTL).await?, write)))
}

/// Loads a table for a client: its metadata, and credentials for its folder. Needs `SELECT`; the credentials may write only if
/// the caller has `MODIFY` too.
///
/// # Errors
/// The table is not there for this caller, or not allowed.
pub async fn load(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    namespace: &str,
    name: &str,
) -> Result<Loaded> {
    let (entry, target) = table(app, ask, catalog_name, namespace, name).await?;
    iam::require(
        app,
        ask.identity,
        ask.org,
        ask.sides,
        Privilege::Select,
        &target,
    )
    .await?;
    let may_write = iam::may(
        app,
        ask.identity,
        ask.org,
        ask.sides,
        Privilege::Modify,
        &target,
    )
    .await?;
    let metadata = read_metadata(app, &entry.id, &entry.metadata).await?;
    let credentials = if app.issuer.is_some() {
        Some((
            vend(app, &[&entry.id], may_write, CLIENT_TTL).await?,
            may_write,
        ))
    } else {
        None
    };
    audit::record(
        app,
        ask.org,
        &Who::of(ask.identity),
        "table.load",
        &target.label,
        "ok",
    )
    .await?;
    Ok(Loaded {
        metadata_location: format!("{}/{}", app.stores.uri(&entry.id)?, entry.metadata),
        id: entry.id,
        metadata,
        credentials,
    })
}

/// What a person asks for when making a table.
pub struct NewTable {
    pub name: String,
    pub schema: Value,
    pub spec: Option<Value>,
    pub order: Option<Value>,
    pub properties: Value,
    pub classification: String,
    /// The client will commit what makes the table in a second step: nothing is kept until then.
    pub stage: bool,
}

/// Makes a table: an Iceberg table with no data yet. Needs `CREATE TABLE` on the schema.
///
/// # Errors
/// Not allowed, the name is taken or not valid, or the schema is not valid.
pub async fn create(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    namespace: &str,
    new: NewTable,
) -> Result<Loaded> {
    let parent = schema(app, ask, catalog_name, namespace).await?;
    create_in(app, ask, parent, new).await
}

/// The same, into a schema found by its id (what the screens know).
///
/// # Errors
/// As [`create`]; and the id is not a schema this caller can see.
pub async fn create_under(
    app: &App,
    ask: &Ask<'_>,
    schema_id: &str,
    new: NewTable,
) -> Result<Loaded> {
    let parent = catalogs::get(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        schema_id,
    )
    .await?;
    if parent.kind != Kind::Schema {
        return Err(Error::Invalid("a table goes inside a schema".into()));
    }
    create_in(app, ask, parent, new).await
}

async fn create_in(app: &App, ask: &Ask<'_>, parent: Securable, new: NewTable) -> Result<Loaded> {
    let id = new_id();
    let location = app.stores.uri(&id)?;
    let meta = model::new_metadata(
        &location,
        &new_id(),
        new.schema,
        new.spec,
        new.order,
        &new.properties,
        now_ms(),
    )
    .map_err(Error::Invalid)?;
    if new.stage {
        let credentials = vend_for_client(app, &id, true).await?;
        return Ok(Loaded {
            metadata_location: String::new(),
            id,
            metadata: meta,
            credentials,
        });
    }
    let rel = model::next_metadata_path("", &new_id());
    write_metadata(app, &id, &rel, &meta).await?;
    let made = catalogs::create(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        Draft {
            kind: Kind::Table,
            parent_id: parent.id,
            name: new.name,
            classification: new.classification,
            placement: String::new(),
            consistency: String::new(),
            table: Some((id.clone(), rel.clone())),
        },
    )
    .await;
    if let Err(e) = made {
        // Nothing points at the file; take it away so a refused name leaves nothing behind.
        let _ = app.stores.remove_volume(&id).await;
        return Err(e);
    }
    let credentials = vend_for_client(app, &id, true).await?;
    Ok(Loaded {
        metadata_location: format!("{location}/{rel}"),
        id,
        metadata: meta,
        credentials,
    })
}

/// Commits a client's change: checks what it assumed, applies its updates, writes the next metadata file and moves the pointer.
/// Needs `MODIFY`. If another commit got there first the answer is a conflict and nothing changed; the client retries.
///
/// # Errors
/// Not allowed, the table changed, or the update is not supported.
pub async fn commit(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    namespace: &str,
    name: &str,
    requirements: &[Value],
    updates: &[Value],
) -> Result<(String, Value)> {
    if requirements.iter().any(|r| r["type"] == "assert-create") {
        return commit_new(app, ask, catalog_name, namespace, name, updates).await;
    }
    let (entry, target) = table(app, ask, catalog_name, namespace, name).await?;
    iam::require(
        app,
        ask.identity,
        ask.org,
        ask.sides,
        Privilege::Modify,
        &target,
    )
    .await?;
    let current = read_metadata(app, &entry.id, &entry.metadata).await?;
    model::check_requirements(&current, requirements).map_err(Error::Conflict)?;
    let next = model::apply(&current, updates, now_ms()).map_err(Error::Invalid)?;
    let rel = model::next_metadata_path(&entry.metadata, &new_id());
    write_metadata(app, &entry.id, &rel, &next).await?;
    let mut uow = Uow::begin(app).await?;
    if !catalogs::advance(&mut uow, ask.org, &entry.id, &entry.metadata, &rel).await? {
        drop(uow);
        let _ = app.stores.delete(&entry.id, &rel).await;
        return Err(Error::Conflict(
            "the table changed while this was committed".into(),
        ));
    }
    uow.audit(
        ask.org,
        &Who::of(ask.identity),
        "table.commit",
        &target.label,
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok((format!("{}/{rel}", app.stores.uri(&entry.id)?), next))
}

/// The second step of making a table: it must not exist yet, and the updates are everything that makes it. Needs `CREATE TABLE`.
async fn commit_new(
    app: &App,
    ask: &Ask<'_>,
    catalog_name: &str,
    namespace: &str,
    name: &str,
    updates: &[Value],
) -> Result<(String, Value)> {
    let parent = schema(app, ask, catalog_name, namespace).await?;
    if table(app, ask, catalog_name, namespace, name).await.is_ok() {
        return Err(Error::Conflict(format!("{name} already exists here")));
    }
    let meta = model::apply(&model::empty_metadata(), updates, now_ms()).map_err(Error::Invalid)?;
    model::check_schema(&meta["schemas"][0]).map_err(Error::Invalid)?;
    let base = app.stores.uri("x")?;
    let base = base.strip_suffix("/x").unwrap_or(&base);
    let location = meta["location"].as_str().unwrap_or_default();
    let id = model::id_in_location(location, base)
        .ok_or_else(|| Error::Invalid("the table's location is not one this server gave".into()))?
        .to_owned();
    if catalogs::exists(app, ask.org, &id).await? {
        return Err(Error::Conflict("that location is already a table".into()));
    }
    let rel = model::next_metadata_path("", &new_id());
    write_metadata(app, &id, &rel, &meta).await?;
    let made = catalogs::create(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        Draft {
            kind: Kind::Table,
            parent_id: parent.id,
            name: name.to_owned(),
            classification: String::new(),
            placement: String::new(),
            consistency: String::new(),
            table: Some((id.clone(), rel.clone())),
        },
    )
    .await;
    if let Err(e) = made {
        let _ = app.stores.delete(&id, &rel).await;
        return Err(e);
    }
    Ok((format!("{location}/{rel}"), meta))
}

/// What the screens show of a table: its columns and snapshots, without credentials. Needs only that the caller may see it.
///
/// # Errors
/// The table is not there for this caller.
pub async fn describe(app: &App, ask: &Ask<'_>, id: &str) -> Result<Value> {
    let entry = catalogs::get(app, ask.identity, ask.workspace, ask.org, ask.sides, id).await?;
    if entry.kind != Kind::Table {
        return Err(nope());
    }
    let meta = read_metadata(app, &entry.id, &entry.metadata).await?;
    let snapshots: Vec<Value> = meta["snapshots"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .take(50)
        .map(|s| {
            json!({
                "id": s["snapshot-id"].to_string(), "at_ms": s["timestamp-ms"],
                "operation": s["summary"]["operation"], "records": s["summary"]["total-records"],
                "files": s["summary"]["total-data-files"],
            })
        })
        .collect();
    Ok(json!({
        "format": "iceberg",
        "location": app.stores.uri(&entry.id)?,
        "columns": model::columns(&meta),
        "snapshots": snapshots,
        "current_snapshot": meta["current-snapshot-id"].to_string(),
        "properties": meta["properties"],
    }))
}

/// Where a table's current metadata is: for a machine that was given credentials to read the table itself.
///
/// # Errors
/// The table is gone.
pub async fn locate(app: &App, org: &str, id: &str) -> Result<(String, String)> {
    let (entry, _) = catalogs::target(app, org, id).await?;
    if entry.kind != Kind::Table {
        return Err(nope());
    }
    Ok((app.stores.uri(&entry.id)?, entry.metadata))
}
