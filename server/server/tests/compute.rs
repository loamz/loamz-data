//! Compute end to end through the router: tables that hold files, SQL that is checked before it runs, agents that lease work, and
//! the token a running job carries. The agent here is the test itself, speaking the agent's protocol.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod common;

use axum::body::Body;
use common::{ACME, Server, Who, ana, bob, cy, fresh_db, server, token};
use loamz_data_server::{compute::service as compute, kernel::now_ms};
use serde_json::{Value, json};

macro_rules! both {
    ($name:ident) => {
        mod $name {
            use super::*;
            #[tokio::test]
            async fn sqlite() {
                super::$name(server(fresh_db(false).await).await).await;
            }
            #[tokio::test]
            #[ignore = "needs LOAMZ_TEST_POSTGRES_URL"]
            async fn postgres() {
                super::$name(server(fresh_db(true).await).await).await;
            }
        }
    };
}

fn items(r: &common::Reply) -> Vec<Value> {
    r.json()["items"].as_array().unwrap().clone()
}

/// ana's workspace with `sales.raw` holding a table `orders` and a volume `out`, and a second table `secrets` nobody else may read.
struct Lab {
    ws: String,
    schema: String,
    orders: String,
    out: String,
}

async fn lab(s: &Server) -> Lab {
    let ws = s.acme().await;
    let c = s.make(&ws, &ana(), "catalog", "", "sales").await;
    let sc = s.make(&ws, &ana(), "schema", &c, "raw").await;
    let orders = s.make(&ws, &ana(), "table", &sc, "orders").await;
    s.make(&ws, &ana(), "table", &sc, "secrets").await;
    let out = s.make(&ws, &ana(), "volume", &sc, "out").await;
    Lab {
        ws,
        schema: sc,
        orders,
        out,
    }
}

