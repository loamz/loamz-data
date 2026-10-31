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
    secrets: String,
    out: String,
}

async fn lab(s: &Server) -> Lab {
    let ws = s.acme().await;
    let c = s.make(&ws, &ana(), "catalog", "", "sales").await;
    let sc = s.make(&ws, &ana(), "schema", &c, "raw").await;
    let orders = s.make(&ws, &ana(), "table", &sc, "orders").await;
    let secrets = s.make(&ws, &ana(), "table", &sc, "secrets").await;
    let out = s.make(&ws, &ana(), "volume", &sc, "out").await;
    Lab {
        ws,
        schema: sc,
        orders,
        secrets,
        out,
    }
}

/// Attaches compute, and has it heard from, as an agent that has just asked for work would be. Returns its secret and id.
async fn attach(s: &Server) -> (String, String) {
    let r = s
        .post(
            &format!("/api/v1/orgs/{ACME}/compute/targets"),
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

// ---- tables hold files -------------------------------------------------------------------------------------------

async fn tables_hold_files(s: Server) {
    let l = lab(&s).await;
    let files = |id: &str| format!("/api/v1/workspaces/{}/tables/{id}/files", l.ws);
    let put = |id: String, who: Who, bytes: &'static [u8]| {
        let (s, ws) = (&s, l.ws.clone());
        async move {
            s.send(
                "PUT",
                &format!("/api/v1/workspaces/{ws}/tables/{id}/files/a.csv"),
                Some(&token(&who)),
                Body::from(bytes),
            )
            .await
        }
    };
    assert_eq!(
        put(l.orders.clone(), ana(), b"id,total\n1,10\n2,20\n")
            .await
            .status,
        201
    );
    assert_eq!(
        s.get(&files(&l.orders), &ana()).await.json()["items"][0]["path"],
        "a.csv"
    );

    // A table is read with SELECT and changed with MODIFY: a reader reads, and cannot change.
    s.assign(&l.ws, &[&bob()]).await;
    s.grant(&l.ws, &ana(), &l.orders, "user:bob", "reader")
        .await;
    assert_eq!(
        s.get(&format!("{}/a.csv", files(&l.orders)), &bob())
            .await
            .bytes,
        b"id,total\n1,10\n2,20\n"
    );
    assert_eq!(put(l.orders.clone(), bob(), b"x").await.status, 403);
    assert_eq!(
        s.get(&files(&l.secrets), &bob()).await.status,
        404,
        "the other table is not theirs"
    );

    // The two kinds do not answer to each other's addresses.
    assert_eq!(
        s.get(
            &format!("/api/v1/workspaces/{}/volumes/{}/files", l.ws, l.orders),
            &ana()
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.get(
            &format!("/api/v1/workspaces/{}/tables/{}/files", l.ws, l.out),
            &ana()
        )
        .await
        .status,
        404
    );

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
both!(tables_hold_files);

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
            .contains("no compute is answering")
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
    assert!(
        trail
            .iter()
            .any(|e| e["actor"] == "bob" && e["outcome"] == "denied")
    );
    assert!(trail.iter().any(|e| e["action"] == "sql.submit"));
}
both!(sql_is_checked_before_it_runs);

// ---- an agent does the work --------------------------------------------------------------------------------------

async fn an_agent_runs_a_statement_with_a_token_that_reaches_only_what_it_declared(s: Server) {
    let l = lab(&s).await;
    s.put(
        &format!(
            "/api/v1/workspaces/{}/tables/{}/files/a.csv",
            l.ws, l.orders
        ),
        &ana(),
        json!({}),
    )
    .await; // not a file upload: ignored
    s.send(
        "PUT",
        &format!(
            "/api/v1/workspaces/{}/tables/{}/files/a.csv",
            l.ws, l.orders
        ),
        Some(&token(&ana())),
        Body::from("id,total\n1,10\n"),
    )
    .await;
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
    let jt = work["token"].as_str().unwrap().to_owned();
    assert!(jt.starts_with("jt_"));

    // The token reads the table it declared, and the files of nothing else.
    let with_jt = |method: &str, path: String, body: &'static str| {
        let s = &s;
        let (method, jt) = (method.to_owned(), jt.clone());
        async move { s.send(&method, &path, Some(&jt), Body::from(body)).await }
    };
    let ws = &l.ws;
    assert_eq!(
        with_jt(
            "GET",
            format!("/api/v1/workspaces/{ws}/tables/{}/files/a.csv", l.orders),
            ""
        )
        .await
        .bytes,
        b"id,total\n1,10\n"
    );
    assert_eq!(
        with_jt(
            "GET",
            format!("/api/v1/workspaces/{ws}/tables/{}/files", l.secrets),
            ""
        )
        .await
        .status,
        404,
        "another table"
    );
    assert_eq!(
        with_jt(
            "PUT",
            format!("/api/v1/workspaces/{ws}/tables/{}/files/x.csv", l.orders),
            "x"
        )
        .await
        .status,
        403,
        "it read, it may not write"
    );
    assert_eq!(
        with_jt(
            "PUT",
            format!("/api/v1/workspaces/{ws}/volumes/{}/files/x", l.out),
            "x"
        )
        .await
        .status,
        404,
        "a volume it never declared"
    );
    // And it is good for files only: not for the catalog, the account, or anything else.
    for path in [
        format!("/api/v1/workspaces/{ws}/securables"),
        format!("/api/v1/orgs/{ACME}/admins"),
        "/api/v1/me".to_owned(),
        format!("/api/v1/workspaces/{ws}/runs"),
    ] {
        assert_eq!(with_jt("GET", path.clone(), "").await.status, 401, "{path}");
    }
    // Rights are the person's, checked each time: take the grant away and the token reads nothing more.
    assert_eq!(
        with_jt(
            "DELETE",
            format!("/api/v1/workspaces/{ws}/tables/{}/files/a.csv", l.orders),
            ""
        )
        .await
        .status,
        403
    );

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
    assert_eq!(
        with_jt(
            "GET",
            format!("/api/v1/workspaces/{ws}/tables/{}/files", l.orders),
            ""
        )
        .await
        .status,
        401,
        "a finished run's token no longer works"
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
both!(an_agent_runs_a_statement_with_a_token_that_reaches_only_what_it_declared);

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
    let put = |id: &str, bytes: &'static str| {
        let (s, jt, path) = (
            &s,
            jt.clone(),
            format!(
                "/api/v1/workspaces/{}/volumes/{id}/files/runs/x/out.txt",
                l.ws
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
        s.download(&l.ws, &l.out, "runs/x/out.txt", &ana())
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
    s.post(
        &format!("/api/v1/workspaces/{}/runs", l.ws),
        &ana(),
        json!({"script": "1", "inputs": [l.orders], "output": l.out}),
    )
    .await;
    let jt = agent(&s, "POST", "/api/v1/agent/lease", &secret, None)
        .await
        .json()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    // The same catalog is visible from the other workspace, but this token is not for it.
    assert_eq!(
        s.send(
            "GET",
            &format!("/api/v1/workspaces/{elsewhere}/tables/{}/files", l.orders),
            Some(&jt),
            Body::empty()
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.send(
            "GET",
            &format!("/api/v1/workspaces/{}/tables/{}/files", l.ws, l.orders),
            Some(&jt),
            Body::empty()
        )
        .await
        .status,
        200
    );
}
both!(a_token_is_for_one_workspace);

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
    let lease = agent(&s, "POST", "/api/v1/agent/lease", &secret, None)
        .await
        .json();
    let jt = lease["token"].as_str().unwrap().to_owned();
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
    assert_eq!(
        s.send(
            "GET",
            &format!("/api/v1/workspaces/{}/tables/{}/files", l.ws, l.orders),
            Some(&jt),
            Body::empty()
        )
        .await
        .status,
        401
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
            &format!("/api/v1/orgs/{ACME}/compute/targets/{id}/secret"),
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
        s.delete(&format!("/api/v1/orgs/{ACME}/compute/targets/{id}"), &bob())
            .await
            .status,
        403
    );
    assert_eq!(
        s.delete(&format!("/api/v1/orgs/{ACME}/compute/targets/{id}"), &ana())
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

async fn compute_is_attached_by_the_account(s: Server) {
    let l = lab(&s).await;
    let url = format!("/api/v1/orgs/{ACME}/compute/targets");
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
    s.assign(&l.ws, &[&bob()]).await;
    let seen = items(
        &s.get(
            &format!("/api/v1/workspaces/{}/compute/targets", l.ws),
            &bob(),
        )
        .await,
    );
    assert_eq!((seen.len(), seen[0]["online"].clone()), (1, json!(false)));
}
both!(compute_is_attached_by_the_account);
