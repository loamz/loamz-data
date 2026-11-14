//! A long-lived sandbox process: one `DuckDB` for its whole life, one connection per request.

use std::io::{self, BufRead, Write};

use loamz_worker::{Access, Export, Session, SessionKey, Table};
use serde_json::{Value, json};

mod proxy;

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value[key].as_str().ok_or("the work request is incomplete")
}

/// The first part of an engine's message: enough to say what went wrong, never a whole page.
fn short(text: &str) -> String {
    text.chars().take(300).collect()
}

/// What one request asks for, checked.
struct Request {
    sql: String,
    limit: usize,
    tables: Vec<Table>,
    access: Option<Access>,
    key: SessionKey,
    /// The whole result as an Arrow file, when the statement asked for it.
    export: Option<Export>,
}

fn parse(request: &Value, proxy_port: u16) -> Result<Request, String> {
    let spec = &request["spec"];
    let sql = text(spec, "sql")?.to_owned();
    let limit =
        usize::try_from(spec["limit"].as_u64().unwrap_or(1000).min(1_000_000)).unwrap_or(1000);
    let listed = spec["tables"]
        .as_array()
        .ok_or("the work request has no tables")?;
    let storage = &request["storage"];
    let memory_mb = request["limits"]["memory_mb"].as_u64().unwrap_or(0);
    let cpu_millis = request["limits"]["cpu_millis"].as_u64().unwrap_or(0);
    if !(64..=8192).contains(&memory_mb) || !(1..=8000).contains(&cpu_millis) {
        return Err("invalid resource limits".into());
    }
    let mut tables = Vec::new();
    for table in listed {
        let id = text(table, "id")?;
        let name = text(table, "name")?;
        let parts: Vec<_> = name.split('.').collect();
        if parts.len() != 3 || parts.iter().any(|s| s.is_empty()) {
            return Err("invalid table name".into());
        }
        let entry = &storage["tables"][id];
        let uri = text(entry, "uri")?;
        let metadata = text(entry, "metadata")?;
        let version = metadata
            .strip_prefix("metadata/")
            .and_then(|v| v.strip_suffix(".metadata.json"))
            .filter(|v| !v.contains('/'))
            .ok_or("invalid metadata path")?;
        let bucket = text(storage, "bucket")?;
        if !uri.starts_with(&format!("s3://{bucket}/")) {
            return Err("invalid table location".into());
        }
        tables.push(Table {
            catalog: parts[0].to_owned(),
            schema: parts[1].to_owned(),
            name: parts[2].to_owned(),
            uri: uri.trim_end_matches('/').to_owned(),
            version: version.to_owned(),
        });
    }
    let (access, bucket) = if tables.is_empty() {
        (None, None)
    } else {
        let field = |key| text(storage, key).map(str::to_owned);
        (
            Some(Access {
                key_id: field("key_id")?,
                secret: field("secret")?,
                session_token: field("session_token")?,
                endpoint: format!("127.0.0.1:{proxy_port}"),
            }),
            Some(field("bucket")?),
        )
    };
    let export = (spec["export"] == true).then(|| Export {
        path: "/workspace/result.arrow".into(),
        max_bytes: 1 << 30,
    });
    Ok(Request {
        sql,
        limit,
        tables,
        access,
        export,
        key: SessionKey {
            bucket,
            memory_mb,
            cpu_millis,
        },
    })
}