/// The id of acme's workspace Main.
async fn main_ws(s: &Server) -> String {
    let r = s
        .get(&format!("/api/v1/orgs/{ACME}/workspaces"), &ana())
        .await;
    r.json()["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "Main")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Attaches compute to Main, and has it heard from, as an agent that has just asked for work would be. Returns its secret and id.
async fn attach(s: &Server) -> (String, String) {
    let ws = main_ws(s).await;
    let r = s
        .post(
            &format!("/api/v1/workspaces/{ws}/compute/targets"),
            &ana(),
            json!({"name": "oracle", "max_memory_mb": 2048, "max_cpu_millis": 1000}),
        )
        .await;
    assert_eq!(r.status, 201, "{}", r.text());
    let (secret, id) = (
        r.json()["secret"].as_str().unwrap().to_owned(),
        r.json()["id"].as_str().unwrap().to_owned(),
    );
    compute::authenticate_agent(&s.app, &secret).await.unwrap();
    (secret, id)
}

async fn agent(
    s: &Server,
    method: &str,
    path: &str,
    secret: &str,
    body: Option<Value>,
) -> common::Reply {
    s.send(
        method,
        path,
        Some(secret),
        body.map_or_else(Body::empty, |b| Body::from(b.to_string())),
    )
    .await
}

// ---- tables are Iceberg tables, reached through the catalog -----------------------------------------------------

async fn iceberg_tables_are_governed(s: Server) {
    async fn pointer(s: &Server, id: String) -> String {
        sqlx::query_scalar::<_, String>("SELECT metadata FROM securables WHERE id = $1")
            .bind(id)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap()
    }
    let l = lab(&s).await;
    let base = format!(
        "/api/v1/workspaces/{}/iceberg/v1/sales/namespaces/raw/tables",
        l.ws
    );
    let load = |who: Who, name: &'static str| {
        let (s, base) = (&s, base.clone());
        async move { s.get(&format!("{base}/{name}"), &who).await }
    };
    let loaded = load(ana(), "orders").await;
    assert_eq!(loaded.status, 200, "{}", loaded.text());
    let meta = loaded.json();
    assert_eq!(meta["metadata"]["format-version"], 2);
    assert_eq!(meta["metadata"]["schemas"][0]["fields"][0]["name"], "id");
    assert!(
        meta["config"].as_object().unwrap().is_empty(),
        "a folder store cannot give credentials to anyone"
    );

    // A table is read with SELECT and changed with MODIFY: a reader loads it and cannot commit.
    s.assign(&l.ws, &[&bob()]).await;
    assert_eq!(load(bob(), "orders").await.status, 404, "not theirs yet");
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    assert_eq!(load(bob(), "orders").await.status, 200);
    let change = json!({"requirements": [], "updates": [{"action": "set-properties", "updates": {"owner": "sales"}}]});
    assert_eq!(
        s.post(&format!("{base}/orders"), &bob(), change.clone())
            .await
            .status,
        403
    );
    assert_eq!(
        load(bob(), "secrets").await.status,
        404,
        "the other table is not theirs"
    );

    // The owner commits; the pointer moves, and a commit that assumed the old table is a conflict.
    let first = s.post(&format!("{base}/orders"), &ana(), change).await;
    assert_eq!(first.status, 200, "{}", first.text());
    assert_eq!(first.json()["metadata"]["properties"]["owner"], "sales");
    assert!(
        pointer(&s, l.orders.clone())
            .await
            .starts_with("metadata/00001-")
    );
    // Every commit leaves the version hint at the current metadata file, so any Iceberg reader can find it without us.
    let current = pointer(&s, l.orders.clone()).await;
    let folder: String = sqlx::query_scalar("SELECT folder FROM securables WHERE id = $1")
        .bind(&l.orders)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    let (mut hint, _) = s
        .app
        .stores
        .open(&folder, "metadata/version-hint.text")
        .await
        .unwrap();
    let mut said = Vec::new();
    while let Some(chunk) = futures_util::StreamExt::next(&mut hint).await {
        said.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(
        String::from_utf8(said).unwrap(),
        current
            .trim_start_matches("metadata/")
            .trim_end_matches(".metadata.json")
    );
    let stale = json!({"requirements": [{"type": "assert-table-uuid", "uuid": "someone-else"}], "updates": []});
    assert_eq!(
        s.post(&format!("{base}/orders"), &ana(), stale)
            .await
            .status,
        409
    );
    assert!(
        pointer(&s, l.orders.clone())
            .await
            .starts_with("metadata/00001-"),
        "a refused commit changes nothing"
    );

    // A table made in two steps, the way DuckDB makes one: staged, then committed with everything that makes it.
    let staged = s
        .post(
            &base,
            &ana(),
            json!({"name": "events", "stage-create": true, "schema": {"type": "struct", "fields": [{"id": 1, "name": "at", "required": false, "type": "timestamp"}]}}),
        )
        .await;
    assert_eq!(staged.status, 200, "{}", staged.text());
    assert_eq!(
        load(ana(), "events").await.status,
        404,
        "staging keeps nothing"
    );
    let meta = staged.json()["metadata"].clone();
    let steps = json!({"requirements": [{"type": "assert-create"}], "updates": [
        {"action": "assign-uuid", "uuid": meta["table-uuid"]},
        {"action": "upgrade-format-version", "format-version": 2},
        {"action": "add-schema", "schema": meta["schemas"][0], "last-column-id": 1},
        {"action": "set-current-schema", "schema-id": -1},
        {"action": "add-spec", "spec": {"fields": []}}, {"action": "set-default-spec", "spec-id": -1},
        {"action": "add-sort-order", "sort-order": {"fields": []}}, {"action": "set-default-sort-order", "sort-order-id": -1},
        {"action": "set-location", "location": meta["location"]},
    ]});
    assert_eq!(
        s.post(&format!("{base}/events"), &ana(), steps.clone())
            .await
            .status,
        200
    );
    assert_eq!(load(ana(), "events").await.status, 200);
    assert_eq!(
        s.post(&format!("{base}/events"), &ana(), steps.clone())
            .await
            .status,
        409,
        "it exists now"
    );
    let mut elsewhere = steps;
    elsewhere["updates"][8]["location"] = json!("s3://not-ours/x");
    assert_eq!(
        s.post(&format!("{base}/others"), &ana(), elsewhere)
            .await
            .status,
        400,
        "a location we did not give"
    );

    // The tables of a schema, as a catalog client lists them; a table with no grant is not listed.
    let ids: Vec<String> = s.get(&base, &bob()).await.json()["identifiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["orders"]);

    // Dropping the catalog takes the table's bytes with it.
    assert!(s.store_dir(&l.orders).exists());
    let catalog = items(
        &s.get(&format!("/api/v1/workspaces/{}/securables", l.ws), &ana())
            .await,
    )[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    s.delete(
        &format!("/api/v1/workspaces/{}/securables/{catalog}", l.ws),
        &ana(),
    )
    .await;
    s.work().await;
    assert!(!s.store_dir(&l.orders).exists());
}
both!(iceberg_tables_are_governed);

// ---- asking for SQL ----------------------------------------------------------------------------------------------

async fn sql_is_checked_before_it_runs(s: Server) {
    let l = lab(&s).await;
    let url = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let ask = |who: Who, sql: &str| {
        let (s, url, sql) = (&s, url.clone(), sql.to_owned());
        async move { s.post(&url, &who, json!({"sql": sql, "wait_s": 0})).await }
    };

    // No compute yet: a plain answer, not a statement that waits for ever.
    let r = ask(ana(), "SELECT * FROM sales.raw.orders").await;
    assert_eq!(r.status, 409);
    assert!(
        r.json()["detail"]
            .as_str()
            .unwrap()
            .contains("no compute is running")
    );
    attach(&s).await;

    // What is not a plain query, or not a catalog table, never reaches compute.
    for sql in [
        "DROP TABLE sales.raw.orders",
        "SELECT * FROM read_csv('/etc/passwd')",
        "SELECT * FROM orders",
        "SELECT 1; SELECT 2",
    ] {
        assert_eq!(ask(ana(), sql).await.status, 400, "{sql}");
    }
    assert_eq!(
        ask(ana(), "SELECT * FROM sales.raw.nothing").await.status,
        404
    );

    // A person who may not read a table gets exactly the answer a missing table gets, and the refusal is on record.
    s.assign(&l.ws, &[&bob(), &cy()]).await;
    s.put(
        &format!("/api/v1/workspaces/{}/entitlements", l.ws),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "sql-access"}),
    )
    .await;
    let missing = ask(bob(), "SELECT * FROM sales.raw.nothing").await;
    let forbidden = ask(bob(), "SELECT * FROM sales.raw.orders").await;
    assert_eq!((missing.status, forbidden.status), (404, 404));
    assert_eq!(
        forbidden.json()["detail"]
            .as_str()
            .unwrap()
            .replace("orders", "x"),
        missing.json()["detail"]
            .as_str()
            .unwrap()
            .replace("nothing", "x")
    );

    // Reading one table does not give the next; being allowed to read gets the statement queued.
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    assert_eq!(
        ask(bob(), "SELECT * FROM sales.raw.secrets").await.status,
        404
    );
    let ok = ask(bob(), "SELECT * FROM sales.raw.orders o").await;
    assert_eq!(ok.status, 202, "{}", ok.text());
    assert_eq!(ok.json()["state"], "queued");

    // Someone assigned to the workspace without the entitlement may not run SQL at all.
    assert_eq!(ask(cy(), "SELECT 1").await.status, 403);

    let trail = items(
        &s.get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
            .await,
    );
    assert!(trail.iter().any(|e| {
        e["actor"]
            .as_str()
            .is_some_and(|s| s.starts_with("erased:"))
            && e["outcome"] == "denied"
    }));
    assert!(trail.iter().any(|e| e["action"] == "sql.submit"));
}
both!(sql_is_checked_before_it_runs);

// ---- an agent does the work --------------------------------------------------------------------------------------

async fn an_agent_runs_a_statement_and_reports_its_rows(s: Server) {
    let l = lab(&s).await;
    let (secret, _) = attach(&s).await;
    let r = s
        .post(
            &format!("/api/v1/workspaces/{}/sql/statements", l.ws),
            &ana(),
            json!({"sql": "SELECT sum(total) FROM sales.raw.orders", "wait_s": 0}),
        )
        .await;
    assert_eq!(r.status, 202);
    let id = r.json()["id"].as_str().unwrap().to_owned();

    // Only the compute it was queued for can take it, and an unknown secret takes nothing.
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease", "agt_nope", None)
            .await
            .status,
        401
    );
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease", &token(&ana()), None)
            .await
            .status,
        401,
        "a person's token is not an agent's"
    );
    let lease = agent(&s, "POST", "/api/v1/agent/lease", &secret, None).await;
    assert_eq!(lease.status, 200, "{}", lease.text());
    let work = lease.json();
    assert_eq!(
        (work["run"]["id"].as_str(), work["run"]["kind"].as_str()),
        (Some(id.as_str()), Some("sql"))
    );
    assert_eq!(work["run"]["spec"]["tables"][0]["name"], "sales.raw.orders");
    assert!(
        work["run"]["spec"].get("as").is_none(),
        "who it runs as is not handed to the agent"
    );
    assert_eq!(
        work["token"], "",
        "a statement has no token: its data comes as credentials for its tables' folders"
    );
    assert!(
        work["storage"].is_null(),
        "this server's storage is a folder, which no other machine can reach"
    );
    let ws = &l.ws;

    // The agent keeps the run alive, then reports.
    let hb = agent(
        &s,
        "POST",
        &format!("/api/v1/agent/runs/{id}/heartbeat"),
        &secret,
        None,
    )
    .await;
    assert_eq!(
        (hb.status, hb.json()["cancel"].clone()),
        (200, json!(false))
    );
    let rows = json!({"columns": [{"name": "sum", "type": "HUGEINT"}], "rows": [[10]], "truncated": false}).to_string();
    let done = agent(
        &s,
        "POST",
        &format!("/api/v1/agent/runs/{id}/complete"),
        &secret,
        Some(json!({"state": "succeeded", "exit_code": 0, "result": rows})),
    )
    .await;
    assert_eq!(done.status, 204, "{}", done.text());

    // The person sees the rows; the token is dead.
    let run = s
        .get(&format!("/api/v1/workspaces/{ws}/runs/{id}"), &ana())
        .await
        .json();
    assert_eq!(
        (run["state"].as_str(), run["result"]["rows"][0][0].clone()),
        (Some("succeeded"), json!(10))
    );
    // A run is finished once.
    assert_eq!(
        agent(
            &s,
            "POST",
            &format!("/api/v1/agent/runs/{id}/complete"),
            &secret,
            Some(json!({"state": "failed"}))
        )
        .await
        .status,
        409
    );
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease?wait_s=0", &secret, None)
            .await
            .status,
        204,
        "and nothing else is waiting"
    );
}
both!(an_agent_runs_a_statement_and_reports_its_rows);

