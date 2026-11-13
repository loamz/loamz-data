//! The Iceberg catalog: namespaces are schemas, tables are Iceberg tables. Every call finds the thing, asks IAM, then acts.
//!
//! The bytes of a table live in its own folder of the store. This service writes only the table's metadata files and moves its
//! pointer; the data and manifest files are written by the client with short-lived credentials limited to that folder.

use std::time::Duration;

use futures_util::StreamExt;
use loamz_lake::grants::Credentials;
use serde_json::{Value, json};

use super::{manifests, model};
use crate::{
    governance::{
        audit::{model::Who, service as audit, service::Uow},
        catalogs::{
            model::{Draft, Kind, Securable, join_folder},
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
/// The most manifests one profile will read.
const MAX_MANIFESTS: usize = 200;
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
            location: None,
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

/// A metadata file's bytes. They are named by a unique id and never change, so each is read from the bucket once.
async fn read_bytes(app: &App, id: &str, rel: &str) -> Result<std::sync::Arc<Vec<u8>>> {
    let key = format!("{id}/{rel}");
    if let Some(hit) = app.metadata_cache.lock().ok().and_then(|c| c.get(&key)) {
        return Ok(hit);
    }
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
    let bytes = std::sync::Arc::new(bytes);
    if let Ok(mut c) = app.metadata_cache.lock() {
        c.put(key, bytes.clone());
    }
    Ok(bytes)
}

async fn read_metadata(app: &App, id: &str, rel: &str) -> Result<Value> {
    serde_json::from_slice(&read_bytes(app, id, rel).await?)
        .map_err(|e| Error::Internal(format!("a metadata file is not JSON: {e}")))
}

async fn write_metadata(app: &App, id: &str, rel: &str, meta: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(meta).map_err(|e| Error::Internal(e.to_string()))?;
    let mut up = app.stores.begin_put(id, rel, MAX_METADATA).await?;
    up.write(&bytes).await?;
    up.commit().await?;
    Ok(())
}

/// Points the table's version hint at its current metadata file, so any Iceberg reader can find the latest snapshot straight from
/// the bucket with no Loamz server. The commit is already durable, so a failure here is logged: the hint then names an older
/// snapshot, which is still a valid one, until the next commit.
async fn publish_hint(app: &App, id: &str, rel: &str) {
    let hint = model::version_hint(rel);
    let put = async {
        let mut up = app
            .stores
            .begin_put(id, model::VERSION_HINT, MAX_METADATA)
            .await?;
        up.write(hint.as_bytes()).await?;
        up.commit().await
    };
    if let Err(e) = put.await {
        tracing::warn!(table = id, error = ?e, "could not write the table's version hint");
    }
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
    // The same folders and access, asked for again while the credentials still last nearly as long as wanted (within a minute): reuse
    // them. A run asks for its time limit plus five minutes, so reused credentials still outlast the run.
    let mut key: Vec<&str> = ids.to_vec();
    key.sort_unstable();
    let key = format!("{}|{write}", key.join(","));
    let now = std::time::Instant::now();
    if let Ok(cache) = app.credential_cache.lock()
        && let Some((c, until)) = cache.get(&key)
        && until.saturating_duration_since(now) >= ttl.saturating_sub(Duration::from_mins(1))
    {
        return Ok(c.clone());
    }
    let fresh = issuer.issue(ids, write, ttl).await?;
    if let Ok(mut cache) = app.credential_cache.lock() {
        // What the store honours is at least a minute and at most an hour; count on the least it can be.
        cache.retain(|_, (_, until)| *until > now);
        cache.insert(
            key,
            (
                fresh.clone(),
                now + ttl.clamp(Duration::from_mins(1), Duration::from_hours(1)),
            ),
        );
    }
    Ok(fresh)
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
    // Credentials would hand over the raw files, round the table's rules: those are read through SQL on Loamz compute only.
    if catalogs::is_restricted(app, ask.identity, ask.org, ask.sides, &entry.id).await? {
        return Err(Error::Invalid(format!(
            "{} has row or column rules, so it can only be read with SQL run on Loamz",
            entry.name
        )));
    }
    let may_write = iam::may(
        app,
        ask.identity,
        ask.org,
        ask.sides,
        Privilege::Modify,
        &target,
    )
    .await?;
    let metadata = read_metadata(app, &entry.folder, &entry.metadata).await?;
    let credentials = if app.issuer.is_some() {
        Some((
            vend(app, &[&entry.folder], may_write, CLIENT_TTL).await?,
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
        metadata_location: format!("{}/{}", app.stores.uri(&entry.folder)?, entry.metadata),
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
    // The table's folder is under its catalog's root, in the workspace's own prefix.
    let folder = join_folder(&parent.folder, &id);
    let location = app.stores.uri(&folder)?;
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
        let credentials = vend_for_client(app, &folder, true).await?;
        return Ok(Loaded {
            metadata_location: String::new(),
            id,
            metadata: meta,
            credentials,
        });
    }
    let rel = model::next_metadata_path("", &new_id());
    write_metadata(app, &folder, &rel, &meta).await?;
    let made = catalogs::create(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        Draft {
            location: None,
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
        let _ = app.stores.remove_volume(&folder).await;
        return Err(e);
    }
    publish_hint(app, &folder, &rel).await;
    let credentials = vend_for_client(app, &folder, true).await?;
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
    let current = read_metadata(app, &entry.folder, &entry.metadata).await?;
    model::check_requirements(&current, requirements).map_err(Error::Conflict)?;
    let mut next = model::apply(&current, updates, now_ms()).map_err(Error::Invalid)?;
    model::log_previous(
        &mut next,
        &format!("{}/{}", app.stores.uri(&entry.folder)?, entry.metadata),
        current["last-updated-ms"].as_i64().unwrap_or(0),
    );
    let rel = model::next_metadata_path(&entry.metadata, &new_id());
    write_metadata(app, &entry.folder, &rel, &next).await?;
    let mut uow = Uow::begin(app).await?;
    if !catalogs::advance(&mut uow, ask.org, &entry.id, &entry.metadata, &rel).await? {
        drop(uow);
        let _ = app.stores.delete(&entry.folder, &rel).await;
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
    publish_hint(app, &entry.folder, &rel).await;
    Ok((format!("{}/{rel}", app.stores.uri(&entry.folder)?), next))
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
    let (folder, id) = model::folder_in_location(location, base, &parent.folder)
        .ok_or_else(|| Error::Invalid("the table's location is not one this server gave".into()))?;
    if catalogs::exists(app, ask.org, &id).await? {
        return Err(Error::Conflict("that location is already a table".into()));
    }
    let rel = model::next_metadata_path("", &new_id());
    write_metadata(app, &folder, &rel, &meta).await?;
    let made = catalogs::create(
        app,
        ask.identity,
        ask.workspace,
        ask.org,
        ask.sides,
        Draft {
            location: None,
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
        let _ = app.stores.delete(&folder, &rel).await;
        return Err(e);
    }
    publish_hint(app, &folder, &rel).await;
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
    let meta = read_metadata(app, &entry.folder, &entry.metadata).await?;
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
        "location": app.stores.uri(&entry.folder)?,
        "stats": model::table_stats(&meta),
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
pub async fn locate(app: &App, org: &str, id: &str) -> Result<(String, String, String)> {
    let (entry, _) = catalogs::target(app, org, id).await?;
    if entry.kind != Kind::Table {
        return Err(nope());
    }
    Ok((app.stores.uri(&entry.folder)?, entry.metadata, entry.folder))
}

/// Per-column counts and ranges of a table, from what its files recorded about themselves: no data is read and no compute is
/// used. Needs `SELECT`, because a range is a fact about the data.
///
/// # Errors
/// Not allowed, or the table's metadata cannot be read.
pub async fn column_stats(app: &App, ask: &Ask<'_>, id: &str) -> Result<Value> {
    let (entry, target) = catalogs::target_in(app, ask.workspace, ask.org, id).await?;
    if entry.kind != Kind::Table {
        return Err(nope());
    }
    iam::require(
        app,
        ask.identity,
        ask.org,
        ask.sides,
        Privilege::Select,
        &target,
    )
    .await?;
    // A range is a fact about the data the rules may hide.
    if catalogs::is_restricted(app, ask.identity, ask.org, ask.sides, &entry.id).await? {
        return Err(Error::Invalid(
            "this table has row or column rules, so its value ranges are not shown".into(),
        ));
    }
    let meta = read_metadata(app, &entry.folder, &entry.metadata).await?;
    let current = &meta["current-snapshot-id"];
    let list = meta["snapshots"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| &s["snapshot-id"] == current)
        .and_then(|s| s["manifest-list"].as_str());
    let base = format!("{}/", app.stores.uri(&entry.folder)?);
    let rel = |path: &str| {
        path.strip_prefix(&base)
            .map(str::to_owned)
            .ok_or_else(|| Error::Internal("a manifest is outside the table's own folder".into()))
    };
    let mut files = Vec::new();
    if let Some(list) = list {
        let paths = manifests::manifest_paths(&read_bytes(app, &entry.folder, &rel(list)?).await?)
            .map_err(Error::Internal)?;
        if paths.len() > MAX_MANIFESTS {
            return Err(Error::Invalid(
                "this table has too many manifests to profile from its metadata".into(),
            ));
        }
        let reads = paths.iter().map(|path| async {
            let bytes = read_bytes(app, &entry.folder, &rel(path)?).await?;
            manifests::data_files(&bytes).map_err(Error::Internal)
        });
        for found in futures_util::future::join_all(reads).await {
            files.extend(found?);
        }
    }
    let schema = meta["schemas"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["schema-id"] == meta["current-schema-id"]);
    let columns: Vec<(i64, String, String)> = schema
        .and_then(|s| s["fields"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some((
                f["id"].as_i64()?,
                f["name"].as_str()?.to_owned(),
                f["type"].as_str()?.to_owned(),
            ))
        })
        .collect();
    let stats = manifests::combine(&files, &columns);
    Ok(json!({
        "records": files.iter().map(|f| f.records).sum::<i64>(),
        "data_files": files.len(),
        "columns": stats.iter().map(|c| json!({"id": c.id, "name": c.name, "values": c.values, "nulls": c.nulls, "min": c.min, "max": c.max})).collect::<Vec<_>>(),
    }))
}