/// Runs one request on the worker's session, starting a new session first if this request needs something the current one was not
/// made for. `false` in the answer means the worker must stop after replying: something a statement made could not be removed.
fn run(
    request: &Value,
    proxy_port: u16,
    session: &mut Option<Session>,
) -> (Result<Value, String>, bool) {
    let req = match parse(request, proxy_port) {
        Ok(r) => r,
        Err(e) => return (Err(e), true),
    };
    let began = std::time::Instant::now();
    if session.as_ref().is_none_or(|s| s.key() != &req.key) {
        *session = None;
        match Session::start(req.key.clone()) {
            Ok(s) => *session = Some(s),
            Err(e) => {
                return (
                    Err(format!(
                        "the SQL sandbox could not start: {}",
                        short(&e.to_string())
                    )),
                    true,
                );
            }
        }
    }
    let started_ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
    let Some(active) = session.as_ref() else {
        return (Err("the SQL sandbox has no engine".into()), false);
    };
    let done = active.statement(
        &req.tables,
        req.access.as_ref(),
        &req.sql,
        req.limit,
        req.export.as_ref(),
    );
    let result = match done.result {
        Ok(mut answer) => {
            // Where the time went, in milliseconds: starting an engine (0 when one was reused), getting the tables ready, and the run.
            answer["stats"] = json!({"start_ms": started_ms, "prepare_ms": done.prepare_ms, "run_ms": done.run_ms});
            Ok(answer)
        }
        Err(e) => Err(format!(
            "the SQL statement could not be run: {}",
            short(&e.to_string())
        )),
    };
    (result, done.clean)
}

fn report(result: Result<Value, String>) -> Value {
    match result {
        Ok(result) => {
            json!({"state":"succeeded", "exit_code":0, "stdout":"", "stderr":"", "result": result.to_string(), "error":""})
        }
        Err(error) => {
            json!({"state":"failed", "exit_code":-1, "stdout":"", "stderr":"", "result":"", "error":error})
        }
    }
}

fn main() -> io::Result<()> {
    let port = proxy::start("/inputs/broker/socket".into())?;
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut session: Option<Session> = None;
    for line in stdin.lock().lines() {
        let line = line?;
        let (result, clean) = match serde_json::from_str::<Value>(&line) {
            Ok(request) => run(&request, port, &mut session),
            Err(_) => (Err("invalid work request".to_owned()), true),
        };
        writeln!(stdout, "{}", report(result))?;
        stdout.flush()?;
        if !clean {
            // The next statement could see what this one left: stop, and the runner starts a fresh worker.
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn ask(sql: &str, session: &mut Option<Session>) -> Value {
        let request = json!({"spec":{"sql":sql, "limit":5, "tables":[]}, "limits":{"memory_mb":512, "cpu_millis":1000}});
        let (result, clean) = run(&request, 0, session);
        assert!(clean);
        result.unwrap()
    }

    #[test]
    fn answers_a_statement_without_tables() {
        let mut session = None;
        assert_eq!(
            ask("SELECT 42 AS value", &mut session)["rows"],
            json!([[42]])
        );
    }

    #[test]
    fn one_engine_serves_many_statements_and_each_starts_clean() {
        let mut session = None;
        let first = ask("SELECT 1", &mut session);
        assert!(first["stats"]["start_ms"].as_u64().is_some());
        // The same session is reused: a temporary table made through the engine's own connection does not survive a statement.
        let second = ask("SELECT 2", &mut session);
        assert_eq!(second["rows"], json!([[2]]));
        assert!(session.is_some());
    }

    #[test]
    fn a_statement_cannot_read_host_files_or_change_the_lock() {
        let mut session = None;
        for sql in [
            "SELECT * FROM read_csv('/etc/passwd')",
            "SET lock_configuration = false",
            "SET enable_external_access = true",
        ] {
            let request = json!({"spec":{"sql":sql, "limit":5, "tables":[]}, "limits":{"memory_mb":512, "cpu_millis":1000}});
            let (result, _) = run(&request, 0, &mut session);
            assert!(result.is_err(), "{sql}");
        }
        assert_eq!(ask("SELECT 7", &mut session)["rows"], json!([[7]]));
    }

    #[test]
    fn different_limits_get_a_new_engine() {
        let mut session = None;
        ask("SELECT 1", &mut session);
        let before = session.as_ref().unwrap().key().clone();
        let request = json!({"spec":{"sql":"SELECT 1", "limit":5, "tables":[]}, "limits":{"memory_mb":1024, "cpu_millis":2000}});
        let (result, _) = run(&request, 0, &mut session);
        result.unwrap();
        assert_ne!(session.as_ref().unwrap().key(), &before);
    }
}