async fn a_job_writes_only_to_its_declared_output(s: Server) {
    let l = lab(&s).await;
    let other = s
        .make(&l.ws, &ana(), "volume", &l.schema, "elsewhere")
        .await;
    let (secret, _) = attach(&s).await;
    let url = format!("/api/v1/workspaces/{}/runs", l.ws);
    let job = json!({"script": "print(1)", "inputs": [l.orders], "output": l.out, "memory_mb": 512, "timeout_s": 30});
    let r = s.post(&url, &ana(), job.clone()).await;
    assert_eq!(r.status, 202, "{}", r.text());
    let lease = agent(&s, "POST", "/api/v1/agent/lease", &secret, None)
        .await
        .json();
    assert_eq!(
        lease["limits"],
        json!({"memory_mb": 512, "cpu_millis": 1000, "timeout_s": 30})
    );
    assert_eq!(lease["run"]["spec"]["inputs"][0]["name"], "orders");
    let jt = lease["token"].as_str().unwrap().to_owned();
    let run_id = lease["run"]["id"].as_str().unwrap();
    let put = |id: &str, bytes: &'static str| {
        let (s, jt, path) = (
            &s,
            jt.clone(),
            format!(
                "/api/v1/workspaces/{}/volumes/{id}/files/runs/{run_id}/out.txt",
                l.ws,
            ),
        );
        async move { s.send("PUT", &path, Some(&jt), Body::from(bytes)).await }
    };
    assert_eq!(
        put(&l.out, "result").await.status,
        201,
        "the declared output"
    );
    assert_eq!(
        put(&other, "result").await.status,
        404,
        "any other volume is not there for it"
    );
    assert_eq!(
        s.download(&l.ws, &l.out, &format!("runs/{run_id}/out.txt"), &ana())
            .await
            .bytes,
        b"result"
    );

    // Asking for what you may not have is refused at the door, before anything is queued.
    s.assign(&l.ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{}/entitlements", l.ws),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "compute-create"}),
    )
    .await;
    assert_eq!(
        s.post(&url, &bob(), job.clone()).await.status,
        404,
        "he cannot read the input"
    );
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    assert_eq!(
        s.post(&url, &bob(), job.clone()).await.status,
        404,
        "or write the output"
    );
    s.grant(&l.ws, &ana(), &l.out, "user:bob", "contributor")
        .await;
    assert_eq!(s.post(&url, &bob(), job.clone()).await.status, 202);
    for bad in [
        json!({"script": "", "output": l.out}),
        json!({"script": "x", "output": l.orders}),
        json!({"script": "x", "output": l.out, "memory_mb": 99999}),
        json!({"script": "x", "output": l.out, "timeout_s": 100_000}),
    ] {
        assert!(
            [400, 404].contains(&s.post(&url, &ana(), bad.clone()).await.status),
            "{bad}"
        );
    }
}
both!(a_job_writes_only_to_its_declared_output);

async fn a_token_is_for_one_workspace(s: Server) {
    let l = lab(&s).await;
    let elsewhere = s.workspace(&ana(), ACME, "Elsewhere").await;
    let (secret, _) = attach(&s).await;
    let queued = s
        .post(
            &format!("/api/v1/workspaces/{}/runs", l.ws),
            &ana(),
            json!({"script": "1", "inputs": [l.orders], "output": l.out}),
        )
        .await;
    assert_eq!(queued.status, 202, "{}", queued.text());
    let leased = agent(&s, "POST", "/api/v1/agent/lease", &secret, None)
        .await
        .json();
    let jt = leased["token"].as_str().unwrap().to_owned();
    let run_id = leased["run"]["id"].as_str().unwrap().to_owned();
    let put = |ws: String| {
        let (s, jt, out, run_id) = (&s, jt.clone(), l.out.clone(), run_id.clone());
        async move {
            s.send(
                "PUT",
                &format!("/api/v1/workspaces/{ws}/volumes/{out}/files/runs/{run_id}/x"),
                Some(&jt),
                Body::from("x"),
            )
            .await
            .status
        }
    };
    // The same catalog is visible from the other workspace, but this token is not for it.
    assert_eq!(put(elsewhere).await, 404);
    assert_eq!(put(l.ws.clone()).await, 201);
}
both!(a_token_is_for_one_workspace);

async fn a_runs_token_dies_with_the_run(s: Server) {
    let l = lab(&s).await;
    let (secret, _) = attach(&s).await;
    let r = s
        .post(
            &format!("/api/v1/workspaces/{}/runs", l.ws),
            &ana(),
            json!({"script": "1", "inputs": [], "output": l.out}),
        )
        .await;
    let id = r.json()["id"].as_str().unwrap().to_owned();
    let leased = agent(&s, "POST", "/api/v1/agent/lease", &secret, None)
        .await
        .json();
    let jt = leased["token"].as_str().unwrap().to_owned();
    let put = || {
        let (s, jt, out, ws) = (&s, jt.clone(), l.out.clone(), l.ws.clone());
        let id = id.clone();
        async move {
            s.send(
                "PUT",
                &format!("/api/v1/workspaces/{ws}/volumes/{out}/files/runs/{id}/x"),
                Some(&jt),
                Body::from("x"),
            )
            .await
            .status
        }
    };
    assert_eq!(put().await, 201);
    agent(
        &s,
        "POST",
        &format!("/api/v1/agent/runs/{id}/complete"),
        &secret,
        Some(json!({"state": "succeeded"})),
    )
    .await;
    assert_eq!(put().await, 401, "a finished run's token no longer works");
}
both!(a_runs_token_dies_with_the_run);

