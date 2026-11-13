//! A long-lived sandbox process that creates a fresh `DuckDB` instance for every request.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

mod proxy;

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value[key].as_str().ok_or("the work request is incomplete")
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// The first part of an engine's message: enough to say what went wrong, never a whole page.
fn millis(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn short(text: &str) -> String {
    text.chars().take(300).collect()
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn run(request: &Value, proxy_port: u16) -> Result<Value, String> {
    let spec = &request["spec"];
    let sql = text(spec, "sql")?;
    let limit = spec["limit"].as_u64().unwrap_or(1000).min(1_000_000) as usize;
    let tables = spec["tables"]
        .as_array()
        .ok_or("the work request has no tables")?;
    let storage = &request["storage"];
    let mut inputs = Vec::new();
    for table in tables {
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
        inputs.push((
            parts.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            uri.trim_end_matches('/').to_owned(),
            version.to_owned(),
        ));
    }
    let allowed: Vec<_> = inputs.iter().map(|(_, uri, _)| format!("{uri}/")).collect();
    let started = std::time::Instant::now();
    let mut steps: Vec<(&str, u64)> = Vec::new();
    let engine = loamz_worker::Engine::start_for_run(&allowed, |conn| {
        steps.push(("open", millis(started.elapsed())));
        let memory = request["limits"]["memory_mb"].as_u64().unwrap_or(0);
        let cpu = request["limits"]["cpu_millis"].as_u64().unwrap_or(0);
        if !(64..=8192).contains(&memory) || !(1..=8000).contains(&cpu) {
            return Err(duckdb::Error::InvalidParameterName("invalid resource limits".into()));
        }
        let threads = cpu.div_ceil(1000).max(1);
        conn.execute_batch(&format!("SET threads = {threads}; SET memory_limit = '{}MB'; SET enable_http_metadata_cache = true", memory * 7 / 10))?;
        if !inputs.is_empty() {
            conn.execute_batch("SET extension_directory = '/opt/duckdb-extensions'; LOAD httpfs; LOAD iceberg")?;
            steps.push(("extensions", millis(started.elapsed())));
            let endpoint = format!("127.0.0.1:{proxy_port}");
            let field = |key| text(storage, key).map_err(|e| duckdb::Error::InvalidParameterName(e.into()));
            // CREATE SECRET cannot take parameters, so the values are written as quoted text.
            let sql = format!(
                "CREATE TEMP SECRET loamz_run (TYPE s3, KEY_ID {}, SECRET {}, SESSION_TOKEN {}, ENDPOINT {}, URL_STYLE 'path', REGION 'auto', USE_SSL false)",
                literal(field("key_id")?), literal(field("secret")?), literal(field("session_token")?), literal(&endpoint)
            );
            conn.execute_batch(&sql)?;
            steps.push(("secret", millis(started.elapsed())));
            let mut catalogs = std::collections::HashSet::new();
            let mut schemas = std::collections::HashSet::new();
            for (parts, uri, version) in &inputs {
                if catalogs.insert(parts[0].clone()) { conn.execute_batch(&format!("ATTACH ':memory:' AS {}", quote(&parts[0])))?; }
                if schemas.insert((parts[0].clone(), parts[1].clone())) { conn.execute_batch(&format!("CREATE SCHEMA IF NOT EXISTS {}.{}", quote(&parts[0]), quote(&parts[1])))?; }
                conn.execute_batch(&format!("CREATE VIEW {}.{}.{} AS SELECT * FROM iceberg_scan({}, version = {})", quote(&parts[0]), quote(&parts[1]), quote(&parts[2]), literal(uri), literal(version)))?;
            }
        }
        Ok(())
    }).map_err(|e| format!("the SQL sandbox could not prepare the authorized tables: {}", short(&e.to_string())))?;
    let prepared = started.elapsed();
    let mut answer = engine.answer(sql, limit).map_err(|e| {
        format!(
            "the SQL statement could not be run: {}",
            short(&e.to_string())
        )
    })?;
    // Where the time went, in milliseconds: getting the tables ready, then running the statement.
    // Times are milliseconds since the statement began: opening the engine, loading extensions, the secret, and (prepare_ms) the views.
    answer["stats"] = json!({"prepare_ms": millis(prepared), "run_ms": millis(started.elapsed().saturating_sub(prepared)), "steps": steps});
    Ok(answer)
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
    for line in stdin.lock().lines() {
        let line = line?;
        let result = serde_json::from_str::<Value>(&line)
            .map_err(|_| "invalid work request".to_owned())
            .and_then(|request| run(&request, port));
        writeln!(stdout, "{}", report(result))?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn answers_a_statement_without_tables() {
        let request = json!({"spec":{"sql":"SELECT 42 AS value", "limit":5, "tables":[]}, "limits":{"memory_mb":512, "cpu_millis":1000}});
        let result = run(&request, 0).unwrap();
        assert_eq!(result["rows"], json!([[42]]));
    }
}
