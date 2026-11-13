//! A single-run `DuckDB` connection with an immutable, scoped filesystem policy.

use duckdb::{Connection, Result};
use serde_json::{Value as Json, json};

/// The connection is private so callers cannot run a statement before lockdown.
pub struct Engine {
    connection: Connection,
}

impl Engine {
    /// Create a fresh connection and lock down external access before it is returned.
    /// `allowed_paths` must come from an authorized lease, never from user SQL.
    pub fn start(allowed_paths: &[String]) -> Result<Self> {
        Self::start_for_run(allowed_paths, |_| Ok(()))
    }

    /// Prepare run-specific settings and temporary secrets before locking this new connection.
    pub fn start_for_run(
        allowed_paths: &[String],
        prepare: impl FnOnce(&Connection) -> Result<()>,
    ) -> Result<Self> {
        for path in allowed_paths {
            let valid = path.strip_prefix("s3://").is_some_and(|rest| {
                rest.ends_with('/')
                    && rest.split('/').filter(|part| !part.is_empty()).count() >= 2
                    && !rest.contains(['?', '#', '\\', '\'', '%'])
                    && !rest.split('/').any(|part| matches!(part, "." | ".."))
            });
            if !valid {
                return Err(duckdb::Error::InvalidParameterName(
                    "invalid authorized storage prefix".into(),
                ));
            }
        }
        let connection = Connection::open_in_memory()?;
        connection.execute_batch("SET allow_persistent_secrets = false")?;
        prepare(&connection)?;
        let paths = allowed_paths
            .iter()
            .map(|path| format!("'{}'", path.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        connection.execute_batch(&format!(
            "SET autoinstall_known_extensions = false; SET autoload_known_extensions = false; \
             SET allowed_directories = [{paths}]; \
             SET enable_external_access = false; SET lock_configuration = true;"
        ))?;
        Ok(Self { connection })
    }

    /// Execute one statement with the connection's immutable access policy.
    pub fn execute(&self, sql: &str) -> Result<()> {
        self.connection.execute_batch(sql)
    }

    /// Execute a read-only statement and return bounded JSON rows.
    pub fn answer(&self, sql: &str, limit: usize) -> Result<Json> {
        answer(&self.connection, sql, limit)
    }
}

/// Execute a read-only statement on a connection and return bounded JSON rows.
fn answer(connection: &Connection, sql: &str, limit: usize) -> Result<Json> {
    {
        let sql = sql.trim().trim_end_matches(';');
        let query = format!(
            "SELECT * FROM ({sql}) AS loamz_result LIMIT {}",
            limit.saturating_add(1)
        );
        let mut stmt = connection.prepare(&query)?;
        let mut cursor = stmt.query([])?;
        let statement = cursor.as_ref().ok_or(duckdb::Error::InvalidQuery)?;
        let names = statement.column_names();
        let columns: Vec<Json> = names
            .iter()
            .enumerate()
            .map(|(i, name)| json!({"name": name, "type": statement.column_type(i).to_string()}))
            .collect();
        let mut rows = Vec::new();
        let mut size = 0_usize;
        let mut truncated = false;
        while let Some(row) = cursor.next()? {
            if rows.len() == limit {
                truncated = true;
                break;
            }
            let values = (0..names.len())
                .map(|i| row.get::<_, duckdb::types::Value>(i).and_then(to_json))
                .collect::<Result<Vec<_>>>()?;
            size = size.saturating_add(serde_json::to_vec(&values).map_or(usize::MAX, |v| v.len()));
            if size > 3_500_000 {
                truncated = true;
                break;
            }
            rows.push(values);
        }
        Ok(json!({"columns": columns, "rows": rows, "truncated": truncated}))
    }
}

/// What a session is started for: if the next statement needs something different, the worker starts a new one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionKey {
    /// The one bucket its tables are in (none: a statement that reads no table).
    pub bucket: Option<String>,
    pub memory_mb: u64,
    pub cpu_millis: u64,
}

/// A table a statement reads: its three-part name, where it is, and which metadata version to read.
pub struct Table {
    pub catalog: String,
    pub schema: String,
    pub name: String,
    pub uri: String,
    pub version: String,
}

/// How to reach storage for one statement: the broker's address inside the sandbox, and the lease's credentials.
pub struct Access {
    pub key_id: String,
    pub secret: String,
    pub session_token: String,
    pub endpoint: String,
}

/// The outcome of one statement. `clean` is false when its temporary objects could not all be removed: the worker must then stop,
/// because the next statement could see them.
pub struct Done {
    pub result: Result<Json>,
    pub clean: bool,
    pub prepare_ms: u64,
    pub run_ms: u64,
}

/// One `DuckDB` kept for the life of the worker: its extensions are loaded once and it is locked down once. Every statement runs on its
/// own connection with its own temporary objects (a secret, attached catalogs, views), all removed afterwards. The worker serves one
/// organization and one statement at a time, and the storage broker, not `DuckDB`, decides which folders a statement may read.
pub struct Session {
    db: Connection,
    key: SessionKey,
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn millis(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl Session {
    #[must_use]
    pub fn key(&self) -> &SessionKey {
        &self.key
    }

    /// Opens the engine and locks it down for the bucket named in `key`.
    ///
    /// # Errors
    /// The bucket name is not valid, or the engine could not be set up.
    pub fn start(key: SessionKey) -> Result<Self> {
        let db = Connection::open_in_memory()?;
        let threads = key.cpu_millis.div_ceil(1000).max(1);
        db.execute_batch(&format!(
            "SET allow_persistent_secrets = false; SET threads = {threads}; SET memory_limit = '{}MB'; SET enable_http_metadata_cache = true",
            key.memory_mb * 7 / 10
        ))?;
        let mut allowed = String::new();
        if let Some(bucket) = &key.bucket {
            let valid = !bucket.is_empty()
                && bucket
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
            if !valid {
                return Err(duckdb::Error::InvalidParameterName("invalid bucket".into()));
            }
            db.execute_batch(
                "SET extension_directory = '/opt/duckdb-extensions'; LOAD httpfs; LOAD iceberg",
            )?;
            allowed = literal(&format!("s3://{bucket}/"));
        }
        db.execute_batch(&format!(
            "SET autoinstall_known_extensions = false; SET autoload_known_extensions = false; \
             SET allowed_directories = [{allowed}]; SET enable_external_access = false; SET lock_configuration = true;"
        ))?;
        Ok(Self { db, key })
    }

    /// Runs one statement on a fresh connection, then removes everything it made.
    pub fn statement(
        &self,
        tables: &[Table],
        access: Option<&Access>,
        sql: &str,
        limit: usize,
    ) -> Done {
        let began = std::time::Instant::now();
        let mut prepared = began.elapsed();
        let mut undo: Vec<String> = Vec::new();
        let conn = match self.db.try_clone() {
            Ok(c) => c,
            Err(e) => {
                return Done {
                    result: Err(e),
                    clean: false,
                    prepare_ms: 0,
                    run_ms: 0,
                };
            }
        };
        let result = Self::prepare(&conn, tables, access, &mut undo).and_then(|()| {
            prepared = began.elapsed();
            answer(&conn, sql, limit)
        });
        let run = began.elapsed().saturating_sub(prepared);
        // Always undo, last made first. A failure here means something may be left behind.
        let clean = undo.iter().rev().all(|s| conn.execute_batch(s).is_ok());
        Done {
            result,
            clean,
            prepare_ms: millis(prepared),
            run_ms: millis(run),
        }
    }

    fn prepare(
        conn: &Connection,
        tables: &[Table],
        access: Option<&Access>,
        undo: &mut Vec<String>,
    ) -> Result<()> {
        if tables.is_empty() {
            return Ok(());
        }
        let access = access
            .ok_or_else(|| duckdb::Error::InvalidParameterName("no storage access".into()))?;
        // CREATE SECRET cannot take parameters, so the values are written as quoted text.
        conn.execute_batch(&format!(
            "CREATE TEMP SECRET loamz_run (TYPE s3, KEY_ID {}, SECRET {}, SESSION_TOKEN {}, ENDPOINT {}, URL_STYLE 'path', REGION 'auto', USE_SSL false)",
            literal(&access.key_id),
            literal(&access.secret),
            literal(&access.session_token),
            literal(&access.endpoint)
        ))?;
        undo.push("DROP SECRET loamz_run".into());
        let mut catalogs = std::collections::HashSet::new();
        let mut schemas = std::collections::HashSet::new();
        for t in tables {
            if catalogs.insert(t.catalog.clone()) {
                conn.execute_batch(&format!("ATTACH ':memory:' AS {}", quote(&t.catalog)))?;
                undo.push(format!("DETACH {}", quote(&t.catalog)));
            }
            if schemas.insert((t.catalog.clone(), t.schema.clone())) {
                conn.execute_batch(&format!(
                    "CREATE SCHEMA IF NOT EXISTS {}.{}",
                    quote(&t.catalog),
                    quote(&t.schema)
                ))?;
            }
            conn.execute_batch(&format!(
                "CREATE VIEW {}.{}.{} AS SELECT * FROM iceberg_scan({}, version = {})",
                quote(&t.catalog),
                quote(&t.schema),
                quote(&t.name),
                literal(&t.uri),
                literal(&t.version)
            ))?;
        }
        Ok(())
    }
}

fn to_json(value: duckdb::types::Value) -> Result<Json> {
    use duckdb::types::Value;
    const SAFE: i64 = 1_i64 << 53;
    Ok(match value {
        Value::Null => Json::Null,
        Value::Boolean(v) => json!(v),
        Value::TinyInt(v) => json!(v),
        Value::SmallInt(v) => json!(v),
        Value::Int(v) => json!(v),
        Value::BigInt(v) if (-SAFE..SAFE).contains(&v) => json!(v),
        Value::BigInt(v) => json!(v.to_string()),
        Value::HugeInt(v) => json!(v.to_string()),
        Value::UHugeInt(v) => json!(v.to_string()),
        Value::UTinyInt(v) => json!(v),
        Value::USmallInt(v) => json!(v),
        Value::UInt(v) => json!(v),
        Value::UBigInt(v) if v < SAFE as u64 => json!(v),
        Value::UBigInt(v) => json!(v.to_string()),
        Value::Float(v) if v.is_finite() => json!(v),
        Value::Float(v) => json!(v.to_string()),
        Value::Double(v) if v.is_finite() => json!(v),
        Value::Double(v) => json!(v.to_string()),
        Value::Decimal(v) => json!(v.to_string()),
        Value::Text(v) | Value::Enum(v) => json!(v),
        Value::Blob(v) => {
            use std::fmt::Write;
            let mut text = String::with_capacity(v.len().saturating_mul(2));
            for byte in v {
                write!(text, "{byte:02x}").map_err(|_| duckdb::Error::InvalidQuery)?;
            }
            json!(text)
        }
        Value::Timestamp(unit, value) => {
            let (seconds, nanos) = match unit {
                duckdb::types::TimeUnit::Second => (value, 0),
                duckdb::types::TimeUnit::Millisecond => (
                    value.div_euclid(1_000),
                    u32::try_from(value.rem_euclid(1_000) * 1_000_000)
                        .map_err(|_| duckdb::Error::InvalidQuery)?,
                ),
                duckdb::types::TimeUnit::Microsecond => (
                    value.div_euclid(1_000_000),
                    u32::try_from(value.rem_euclid(1_000_000) * 1_000)
                        .map_err(|_| duckdb::Error::InvalidQuery)?,
                ),
                duckdb::types::TimeUnit::Nanosecond => (
                    value.div_euclid(1_000_000_000),
                    u32::try_from(value.rem_euclid(1_000_000_000))
                        .map_err(|_| duckdb::Error::InvalidQuery)?,
                ),
            };
            let timestamp = chrono::DateTime::from_timestamp(seconds, nanos)
                .ok_or(duckdb::Error::InvalidQuery)?;
            json!(
                timestamp
                    .naive_utc()
                    .format("%Y-%m-%dT%H:%M:%S%.f")
                    .to_string()
            )
        }
        Value::Date32(days) => {
            let epoch =
                chrono::NaiveDate::from_ymd_opt(1970, 1, 1).ok_or(duckdb::Error::InvalidQuery)?;
            let date = epoch
                .checked_add_signed(chrono::Duration::days(i64::from(days)))
                .ok_or(duckdb::Error::InvalidQuery)?;
            json!(date.to_string())
        }
        Value::List(v) | Value::Array(v) => {
            Json::Array(v.into_iter().map(to_json).collect::<Result<Vec<_>>>()?)
        }
        Value::Union(v) => to_json(*v)?,
        _ => {
            return Err(duckdb::Error::InvalidParameterName(
                "unsupported result type".into(),
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::Engine;

    #[test]
    fn lockdown_denies_host_files_and_cannot_be_changed() {
        let engine = Engine::start(&[]).unwrap();
        assert!(
            engine
                .execute("SELECT * FROM read_csv('/etc/passwd')")
                .is_err()
        );
        assert!(engine.execute("SET lock_configuration = false").is_err());
        assert!(engine.execute("SET enable_external_access = true").is_err());
        assert!(engine.execute("SET allowed_directories = ['/']").is_err());
        assert!(engine.execute("INSTALL httpfs").is_err());
        engine.execute("SELECT 1").unwrap();
    }

    #[test]
    fn local_or_broad_paths_are_never_authorized() {
        for path in [
            "/",
            "/etc/",
            "s3://bucket/",
            "s3://bucket/a/../",
            "s3://bucket/a/%2e%2e/",
        ] {
            assert!(Engine::start(&[path.to_string()]).is_err(), "{path}");
        }
        assert!(Engine::start(&["s3://bucket/a/".into()]).is_ok());
    }

    #[test]
    fn fresh_run_does_not_see_another_runs_temporary_data_or_secret() {
        let first = Engine::start_for_run(&[], |conn| {
            conn.execute_batch("CREATE TEMP TABLE prior_run (n INTEGER); CREATE TEMP SECRET prior_secret (TYPE http, EXTRA_HTTP_HEADERS MAP {'Authorization': 'test'});")
        }).unwrap();
        let second = Engine::start(&[]).unwrap();
        let present: i64 = first
            .connection
            .query_row(
                "SELECT count(*) FROM duckdb_secrets() WHERE name = 'prior_secret'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let absent: i64 = second
            .connection
            .query_row(
                "SELECT count(*) FROM duckdb_secrets() WHERE name = 'prior_secret'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((present, absent), (1, 0));
        assert!(first.execute("SELECT * FROM prior_run").is_ok());
        assert!(second.execute("SELECT * FROM prior_run").is_err());
    }

    #[test]
    fn query_results_are_bounded() {
        let engine = Engine::start(&[]).unwrap();
        let result = engine.answer("SELECT * FROM range(3)", 2).unwrap();
        assert_eq!(result["rows"], serde_json::json!([[0], [1]]));
        assert_eq!(result["truncated"], true);
    }
}