// ---- who sees what, stopping, and time ----------------------------------------------------------------------------

async fn runs_belong_to_the_people_who_asked(s: Server) {
    let l = lab(&s).await;
    attach(&s).await;
    s.assign(&l.ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{}/entitlements", l.ws),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "sql-access"}),
    )
    .await;
    let url = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let mine = s
        .post(&url, &ana(), json!({"sql": "SELECT 1", "wait_s": 0}))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let his = s
        .post(&url, &bob(), json!({"sql": "SELECT 2", "wait_s": 0}))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let runs = |who: Who| {
        let (s, ws) = (&s, l.ws.clone());
        async move {
            items(&s.get(&format!("/api/v1/workspaces/{ws}/runs"), &who).await)
                .iter()
                .map(|r| r["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        runs(bob()).await,
        vec![his.clone()],
        "a user sees their own"
    );
    assert_eq!(runs(ana()).await.len(), 2, "an admin sees all of them");
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{}/runs/{mine}", l.ws), &bob())
            .await
            .status,
        404
    );
    assert_eq!(
        s.post(
            &format!("/api/v1/workspaces/{}/runs/{mine}/cancel", l.ws),
            &bob(),
            json!({})
        )
        .await
        .status,
        404
    );
    // A list leaves out what can be large; the run itself has it.
    let listed = items(
        &s.get(&format!("/api/v1/workspaces/{}/runs", l.ws), &ana())
            .await,
    );
    assert!(listed[0].get("result").is_none() && listed[0].get("spec").is_none());
    assert!(
        s.get(&format!("/api/v1/workspaces/{}/runs/{mine}", l.ws), &ana())
            .await
            .json()
            .get("spec")
            .is_some()
    );
}
both!(runs_belong_to_the_people_who_asked);

async fn runs_can_be_narrowed_and_paged(s: Server) {
    let l = lab(&s).await;
    attach(&s).await;
    s.assign(&l.ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{}/entitlements", l.ws),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "sql-access"}),
    )
    .await;
    let url = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    for (who, n) in [(ana(), 1), (ana(), 2), (bob(), 3)] {
        s.post(
            &url,
            &who,
            json!({"sql": format!("SELECT {n}"), "wait_s": 0}),
        )
        .await;
        // Runs are told apart by the millisecond they were asked.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let list = |q: &'static str, who: Who| {
        let (s, ws) = (&s, l.ws.clone());
        async move {
            items(
                &s.get(&format!("/api/v1/workspaces/{ws}/runs?{q}"), &who)
                    .await,
            )
        }
    };
    assert_eq!(list("", ana()).await.len(), 3);
    assert_eq!(
        list("by=bob", ana()).await.len(),
        1,
        "an admin narrows to one person"
    );
    assert_eq!(
        list("by=ana", bob()).await.len(),
        1,
        "anyone else is still only shown their own"
    );
    assert_eq!(list("state=queued", ana()).await.len(), 3);
    assert_eq!(list("state=succeeded", ana()).await.len(), 0);
    assert_eq!(list("kind=job", ana()).await.len(), 0);
    let page = list("limit=2", ana()).await;
    assert_eq!(page.len(), 2);
    let cursor = page[1]["created_ms"].as_i64().unwrap();
    let rest = items(
        &s.get(
            &format!(
                "/api/v1/workspaces/{}/runs?limit=2&before_ms={cursor}",
                l.ws
            ),
            &ana(),
        )
        .await,
    );
    assert_eq!(rest.len(), 1, "the cursor continues where the page ended");
    assert_eq!(
        rest[0]["queued_ms"], -1,
        "a run that has not started has no wait yet"
    );
    assert_eq!(
        s.get(
            &format!("/api/v1/workspaces/{}/runs?state=sleeping", l.ws),
            &ana()
        )
        .await
        .status,
        400
    );
}
both!(runs_can_be_narrowed_and_paged);

async fn a_run_can_be_stopped(s: Server) {
    let l = lab(&s).await;
    let (secret, _) = attach(&s).await;
    let url = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let queued = s
        .post(&url, &ana(), json!({"sql": "SELECT 1", "wait_s": 0}))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let cancel = |id: &str| format!("/api/v1/workspaces/{}/runs/{id}/cancel", l.ws);
    assert_eq!(
        s.post(&cancel(&queued), &ana(), json!({})).await.json()["state"],
        "cancelled",
        "before it started, at once"
    );
    assert_eq!(
        s.post(&cancel(&queued), &ana(), json!({})).await.status,
        409,
        "once"
    );

    let running = s
        .post(&url, &ana(), json!({"sql": "SELECT 2", "wait_s": 0}))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease", &secret, None)
            .await
            .status,
        200
    );
    let r = s.post(&cancel(&running), &ana(), json!({})).await;
    assert_eq!(
        (r.status, r.json()["cancelling"].clone()),
        (202, json!(true))
    );
    let hb = agent(
        &s,
        "POST",
        &format!("/api/v1/agent/runs/{running}/heartbeat"),
        &secret,
        None,
    )
    .await;
    assert_eq!(
        hb.json()["cancel"],
        true,
        "the agent hears of it when it next checks in"
    );
    agent(
        &s,
        "POST",
        &format!("/api/v1/agent/runs/{running}/complete"),
        &secret,
        Some(json!({"state": "cancelled"})),
    )
    .await;
    assert_eq!(
        s.get(
            &format!("/api/v1/workspaces/{}/runs/{running}", l.ws),
            &ana()
        )
        .await
        .json()["state"],
        "cancelled"
    );
}
both!(a_run_can_be_stopped);

async fn work_that_cannot_finish_is_failed_and_says_why(s: Server) {
    let l = lab(&s).await;
    let (secret, id) = attach(&s).await;
    let url = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let ask = || async {
        s.post(&url, &ana(), json!({"sql": "SELECT 1", "wait_s": 0}))
            .await
            .json()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let got = |id: String| {
        let (s, ws) = (&s, l.ws.clone());
        async move {
            s.get(&format!("/api/v1/workspaces/{ws}/runs/{id}"), &ana())
                .await
                .json()
        }
    };

    // The agent takes it and goes silent: past its lease, the run fails, and its token is gone.
    let silent = ask().await;
    agent(&s, "POST", "/api/v1/agent/lease", &secret, None).await;
    sqlx::query("UPDATE runs SET lease_until = 1 WHERE id = $1")
        .bind(&silent)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(compute::reap(&s.app, now_ms()).await.unwrap(), 1);
    let r = got(silent).await;
    assert_eq!(
        (
            r["state"].clone(),
            r["error"].as_str().unwrap().contains("stopped answering")
        ),
        (json!("failed"), true)
    );

    // Nobody takes it for ten minutes: it fails instead of waiting for ever.
    let forgotten = ask().await;
    sqlx::query("UPDATE runs SET created_ms = 1 WHERE id = $1")
        .bind(&forgotten)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    compute::reap(&s.app, now_ms()).await.unwrap();
    assert!(
        got(forgotten).await["error"]
            .as_str()
            .unwrap()
            .contains("no compute picked this up")
    );

    // Taking the compute away fails what was on it; a new secret turns the old agent away.
    let doomed = ask().await;
    let rotated = s
        .post(
            &format!(
                "/api/v1/workspaces/{}/compute/targets/{id}/secret",
                main_ws(&s).await
            ),
            &ana(),
            json!({}),
        )
        .await
        .json()["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease", &secret, None)
            .await
            .status,
        401
    );
    compute::authenticate_agent(&s.app, &rotated).await.unwrap();
    assert_eq!(
        s.delete(
            &format!(
                "/api/v1/workspaces/{}/compute/targets/{id}",
                main_ws(&s).await
            ),
            &bob()
        )
        .await
        .status,
        404,
        "someone outside the workspace cannot tell it has compute"
    );
    assert_eq!(
        s.delete(
            &format!(
                "/api/v1/workspaces/{}/compute/targets/{id}",
                main_ws(&s).await
            ),
            &ana()
        )
        .await
        .status,
        204
    );
    assert!(
        got(doomed).await["error"]
            .as_str()
            .unwrap()
            .contains("removed")
    );
}
both!(work_that_cannot_finish_is_failed_and_says_why);

async fn compute_is_attached_by_a_workspaces_admins(s: Server) {
    let l = lab(&s).await;
    let url = format!("/api/v1/workspaces/{}/compute/targets", l.ws);
    // A workspace member who is not its admin cannot attach compute; someone outside the workspace cannot even tell it exists.
    assert_eq!(
        s.post(&url, &bob(), json!({"name": "mine"})).await.status,
        404
    );
    s.assign(&l.ws, &[&bob()]).await;
    assert_eq!(
        s.post(&url, &bob(), json!({"name": "mine"})).await.status,
        403
    );
    let r = s.post(&url, &ana(), json!({"name": "oracle"})).await;
    assert_eq!(
        (r.status, r.json()["max_memory_mb"].clone()),
        (201, json!(2048))
    );
    assert!(r.json()["secret"].as_str().unwrap().starts_with("agt_"));
    assert_eq!(
        s.post(&url, &ana(), json!({"name": "oracle"})).await.status,
        409
    );
    assert_eq!(
        s.post(&url, &ana(), json!({"name": "<x>"})).await.status,
        400
    );
    assert_eq!(
        s.post(&url, &ana(), json!({"name": "tiny", "max_memory_mb": 1}))
            .await
            .status,
        400
    );
    // The secret is shown once: no listing carries it, and only a hash is kept.
    assert!(!s.get(&url, &ana()).await.text().contains("agt_"));
    let stored: String =
        sqlx::query_scalar("SELECT secret_hash FROM compute_targets WHERE name = 'oracle'")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert!(!stored.starts_with("agt_") && stored.len() == 64);
    // A workspace member can see what compute there is, and whether it answers; not attach it.
    let seen = items(
        &s.get(
            &format!("/api/v1/workspaces/{}/compute/targets", l.ws),
            &bob(),
        )
        .await,
    );
    assert_eq!((seen.len(), seen[0]["online"].clone()), (1, json!(false)));
}
both!(compute_is_attached_by_a_workspaces_admins);

/// Compute belongs to one workspace: another workspace of the same organization cannot see it, name it, or run on it.
async fn compute_belongs_to_its_workspace(s: Server) {
    let _ = lab(&s).await;
    let (_, id) = attach(&s).await;
    let other = s.workspace(&ana(), ACME, "Other").await;
    let mine = format!("/api/v1/workspaces/{other}/compute/targets");
    assert!(
        items(&s.get(&mine, &ana()).await).is_empty(),
        "it is not listed there"
    );
    let asked = s
        .post(
            &format!("/api/v1/workspaces/{other}/sql/statements"),
            &ana(),
            json!({"sql": "SELECT 1", "target": id}),
        )
        .await;
    assert_eq!(asked.status, 404, "{}", asked.text());
    assert_eq!(
        s.post(
            &format!("/api/v1/workspaces/{other}/compute/targets/{id}/secret"),
            &ana(),
            json!({})
        )
        .await
        .status,
        404,
        "nor can it be managed from there"
    );
    // A workspace that still has compute cannot be deleted.
    let main = main_ws(&s).await;
    assert_eq!(
        s.delete(&format!("/api/v1/workspaces/{main}"), &ana())
            .await
            .status,
        409
    );
}
both!(compute_belongs_to_its_workspace);

/// A workspace can be given its own cap on active runs, below its organization's; only an account admin sets it.
async fn a_workspace_can_have_its_own_run_cap(s: Server) {
    let l = lab(&s).await;
    attach(&s).await;
    let quota = format!("/api/v1/workspaces/{}/quota", l.ws);
    let statement = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let ask = || async {
        s.post(
            &statement,
            &ana(),
            json!({"sql": "SELECT * FROM sales.raw.orders", "wait_s": 0}),
        )
        .await
    };
    assert_eq!(
        s.get(&quota, &ana()).await.json()["max_active_runs"],
        json!(null)
    );
    s.assign(&l.ws, &[&bob()]).await;
    assert_eq!(
        s.put(&quota, &bob(), json!({"max_active_runs": 1}))
            .await
            .status,
        403,
        "only account admins"
    );
    assert_eq!(
        s.put(&quota, &ana(), json!({"max_active_runs": 0}))
            .await
            .status,
        400
    );
    assert_eq!(
        s.put(&quota, &ana(), json!({"max_active_runs": 1}))
            .await
            .status,
        204
    );
    assert_eq!(ask().await.status, 202, "one is allowed");
    let second = ask().await;
    assert_eq!(second.status, 429, "{}", second.text());
    assert!(second.text().contains("this workspace"));
    // Removing the cap lifts it.
    assert_eq!(
        s.put(&quota, &ana(), json!({"max_active_runs": null}))
            .await
            .status,
        204
    );
    assert_eq!(ask().await.status, 202);
}
both!(a_workspace_can_have_its_own_run_cap);

/// The same table loaded twice in a row asks the store for credentials once.
#[tokio::test]
async fn storage_credentials_are_reused_while_they_last() {
    let issuer = std::sync::Arc::new(common::CountingIssuer::default());
    let s = common::server_with_issuer(fresh_db(false).await, issuer.clone()).await;
    let l = lab(&s).await;
    let table = format!(
        "/api/v1/workspaces/{}/iceberg/v1/sales/namespaces/raw/tables/orders",
        l.ws
    );
    let asked = || issuer.0.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(s.get(&table, &ana()).await.status, 200);
    let first = asked();
    assert!(first >= 1, "a table load asks for credentials");
    assert_eq!(s.get(&table, &ana()).await.status, 200);
    assert_eq!(asked(), first, "the second load reuses them");
}

/// A catalog can say which of its workspace's compute may read it. Without a list, any may.
async fn a_catalog_can_name_the_compute_that_may_read_it(s: Server) {
    let l = lab(&s).await;
    let (_, first) = attach(&s).await;
    let made = s
        .post(
            &format!("/api/v1/workspaces/{}/compute/targets", l.ws),
            &ana(),
            json!({"name": "second", "max_memory_mb": 2048, "max_cpu_millis": 1000}),
        )
        .await;
    let (second, second_secret) = (
        made.json()["id"].as_str().unwrap().to_owned(),
        made.json()["secret"].as_str().unwrap().to_owned(),
    );
    compute::authenticate_agent(&s.app, &second_secret)
        .await
        .unwrap();
    let catalog: String = sqlx::query_scalar("SELECT parent_id FROM securables WHERE id = $1")
        .bind(&l.schema)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    let policy = format!("/api/v1/workspaces/{}/catalogs/{catalog}/compute", l.ws);
    let statement = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let ask = |target: Option<&str>| {
        let (s, statement) = (&s, statement.clone());
        let body = json!({"sql": "SELECT * FROM sales.raw.orders", "wait_s": 0, "target": target});
        async move { s.post(&statement, &ana(), body).await }
    };

    // No list: any of the workspace's compute may read it, so a named one is accepted.
    assert_eq!(s.get(&policy, &ana()).await.json()["compute"], json!([]));
    assert_eq!(ask(Some(&first)).await.status, 202);

    // Only people who manage the catalog may say; the compute must be the workspace's own.
    s.assign(&l.ws, &[&bob()]).await;
    assert_eq!(
        s.put(&policy, &bob(), json!({"compute": [second]}))
            .await
            .status,
        404,
        "bob cannot see the catalog"
    );
    assert_eq!(
        s.put(&policy, &ana(), json!({"compute": ["not-ours"]}))
            .await
            .status,
        400
    );
    assert_eq!(
        s.put(&policy, &ana(), json!({"compute": [second]}))
            .await
            .status,
        204
    );
    assert_eq!(
        s.get(&policy, &ana()).await.json()["compute"],
        json!([second])
    );

    // Now only the second may read it: asking for the first is refused, and with no choice made it goes to the second.
    let refused = ask(Some(&first)).await;
    assert_eq!(refused.status, 404, "{}", refused.text());
    let placed = ask(None).await;
    assert_eq!(placed.status, 202, "{}", placed.text());
    assert_eq!(placed.json()["target_id"], json!(second));

    // Taking the second away takes it off the list: the catalog is readable from any compute again.
    assert_eq!(
        s.delete(
            &format!("/api/v1/workspaces/{}/compute/targets/{second}", l.ws),
            &ana()
        )
        .await
        .status,
        204
    );
    assert_eq!(s.get(&policy, &ana()).await.json()["compute"], json!([]));
    assert_eq!(ask(None).await.status, 202);
}
both!(a_catalog_can_name_the_compute_that_may_read_it);

/// An agent acting for an admin is not an admin: running SQL needs the entitlement on both sides, the person's and the agent's.
async fn an_agent_needs_the_sql_entitlement_too(s: Server) {
    let l = lab(&s).await;
    attach(&s).await;
    s.put(
        &format!("/api/v1/workspaces/{}/members", l.ws),
        &ana(),
        json!({"principal": "sp:agent-1", "role": "user"}),
    )
    .await;
    s.grant(&l.ws, &ana(), &l.orders, "sp:agent-1", "reader")
        .await;
    let url = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let pair = ana().acting_as("agent-1");
    let ask = || async {
        s.post(
            &url,
            &pair,
            json!({"sql": "SELECT * FROM sales.raw.orders", "wait_s": 0}),
        )
        .await
    };

    assert_eq!(
        ask().await.status,
        403,
        "ana may, the agent may not: the pair may not"
    );
    s.put(
        &format!("/api/v1/workspaces/{}/entitlements", l.ws),
        &ana(),
        json!({"principal": "sp:agent-1", "entitlement": "sql-access"}),
    )
    .await;
    assert_eq!(ask().await.status, 202, "both may");
    // The agent's own entitlement is not the person's: bob, who has none, is refused with the same agent.
    s.assign(&l.ws, &[&bob()]).await;
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    let r = s
        .post(
            &url,
            &bob().acting_as("agent-1"),
            json!({"sql": "SELECT * FROM sales.raw.orders", "wait_s": 0}),
        )
        .await;
    assert_eq!(r.status, 403);
}
both!(an_agent_needs_the_sql_entitlement_too);

async fn a_queued_run_cannot_use_revoked_access(s: Server) {
    let l = lab(&s).await;
    let (secret, _) = attach(&s).await;
    s.assign(&l.ws, &[&bob()]).await;
    for entitlement in ["sql-access", "compute-create"] {
        assert_eq!(
            s.put(
                &format!("/api/v1/workspaces/{}/entitlements", l.ws),
                &ana(),
                json!({"principal": "user:bob", "entitlement": entitlement}),
            )
            .await
            .status,
            204
        );
    }
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    s.grant(&l.ws, &ana(), &l.out, "user:bob", "contributor")
        .await;
    let sql = s
        .post(
            &format!("/api/v1/workspaces/{}/sql/statements", l.ws),
            &bob(),
            json!({"sql": "SELECT * FROM sales.raw.orders", "wait_s": 0}),
        )
        .await;
    assert_eq!(sql.status, 202, "{}", sql.text());
    let sql_id = sql.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        s.delete(
            &format!(
                "/api/v1/workspaces/{}/securables/{}/grants/user:bob",
                l.ws, l.orders
            ),
            &ana(),
        )
        .await
        .status,
        204
    );
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease?wait_s=0", &secret, None)
            .await
            .status,
        204
    );
    let finished = s
        .get(
            &format!("/api/v1/workspaces/{}/runs/{sql_id}", l.ws),
            &ana(),
        )
        .await
        .json();
    assert_eq!(finished["state"], "failed");
    assert!(finished["error"].as_str().unwrap().contains("authorized"));

    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    let job = s
        .post(
            &format!("/api/v1/workspaces/{}/runs", l.ws),
            &bob(),
            json!({"script": "print(1)", "inputs": [l.orders], "output": l.out}),
        )
        .await;
    assert_eq!(job.status, 202, "{}", job.text());
    let job_id = job.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        s.delete(
            &format!(
                "/api/v1/workspaces/{}/securables/{}/grants/user:bob",
                l.ws, l.out
            ),
            &ana(),
        )
        .await
        .status,
        204
    );
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease?wait_s=0", &secret, None)
            .await
            .status,
        204
    );
    let finished = s
        .get(
            &format!("/api/v1/workspaces/{}/runs/{job_id}", l.ws),
            &ana(),
        )
        .await
        .json();
    assert_eq!(finished["state"], "failed");

    let sql = s
        .post(
            &format!("/api/v1/workspaces/{}/sql/statements", l.ws),
            &bob(),
            json!({"sql": "SELECT * FROM sales.raw.orders", "wait_s": 0}),
        )
        .await;
    assert_eq!(sql.status, 202);
    let sql_id = sql.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        s.delete(
            &format!(
                "/api/v1/workspaces/{}/entitlements/user:bob/sql-access",
                l.ws
            ),
            &ana(),
        )
        .await
        .status,
        204
    );
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease?wait_s=0", &secret, None)
            .await
            .status,
        204
    );
    let finished = s
        .get(
            &format!("/api/v1/workspaces/{}/runs/{sql_id}", l.ws),
            &ana(),
        )
        .await
        .json();
    assert_eq!(finished["state"], "failed");
}
both!(a_queued_run_cannot_use_revoked_access);

