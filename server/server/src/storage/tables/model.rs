//! Iceberg table metadata (format version 2): making it, checking what a commit assumed, and applying what it changes. Pure.
//!
//! Metadata is kept as JSON exactly as the Iceberg specification names it, so any Iceberg client reads what is written here.

use serde_json::{Value, json};

/// The Iceberg types a column may have.
fn valid_type(t: &Value) -> bool {
    match t {
        Value::String(s) => {
            matches!(
                s.as_str(),
                "boolean"
                    | "int"
                    | "long"
                    | "float"
                    | "double"
                    | "date"
                    | "time"
                    | "timestamp"
                    | "timestamptz"
                    | "timestamp_ns"
                    | "timestamptz_ns"
                    | "string"
                    | "uuid"
                    | "binary"
            ) || is_decimal(s)
                || is_fixed(s)
        }
        Value::Object(o) => match o.get("type").and_then(Value::as_str) {
            Some("struct") => o
                .get("fields")
                .and_then(Value::as_array)
                .is_some_and(|f| f.iter().all(valid_field)),
            Some("list") => o.get("element").is_some_and(valid_type),
            Some("map") => {
                o.get("key").is_some_and(valid_type) && o.get("value").is_some_and(valid_type)
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_decimal(s: &str) -> bool {
    s.strip_prefix("decimal(")
        .and_then(|r| r.strip_suffix(')'))
        .and_then(|r| r.split_once(','))
        .is_some_and(|(p, q)| {
            let (p, q) = (p.trim().parse::<u32>(), q.trim().parse::<u32>());
            matches!((p, q), (Ok(p), Ok(q)) if (1..=38).contains(&p) && q <= p)
        })
}

fn is_fixed(s: &str) -> bool {
    s.strip_prefix("fixed[")
        .and_then(|r| r.strip_suffix(']'))
        .is_some_and(|n| n.parse::<u32>().is_ok())
}

fn valid_field(f: &Value) -> bool {
    f["id"].as_i64().is_some_and(|i| i > 0)
        && f["name"]
            .as_str()
            .is_some_and(|n| !n.is_empty() && n.len() <= 128)
        && f["required"].is_boolean()
        && valid_type(&f["type"])
}

/// The highest field id in a schema, nested fields included.
fn max_field_id(t: &Value) -> i64 {
    match t {
        Value::Object(o) => {
            let mut m = 0;
            if let Some(fields) = o.get("fields").and_then(Value::as_array) {
                for f in fields {
                    m = m
                        .max(f["id"].as_i64().unwrap_or(0))
                        .max(max_field_id(&f["type"]));
                }
            }
            for (k, id) in [
                ("element-id", "element"),
                ("key-id", "key"),
                ("value-id", "value"),
            ] {
                if let Some(i) = o.get(k).and_then(Value::as_i64) {
                    m = m.max(i);
                }
                if let Some(inner) = o.get(id) {
                    m = m.max(max_field_id(inner));
                }
            }
            m
        }
        _ => 0,
    }
}

/// A schema a person typed in: columns of simple types, numbered in order.
///
/// # Errors
/// No columns, too many, a duplicate or empty name, or an unknown type.
pub fn schema_from_columns(columns: &[(String, String, bool)]) -> Result<Value, String> {
    if columns.is_empty() || columns.len() > 500 {
        return Err("a table has 1 to 500 columns".into());
    }
    let mut fields = Vec::new();
    for (i, (name, ty, required)) in columns.iter().enumerate() {
        if fields.iter().any(|f: &Value| f["name"] == *name) {
            return Err(format!("column {name} appears twice"));
        }
        fields.push(json!({"id": i + 1, "name": name, "required": required, "type": ty}));
    }
    let schema = json!({"type": "struct", "schema-id": 0, "fields": fields});
    check_schema(&schema)?;
    Ok(schema)
}

/// # Errors
/// A message saying what is wrong with the schema.
pub fn check_schema(schema: &Value) -> Result<(), String> {
    let fields = schema["fields"]
        .as_array()
        .filter(|f| !f.is_empty())
        .ok_or("the schema has no columns")?;
    if schema["type"] != "struct" || !fields.iter().all(valid_field) {
        return Err(
            "the schema has a column that is not valid (an id, a name, required, and a known type)"
                .into(),
        );
    }
    let mut names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err("two columns have the same name".into());
    }
    Ok(())
}

/// The metadata of a table with no data yet. `location` is the table's folder.
///
/// # Errors
/// The schema is not valid.
pub fn new_metadata(
    location: &str,
    uuid: &str,
    mut schema: Value,
    spec: Option<Value>,
    order: Option<Value>,
    properties: &Value,
    at_ms: i64,
) -> Result<Value, String> {
    check_schema(&schema)?;
    schema["schema-id"] = json!(0);
    let last_column = max_field_id(&schema);
    let mut spec = spec.unwrap_or_else(|| json!({"spec-id": 0, "fields": []}));
    spec["spec-id"] = json!(0);
    let mut order = order.unwrap_or_else(|| json!({"order-id": 0, "fields": []}));
    if order["fields"].as_array().is_none_or(Vec::is_empty) {
        order["order-id"] = json!(0);
    }
    let last_partition = spec["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["field-id"].as_i64())
        .max()
        .unwrap_or(999);
    Ok(json!({
        "format-version": 2,
        "table-uuid": uuid,
        "location": location,
        "last-sequence-number": 0,
        "last-updated-ms": at_ms,
        "last-column-id": last_column,
        "current-schema-id": 0,
        "schemas": [schema],
        "default-spec-id": 0,
        "partition-specs": [spec],
        "last-partition-id": last_partition,
        "default-sort-order-id": order["order-id"],
        "sort-orders": [order],
        "properties": properties,
        "current-snapshot-id": -1,
        "refs": {},
        "snapshots": [],
        "snapshot-log": [],
        "metadata-log": [],
    }))
}

/// What a table is before its first commit, when a client creates it in two steps (it asks for the table to be staged, then
/// commits everything that makes it): nothing yet, and every update fills it in.
#[must_use]
pub fn empty_metadata() -> Value {
    json!({
        "format-version": 2, "table-uuid": "", "location": "", "last-sequence-number": 0, "last-updated-ms": 0,
        "last-column-id": 0, "current-schema-id": -1, "schemas": [], "default-spec-id": 0, "partition-specs": [],
        "last-partition-id": 999, "default-sort-order-id": 0, "sort-orders": [], "properties": {},
        "current-snapshot-id": -1, "refs": {}, "snapshots": [], "snapshot-log": [], "metadata-log": [],
    })
}

/// The folder and the table id a staged location names: `<address of the catalog's root>/<id>`. `root_uri` is the address the store
/// gives the catalog's root folder (`root`), whichever store it is in.
#[must_use]
pub fn folder_in_location(location: &str, root_uri: &str, root: &str) -> Option<(String, String)> {
    let id = location.strip_prefix(root_uri)?.strip_prefix('/')?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .then(|| (format!("{root}/{id}"), id.to_owned()))
}

/// Checks what a commit assumed about the table still holds.
///
/// # Errors
/// What no longer holds, or an assumption this server does not know.
pub fn check_requirements(meta: &Value, requirements: &[Value]) -> Result<(), String> {
    for r in requirements {
        let kind = r["type"].as_str().unwrap_or_default();
        let held = match kind {
            "assert-create" => false,
            "assert-table-uuid" => meta["table-uuid"] == r["uuid"],
            "assert-ref-snapshot-id" => {
                let name = r["ref"].as_str().unwrap_or_default();
                let now = meta["refs"][name]["snapshot-id"].as_i64();
                let want = r["snapshot-id"].as_i64();
                now == want
            }
            "assert-last-assigned-field-id" => {
                meta["last-column-id"] == r["last-assigned-field-id"]
            }
            "assert-current-schema-id" => meta["current-schema-id"] == r["current-schema-id"],
            "assert-last-assigned-partition-id" => {
                meta["last-partition-id"] == r["last-assigned-partition-id"]
            }
            "assert-default-spec-id" => meta["default-spec-id"] == r["default-spec-id"],
            "assert-default-sort-order-id" => {
                meta["default-sort-order-id"] == r["default-sort-order-id"]
            }
            other => return Err(format!("the requirement {other} is not supported")),
        };
        if !held {
            return Err(format!("the table changed since it was read ({kind})"));
        }
    }
    Ok(())
}

fn push(meta: &mut Value, key: &str, v: Value) {
    if let Some(a) = meta[key].as_array_mut() {
        a.push(v);
    }
}

/// Applies a commit's updates to metadata and says when it was done. The caller writes the result as the next metadata file.
///
/// # Errors
/// An update this server does not support, or one that makes the metadata wrong.
#[allow(clippy::too_many_lines)] // one flat match over the update actions of the specification
pub fn apply(meta: &Value, updates: &[Value], at_ms: i64) -> Result<Value, String> {
    let mut m = meta.clone();
    let mut last_schema: Option<i64> = None;
    let mut last_spec: Option<i64> = None;
    let mut last_order: Option<i64> = None;
    for u in updates {
        let action = u["action"].as_str().unwrap_or_default();
        match action {
            "assign-uuid" => {
                if m["table-uuid"] == "" {
                    m["table-uuid"] = u["uuid"].clone();
                } else if m["table-uuid"] != u["uuid"] {
                    return Err("a table's uuid cannot change".into());
                }
            }
            "upgrade-format-version" => {
                if u["format-version"].as_i64() != Some(2) {
                    return Err("only format version 2 is supported".into());
                }
            }
            "add-schema" => {
                let mut s = u["schema"].clone();
                check_schema(&s)?;
                let id = m["schemas"].as_array().map_or(0, |a| {
                    a.iter()
                        .filter_map(|x| x["schema-id"].as_i64())
                        .max()
                        .unwrap_or(-1)
                        + 1
                });
                s["schema-id"] = json!(id);
                let top = max_field_id(&s).max(u["last-column-id"].as_i64().unwrap_or(0));
                m["last-column-id"] = json!(top.max(m["last-column-id"].as_i64().unwrap_or(0)));
                push(&mut m, "schemas", s);
                last_schema = Some(id);
            }
            "set-current-schema" => {
                let id = match u["schema-id"].as_i64() {
                    Some(-1) => last_schema.ok_or("no schema was added in this commit")?,
                    Some(i) => i,
                    None => return Err("set-current-schema needs a schema-id".into()),
                };
                if !m["schemas"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|s| s["schema-id"] == id))
                {
                    return Err(format!("there is no schema {id}"));
                }
                m["current-schema-id"] = json!(id);
            }
            "add-spec" => {
                let mut s = u["spec"].clone();
                let id = m["partition-specs"].as_array().map_or(0, |a| {
                    a.iter()
                        .filter_map(|x| x["spec-id"].as_i64())
                        .max()
                        .unwrap_or(-1)
                        + 1
                });
                s["spec-id"] = json!(id);
                let top = s["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f["field-id"].as_i64())
                    .max();
                if let Some(t) = top {
                    m["last-partition-id"] =
                        json!(t.max(m["last-partition-id"].as_i64().unwrap_or(999)));
                }
                push(&mut m, "partition-specs", s);
                last_spec = Some(id);
            }
            "set-default-spec" => {
                let id = match u["spec-id"].as_i64() {
                    Some(-1) => last_spec.ok_or("no spec was added in this commit")?,
                    Some(i) => i,
                    None => return Err("set-default-spec needs a spec-id".into()),
                };
                m["default-spec-id"] = json!(id);
            }
            "add-sort-order" => {
                let mut s = u["sort-order"].clone();
                let id = m["sort-orders"].as_array().map_or(0, |a| {
                    a.iter()
                        .filter_map(|x| x["order-id"].as_i64())
                        .max()
                        .unwrap_or(-1)
                        + 1
                });
                s["order-id"] = json!(id);
                push(&mut m, "sort-orders", s);
                last_order = Some(id);
            }
            "set-default-sort-order" => {
                let id = match u["sort-order-id"].as_i64() {
                    Some(-1) => last_order.ok_or("no sort order was added in this commit")?,
                    Some(i) => i,
                    None => return Err("set-default-sort-order needs a sort-order-id".into()),
                };
                m["default-sort-order-id"] = json!(id);
            }
            "set-location" => {
                if m["location"] == "" {
                    m["location"] = u["location"].clone();
                } else if m["location"] != u["location"] {
                    return Err("a table cannot move: its folder is fixed".into());
                }
            }
            "set-properties" => {
                let props = u["updates"]
                    .as_object()
                    .ok_or("set-properties needs updates")?;
                for (k, v) in props {
                    m["properties"][k] = v.clone();
                }
            }
            "remove-properties" => {
                for k in u["removals"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if let Some(o) = m["properties"].as_object_mut() {
                        o.remove(k);
                    }
                }
            }
            "add-snapshot" => {
                let s = u["snapshot"].clone();
                let id = s["snapshot-id"].as_i64().ok_or("a snapshot needs an id")?;
                if m["snapshots"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x["snapshot-id"] == id))
                {
                    return Err(format!("snapshot {id} is already there"));
                }
                let seq = s["sequence-number"].as_i64().unwrap_or(0);
                if seq > m["last-sequence-number"].as_i64().unwrap_or(0) {
                    m["last-sequence-number"] = json!(seq);
                }
                push(&mut m, "snapshots", s);
            }
            "set-snapshot-ref" => {
                let name = u["ref-name"].as_str().ok_or("a ref needs a name")?;
                let id = u["snapshot-id"]
                    .as_i64()
                    .ok_or("a ref needs a snapshot-id")?;
                if !m["snapshots"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x["snapshot-id"] == id))
                {
                    return Err(format!("there is no snapshot {id}"));
                }
                let mut r = u.clone();
                if let Some(o) = r.as_object_mut() {
                    o.remove("action");
                    o.remove("ref-name");
                }
                m["refs"][name] = r;
                if name == "main" {
                    m["current-snapshot-id"] = json!(id);
                    push(
                        &mut m,
                        "snapshot-log",
                        json!({"timestamp-ms": at_ms, "snapshot-id": id}),
                    );
                }
            }
            "remove-snapshot-ref" => {
                let name = u["ref-name"].as_str().ok_or("a ref needs a name")?;
                if let Some(o) = m["refs"].as_object_mut() {
                    o.remove(name);
                }
                if name == "main" {
                    m["current-snapshot-id"] = json!(-1);
                }
            }
            "remove-snapshots" => {
                let gone: Vec<i64> = u["snapshot-ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_i64)
                    .collect();
                let in_use = |v: &Value| gone.contains(&v["snapshot-id"].as_i64().unwrap_or(-2));
                if m["refs"]
                    .as_object()
                    .is_some_and(|r| r.values().any(in_use))
                {
                    return Err(
                        "a snapshot that a branch or tag points at cannot be removed".into(),
                    );
                }
                if let Some(a) = m["snapshots"].as_array_mut() {
                    a.retain(|s| !in_use(s));
                }
            }
            other => return Err(format!("the update {other} is not supported")),
        }
    }
    m["last-updated-ms"] = json!(at_ms);
    Ok(m)
}

/// Records the metadata file a commit replaces in the new metadata's `metadata-log`, as the Iceberg spec asks: newer `DuckDB` uses the
/// log to find a table as it was when a transaction began. The log keeps the last 100 versions.
pub fn log_previous(meta: &mut Value, previous_location: &str, previous_ms: i64) {
    let mut log = meta["metadata-log"].as_array().cloned().unwrap_or_default();
    log.push(json!({"timestamp-ms": previous_ms, "metadata-file": previous_location}));
    if log.len() > 100 {
        log.drain(..log.len() - 100);
    }
    meta["metadata-log"] = Value::Array(log);
}

/// The file the next version of the metadata is written to: numbered, and unique so two writers never share a name.
#[must_use]
pub fn next_metadata_path(current: &str, unique: &str) -> String {
    let n = current
        .rsplit('/')
        .next()
        .and_then(|f| f.split('-').next())
        .and_then(|n| n.parse::<u64>().ok())
        .map_or(0, |n| n + 1);
    format!("metadata/{n:05}-{unique}.metadata.json")
}

/// Where a table keeps its version hint, which every Iceberg reader that has no catalog looks for.
pub const VERSION_HINT: &str = "metadata/version-hint.text";

/// What the version hint says about the current metadata file `metadata/00004-x.metadata.json`: its name without the extension
/// (`00004-x`), which readers such as `DuckDB` turn back into the file name.
#[must_use]
pub fn version_hint(metadata: &str) -> String {
    let file = metadata.rsplit('/').next().unwrap_or(metadata);
    file.strip_suffix(".metadata.json")
        .unwrap_or(file)
        .to_owned()
}

/// The columns people see: name, type and whether it may be empty, for the current schema.
#[must_use]
pub fn columns(meta: &Value) -> Vec<Value> {
    let current = &meta["current-schema-id"];
    meta["schemas"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| &s["schema-id"] == current)
        .and_then(|s| s["fields"].as_array())
        .map(|f| {
            f.iter()
                .map(|c| json!({"id": c["id"], "name": c["name"], "type": c["type"], "required": c["required"]}))
                .collect()
        })
        .unwrap_or_default()
}

/// What the current snapshot says about the whole table, with no data read: rows, files, bytes and when it was written.
#[must_use]
pub fn table_stats(meta: &Value) -> Value {
    let current = &meta["current-snapshot-id"];
    let snapshot = meta["snapshots"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| &s["snapshot-id"] == current);
    let number = |key: &str| {
        snapshot
            .and_then(|s| s["summary"][key].as_str())
            .and_then(|v| v.parse::<i64>().ok())
    };
    json!({
        "records": number("total-records"),
        "data_files": number("total-data-files"),
        "size_bytes": number("total-files-size"),
        "at_ms": snapshot.map(|s| s["timestamp-ms"].clone()),
    })
}

/// A column bound as it compares: every value of one column decodes to the same kind.
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum Bound {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

/// A bound as people read it: dates and timestamps in words, the rest as they are.
#[must_use]
pub fn show_bound(ty: &str, b: &Bound) -> String {
    match (ty, b) {
        ("date", Bound::Int(days)) => civil(*days),
        ("timestamp" | "timestamptz" | "timestamp_ns" | "timestamptz_ns", Bound::Int(t)) => {
            let micros = if ty.ends_with("_ns") { *t / 1000 } else { *t };
            let (days, rest) = (
                micros.div_euclid(86_400_000_000),
                micros.rem_euclid(86_400_000_000),
            );
            let secs = rest / 1_000_000;
            format!(
                "{} {:02}:{:02}:{:02}",
                civil(days),
                secs / 3600,
                secs % 3600 / 60,
                secs % 60
            )
        }
        (_, Bound::Int(i)) => i.to_string(),
        (_, Bound::Float(f)) => f.to_string(),
        (_, Bound::Bool(v)) => v.to_string(),
        (_, Bound::Text(t)) => t.clone(),
    }
}

/// Days since 1970-01-01 as `YYYY-MM-DD` (the proleptic Gregorian calendar).
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Decodes a lower or upper bound as Iceberg stores it (a single value, little endian). Types whose bounds this does not read
/// (decimals, binary, nested) give None, which only means "not shown".
#[must_use]
pub fn decode_bound(ty: &str, bytes: &[u8]) -> Option<Bound> {
    match ty {
        "boolean" => bytes.first().map(|b| Bound::Bool(*b != 0)),
        "int" | "date" => <[u8; 4]>::try_from(bytes)
            .ok()
            .map(|b| Bound::Int(i64::from(i32::from_le_bytes(b)))),
        "long" | "time" | "timestamp" | "timestamptz" | "timestamp_ns" | "timestamptz_ns" => {
            <[u8; 8]>::try_from(bytes)
                .ok()
                .map(|b| Bound::Int(i64::from_le_bytes(b)))
        }
        "float" => <[u8; 4]>::try_from(bytes)
            .ok()
            .map(|b| Bound::Float(f64::from(f32::from_le_bytes(b)))),
        "double" => <[u8; 8]>::try_from(bytes)
            .ok()
            .map(|b| Bound::Float(f64::from_le_bytes(b))),
        "string" => std::str::from_utf8(bytes)
            .ok()
            .map(|t| Bound::Text(t.to_owned())),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn table() -> Value {
        let s = schema_from_columns(&[
            ("id".into(), "long".into(), true),
            ("city".into(), "string".into(), false),
        ])
        .unwrap();
        new_metadata("s3://b/t1", "u-1", s, None, None, &json!({}), 10).unwrap()
    }

    fn snapshot(id: i64, seq: i64) -> Value {
        json!({"snapshot-id": id, "sequence-number": seq, "timestamp-ms": 5, "manifest-list": "s3://b/t1/metadata/snap.avro", "summary": {"operation": "append"}})
    }

    #[test]
    fn a_new_table_has_its_columns_numbered_and_no_snapshot() {
        let m = table();
        assert_eq!(m["last-column-id"], 2);
        assert_eq!(m["current-snapshot-id"], -1);
        assert_eq!(columns(&m).len(), 2);
    }

    #[test]
    fn bad_columns_are_refused() {
        let dup = [
            ("a".into(), "int".into(), true),
            ("a".into(), "int".into(), true),
        ];
        assert!(schema_from_columns(&dup).is_err());
        assert!(schema_from_columns(&[("a".into(), "varchar".into(), true)]).is_err());
        assert!(schema_from_columns(&[]).is_err());
        assert!(schema_from_columns(&[("a".into(), "decimal(10,2)".into(), true)]).is_ok());
    }

    #[test]
    fn a_snapshot_becomes_current_when_main_points_at_it() {
        let m = table();
        let updates = [
            json!({"action": "add-snapshot", "snapshot": snapshot(7, 1)}),
            json!({"action": "set-snapshot-ref", "ref-name": "main", "type": "branch", "snapshot-id": 7}),
        ];
        let next = apply(&m, &updates, 20).unwrap();
        assert_eq!(next["current-snapshot-id"], 7);
        assert_eq!(next["last-sequence-number"], 1);
        assert_eq!(next["snapshot-log"].as_array().unwrap().len(), 1);
        assert_eq!(
            m["current-snapshot-id"], -1,
            "the old metadata is untouched"
        );
    }

    #[test]
    fn a_ref_to_a_missing_snapshot_is_refused() {
        let u = [
            json!({"action": "set-snapshot-ref", "ref-name": "main", "type": "branch", "snapshot-id": 9}),
        ];
        assert!(apply(&table(), &u, 1).is_err());
    }

    #[test]
    fn a_commit_that_read_an_older_table_is_refused() {
        let m = table();
        let after = apply(
            &m,
            &[
                json!({"action": "add-snapshot", "snapshot": snapshot(7, 1)}),
                json!({"action": "set-snapshot-ref", "ref-name": "main", "type": "branch", "snapshot-id": 7}),
            ],
            2,
        )
        .unwrap();
        let assumed_empty =
            [json!({"type": "assert-ref-snapshot-id", "ref": "main", "snapshot-id": null})];
        assert!(check_requirements(&m, &assumed_empty).is_ok());
        assert!(check_requirements(&after, &assumed_empty).is_err());
        assert!(check_requirements(&m, &[json!({"type": "assert-create"})]).is_err());
        assert!(
            check_requirements(&m, &[json!({"type": "assert-table-uuid", "uuid": "u-1"})]).is_ok()
        );
        assert!(check_requirements(&m, &[json!({"type": "assert-mystery"})]).is_err());
    }

    #[test]
    fn schemas_evolve_and_a_table_cannot_move() {
        let m = table();
        let more = json!({"type": "struct", "schema-id": 1, "fields": [
            {"id": 1, "name": "id", "required": true, "type": "long"},
            {"id": 2, "name": "city", "required": false, "type": "string"},
            {"id": 3, "name": "pop", "required": false, "type": "long"}]});
        let next = apply(
            &m,
            &[
                json!({"action": "add-schema", "schema": more}),
                json!({"action": "set-current-schema", "schema-id": -1}),
            ],
            3,
        )
        .unwrap();
        assert_eq!(next["current-schema-id"], 1);
        assert_eq!(next["last-column-id"], 3);
        assert!(
            apply(
                &m,
                &[json!({"action": "set-location", "location": "s3://elsewhere"})],
                3
            )
            .is_err()
        );
        assert!(apply(&m, &[json!({"action": "mystery"})], 3).is_err());
    }

    #[test]
    fn a_table_made_in_two_steps_is_filled_in_by_its_commit() {
        let schema = json!({"type": "struct", "schema-id": 0, "fields": [{"id": 1, "name": "a", "required": false, "type": "long"}]});
        let updates = [
            json!({"action": "assign-uuid", "uuid": "u-9"}),
            json!({"action": "upgrade-format-version", "format-version": 2}),
            json!({"action": "add-schema", "schema": schema, "last-column-id": 1}),
            json!({"action": "set-current-schema", "schema-id": -1}),
            json!({"action": "add-spec", "spec": {"fields": []}}),
            json!({"action": "set-default-spec", "spec-id": -1}),
            json!({"action": "add-sort-order", "sort-order": {"fields": []}}),
            json!({"action": "set-default-sort-order", "sort-order-id": -1}),
            json!({"action": "set-location", "location": "s3://b/t9"}),
        ];
        let m = apply(&empty_metadata(), &updates, 5).unwrap();
        assert_eq!(
            (m["table-uuid"].as_str(), m["location"].as_str()),
            (Some("u-9"), Some("s3://b/t9"))
        );
        assert_eq!(
            (m["current-schema-id"].clone(), m["last-column-id"].clone()),
            (json!(0), json!(1))
        );
        assert_eq!(columns(&m).len(), 1);
    }

    #[test]
    fn a_staged_location_must_be_ours() {
        let found = |l, r: &str| folder_in_location(l, &format!("s3://b/{r}"), r);
        assert_eq!(
            found("s3://b/ws/cat/abc-1", "ws/cat"),
            Some(("ws/cat/abc-1".into(), "abc-1".into()))
        );
        assert_eq!(
            found("s3://b/ws/other/abc", "ws/cat"),
            None,
            "another catalog's root"
        );
        assert_eq!(found("s3://b/ws/cat/a/b", "ws/cat"), None, "one level only");
        assert_eq!(found("s3://b/ws/cat/", "ws/cat"), None);
        assert_eq!(found("s3://b/abc", "ws/cat"), None, "not under the root");
        assert_eq!(
            folder_in_location("s3://other/ws/cat/abc", "s3://b/ws/cat", "ws/cat"),
            None
        );
        // On a customer's location the address has the location's own folder, and the folder the marker.
        assert_eq!(
            folder_in_location("s3://b/pre/cat/abc", "s3://b/pre/cat", "loc-1/cat"),
            Some(("loc-1/cat/abc".into(), "abc".into()))
        );
    }

    #[test]
    fn the_version_hint_names_the_metadata_file_without_its_extension() {
        assert_eq!(
            version_hint("metadata/00004-abc.metadata.json"),
            "00004-abc"
        );
        assert_eq!(version_hint("00000-x.metadata.json"), "00000-x");
    }

    #[test]
    fn metadata_files_are_numbered_on() {
        assert_eq!(
            next_metadata_path("metadata/00003-abc.metadata.json", "x"),
            "metadata/00004-x.metadata.json"
        );
        assert_eq!(
            next_metadata_path("", "x"),
            "metadata/00000-x.metadata.json"
        );
    }

    #[test]
    fn bounds_are_decoded_by_their_type_and_shown_in_words() {
        assert_eq!(
            decode_bound("long", &120_i64.to_le_bytes()),
            Some(Bound::Int(120))
        );
        assert_eq!(
            decode_bound("int", &(-3_i32).to_le_bytes()),
            Some(Bound::Int(-3))
        );
        assert_eq!(
            decode_bound("string", b"north"),
            Some(Bound::Text("north".into()))
        );
        assert_eq!(
            decode_bound("double", &1.5_f64.to_le_bytes()),
            Some(Bound::Float(1.5))
        );
        assert_eq!(
            decode_bound("long", &[1, 2]),
            None,
            "a bound of the wrong size is not guessed at"
        );
        assert_eq!(decode_bound("decimal(10,2)", &[0]), None);
        assert_eq!(show_bound("date", &Bound::Int(0)), "1970-01-01");
        assert_eq!(show_bound("date", &Bound::Int(20_000)), "2024-10-04");
        assert_eq!(
            show_bound("timestamp", &Bound::Int(86_400_000_000 + 3_661_000_000)),
            "1970-01-02 01:01:01"
        );
        assert!(
            Bound::Int(2) < Bound::Int(10) && Bound::Text("a".into()) < Bound::Text("b".into())
        );
    }

    #[test]
    fn the_whole_table_is_described_from_the_current_snapshot_alone() {
        let m = json!({"current-snapshot-id": 7, "snapshots": [
            {"snapshot-id": 6, "timestamp-ms": 1, "summary": {"total-records": "1"}},
            {"snapshot-id": 7, "timestamp-ms": 9, "summary": {"total-records": "3", "total-data-files": "1", "total-files-size": "800"}}]});
        let s = table_stats(&m);
        assert_eq!(
            (
                s["records"].as_i64(),
                s["data_files"].as_i64(),
                s["size_bytes"].as_i64(),
                s["at_ms"].as_i64()
            ),
            (Some(3), Some(1), Some(800), Some(9))
        );
        assert!(
            table_stats(&json!({"current-snapshot-id": -1, "snapshots": []}))["records"].is_null()
        );
    }

    #[test]
    fn a_commit_remembers_the_file_it_replaces() {
        let mut m = empty_metadata();
        log_previous(&mut m, "s3://b/t/metadata/0-a.metadata.json", 5);
        log_previous(&mut m, "s3://b/t/metadata/1-b.metadata.json", 9);
        let log = m["metadata-log"].as_array().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(
            log[1]["metadata-file"],
            "s3://b/t/metadata/1-b.metadata.json"
        );
        for i in 0..150 {
            log_previous(&mut m, &format!("f{i}"), i);
        }
        assert_eq!(m["metadata-log"].as_array().unwrap().len(), 100);
    }
}
