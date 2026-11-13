//! A single-run DuckDB connection with an immutable, scoped filesystem policy.

use duckdb::{Connection, Result};

/// The connection is private so callers cannot run a statement before lockdown.
pub struct Engine {
    connection: Connection,
}

impl Engine {
    /// Create a fresh connection and lock down external access before it is returned.
    /// `allowed_paths` must come from an authorized lease, never from user SQL.
    pub fn start(allowed_paths: &[String]) -> Result<Self> {
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
        let paths = allowed_paths
            .iter()
            .map(|path| format!("'{}'", path.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        connection.execute_batch(&format!(
            "SET autoinstall_known_extensions = false; SET autoload_known_extensions = false; \
             SET allow_persistent_secrets = false; SET allowed_directories = [{paths}]; \
             SET enable_external_access = false; SET lock_configuration = true;"
        ))?;
        Ok(Self { connection })
    }

    /// Execute one statement with the connection's immutable access policy.
    pub fn execute(&self, sql: &str) -> Result<()> {
        self.connection.execute_batch(sql)
    }
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
}