async fn completed_results_require_current_input_access(s: Server) {
    let l = lab(&s).await;
    let (secret, _) = attach(&s).await;
    s.assign(&l.ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{}/entitlements", l.ws),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "sql-access"}),
    )
    .await;
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    let submitted = s
        .post(
            &format!("/api/v1/workspaces/{}/sql/statements", l.ws),
            &bob(),
            json!({"sql": "SELECT * FROM sales.raw.orders", "wait_s": 0}),
        )
        .await;
    assert_eq!(submitted.status, 202);
    let id = submitted.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease?wait_s=0", &secret, None)
            .await
            .status,
        200
    );
    let rows =
        json!({"columns": [{"name": "id", "type": "BIGINT"}], "rows": [[7]], "truncated": false})
            .to_string();
    assert_eq!(
        agent(
            &s,
            "POST",
            &format!("/api/v1/agent/runs/{id}/complete"),
            &secret,
            Some(json!({"state": "succeeded", "exit_code": 0, "result": rows, "stdout": "private stdout", "stderr": "private stderr", "error": "private error"}))
        )
        .await
        .status,
        204
    );
    let path = format!("/api/v1/workspaces/{}/runs/{id}", l.ws);
    assert_eq!(s.get(&path, &bob()).await.json()["result"]["rows"][0][0], 7);
    assert_eq!(
        s.delete(
            &format!(
                "/api/v1/workspaces/{}/securables/{}/grants/user:bob",
                l.ws, l.orders
            ),
            &ana()
        )
        .await
        .status,
        204
    );
    let restricted = s.get(&path, &bob()).await;
    assert_eq!(restricted.status, 200);
    let view = restricted.json();
    assert_eq!(view["state"], "succeeded");
    assert_eq!(view["result_access"], "restricted");
    assert_eq!(view["spec"]["sql"], "SELECT * FROM sales.raw.orders");
    assert_eq!(view["error"], "");
    assert!(
        view.get("stdout").is_none()
            && view.get("stderr").is_none()
            && view.get("result").is_none()
    );
    s.assign(&l.ws, &[&bob(), &cy()]).await;
    assert_eq!(
        s.get(&path, &cy()).await.status,
        404,
        "another user cannot see the history"
    );
    assert_eq!(s.get(&path, &ana()).await.json()["result"]["rows"][0][0], 7);
    assert!(
        items(
            &s.get(&format!("/api/v1/workspaces/{}/runs", l.ws), &bob())
                .await
        )
        .iter()
        .any(|r| r["id"] == id)
    );
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    s.grant(&l.ws, &ana(), &l.out, "user:bob", "contributor")
        .await;
    s.put(
        &format!("/api/v1/workspaces/{}/entitlements", l.ws),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "compute-create"}),
    )
    .await;
    let job = s
        .post(
            &format!("/api/v1/workspaces/{}/runs", l.ws),
            &bob(),
            json!({"script": "print('data')", "inputs": [l.orders], "output": l.out}),
        )
        .await;
    assert_eq!(job.status, 202, "{}", job.text());
    let job_id = job.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease?wait_s=0", &secret, None)
            .await
            .status,
        200
    );
    assert_eq!(
        agent(
            &s,
            "POST",
            &format!("/api/v1/agent/runs/{job_id}/complete"),
            &secret,
            Some(json!({"state": "succeeded", "exit_code": 0, "stdout": "data"}))
        )
        .await
        .status,
        204
    );
    let job_path = format!("/api/v1/workspaces/{}/runs/{job_id}", l.ws);
    assert_eq!(s.get(&job_path, &bob()).await.json()["stdout"], "data");
    assert_eq!(
        s.delete(
            &format!(
                "/api/v1/workspaces/{}/securables/{}/grants/user:bob",
                l.ws, l.orders
            ),
            &ana()
        )
        .await
        .status,
        204
    );
    let restricted = s.get(&job_path, &bob()).await;
    assert_eq!(restricted.status, 200);
    let view = restricted.json();
    assert_eq!(view["result_access"], "restricted");
    assert_eq!(view["title"], "Job");
    assert!(
        view.get("spec").is_none()
            && view.get("stdout").is_none()
            && view.get("stderr").is_none()
            && view.get("result").is_none()
    );
    assert_eq!(s.get(&job_path, &ana()).await.json()["stdout"], "data");
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    assert_eq!(
        s.get(&path, &bob()).await.json()["result"]["rows"][0][0],
        7,
        "restoring access restores output"
    );
    let drop = s
        .delete(
            &format!("/api/v1/workspaces/{}/securables/{}", l.ws, l.orders),
            &ana(),
        )
        .await;
    assert_eq!(drop.status, 202, "{}", drop.text());
    let after_drop = s.get(&path, &bob()).await;
    assert_eq!(after_drop.status, 200);
    assert_eq!(after_drop.json()["result_access"], "restricted");
    assert_eq!(
        after_drop.json()["spec"]["sql"],
        "SELECT * FROM sales.raw.orders"
    );
}
both!(completed_results_require_current_input_access);

async fn compute_respects_input_and_output_classification(s: Server) {
    let l = lab(&s).await;
    let (secret, _) = attach(&s).await;
    let create = |name: &'static str, class: &'static str| {
        let (s, ws, schema) = (&s, l.ws.clone(), l.schema.clone());
        async move {
            let r = s.post(&format!("/api/v1/workspaces/{ws}/securables"), &ana(),
                json!({"kind": "volume", "parent_id": schema, "name": name, "classification": class})).await;
            assert_eq!(r.status, 201, "{}", r.text());
            r.json()["id"].as_str().unwrap().to_owned()
        }
    };
    let sealed = create("sealed_input", "sealed").await;
    let personal = create("personal_input", "personal").await;
    let personal_out = create("personal_output", "personal").await;
    assert_eq!(
        s.upload(&l.ws, &sealed, "opaque.bin", &ana(), b"opaque")
            .await
            .status,
        201
    );
    let jobs = format!("/api/v1/workspaces/{}/runs", l.ws);
    let ask = |input: String, output: String| {
        let (s, jobs) = (&s, jobs.clone());
        async move {
            s.post(
                &jobs,
                &ana(),
                json!({"script": "print(1)", "inputs": [input], "output": output}),
            )
            .await
        }
    };
    assert_eq!(ask(sealed.clone(), personal_out.clone()).await.status, 400);
    assert_eq!(ask(personal.clone(), l.out.clone()).await.status, 400);
    assert_eq!(ask(l.out.clone(), sealed).await.status, 400);
    let accepted = ask(personal, personal_out).await;
    assert_eq!(accepted.status, 202, "{}", accepted.text());
    assert_eq!(
        agent(&s, "POST", "/api/v1/agent/lease?wait_s=0", &secret, None)
            .await
            .status,
        200
    );

    let _ = s
        .table(&l.ws, &ana(), &l.schema, "sealed_table", "sealed")
        .await;
    let r = s
        .post(
            &format!("/api/v1/workspaces/{}/sql/statements", l.ws),
            &ana(),
            json!({"sql": "SELECT * FROM sales.raw.sealed_table", "wait_s": 0}),
        )
        .await;
    assert_eq!(r.status, 400);
}
both!(compute_respects_input_and_output_classification);

async fn admission_is_bounded_across_server_instances(s: Server) {
    // Separate App instances have separate in-process mutexes but share the database.
    let a = common::server_with_limits(s.app.db.clone(), 1024 * 1024, 1).await;
    let b = common::server_with_limits(s.app.db.clone(), 1024 * 1024, 1).await;
    let l = lab(&a).await;
    attach(&a).await;
    let url = format!("/api/v1/workspaces/{}/sql/statements", l.ws);
    let who = ana();
    let (one, two) = tokio::join!(
        a.post(&url, &who, json!({"sql": "SELECT 1", "wait_s": 0})),
        b.post(&url, &who, json!({"sql": "SELECT 2", "wait_s": 0}))
    );
    let mut statuses = [one.status, two.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [202, 429], "{} {}", one.text(), two.text());
    let refused = if one.status == 429 { &one } else { &two };
    assert_eq!(refused.content_type, "application/problem+json");
    let trail = items(
        &a.get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
            .await,
    );
    assert!(
        trail
            .iter()
            .any(|e| e["action"] == "sql.submit" && e["outcome"] == "denied")
    );
    let rita = common::person("rita")
        .also_in(common::RIVAL, common::RIVAL)
        .with_role("account-admin");
    assert_eq!(
        a.post(
            &format!("/api/v1/orgs/{}/account", common::RIVAL),
            &rita,
            json!({})
        )
        .await
        .status,
        201
    );
    let rival_ws = a.workspace(&rita, common::RIVAL, "Rival").await;
    let target = a
        .post(
            &format!("/api/v1/workspaces/{rival_ws}/compute/targets"),
            &rita,
            json!({"name": "rival", "max_memory_mb": 2048, "max_cpu_millis": 1000}),
        )
        .await;
    assert_eq!(target.status, 201, "{}", target.text());
    compute::authenticate_agent(&a.app, target.json()["secret"].as_str().unwrap())
        .await
        .unwrap();
    let rival = b
        .post(
            &format!("/api/v1/workspaces/{rival_ws}/sql/statements"),
            &rita,
            json!({"sql": "SELECT 1", "wait_s": 0}),
        )
        .await;
    assert_eq!(
        rival.status,
        202,
        "another organization has its own capacity: {}",
        rival.text()
    );
    let accepted = if one.status == 202 { one } else { two };
    let id = accepted.json()["id"].as_str().unwrap().to_owned();
    let cancelled = a
        .post(
            &format!("/api/v1/workspaces/{}/runs/{id}/cancel", l.ws),
            &ana(),
            json!({}),
        )
        .await;
    assert_eq!(cancelled.status, 202);
    let again = b
        .post(&url, &ana(), json!({"sql": "SELECT 3", "wait_s": 0}))
        .await;
    assert_eq!(again.status, 202, "{}", again.text());
}
both!(admission_is_bounded_across_server_instances);

async fn the_kinds_of_compute_that_can_be_created_say_whether_they_are_available(s: Server) {
    let l = lab(&s).await;
    let r = s
        .get(
            &format!("/api/v1/workspaces/{}/compute/providers", l.ws),
            &ana(),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let items = items(&r);
    let find = |id: &str| items.iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!(find("machine")["available"], true);
    assert_eq!(
        find("serverless")["available"],
        false,
        "this server has no token for a serverless endpoint"
    );
    assert_eq!(find("serverless")["idle_stop_s"], 60);

    // Without a token, creating serverless compute says what is missing.
    let made = s
        .post(
            &format!("/api/v1/workspaces/{}/compute/targets", main_ws(&s).await),
            &ana(),
            json!({"name": "warehouse", "kind": "serverless"}),
        )
        .await;
    assert_eq!(made.status, 400);
    assert!(made.text().contains("serverless token"));
}
both!(the_kinds_of_compute_that_can_be_created_say_whether_they_are_available);

async fn serverless_compute_is_started_and_stopped_by_admins_and_a_machine_is_not(s: Server) {
    let l = lab(&s).await;
    let (_, machine) = attach(&s).await;
    let ws = main_ws(&s).await;
    let power = |ws: &str, id: &str, what: &str, who: Who| {
        let (s, path) = (
            &s,
            format!("/api/v1/workspaces/{ws}/compute/targets/{id}/{what}"),
        );
        async move { s.post(&path, &who, json!({})).await }
    };
    // A machine is started and stopped on the machine itself.
    for what in ["start", "stop"] {
        let r = power(&ws, &machine, what, ana()).await;
        assert_eq!(r.status, 400, "{what}: {}", r.text());
        assert!(r.text().contains("on the machine itself"));
    }
    assert_eq!(power(&ws, "nope", "start", ana()).await.status, 404);
    // Not for people who are not admins of the account.
    s.assign(&l.ws, &[&bob()]).await;
    assert_eq!(power(&ws, &machine, "stop", bob()).await.status, 403);
}
both!(serverless_compute_is_started_and_stopped_by_admins_and_a_machine_is_not);
