//! The server end to end, through the router, on `SQLite` and on `Postgres`.
//! `Postgres` tests are `#[ignore]`d; run them with `LOAMZ_TEST_POSTGRES_URL=... cargo test -- --include-ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod common;

use common::{
    Server, forged, fresh_db, member, person, server, server_with, token, wrong_audience,
};
use loamz_data_server::governance::lifecycle::service::run_once;
use loamz_data_server::kernel::now_ms;
use serde_json::json;

/// Runs one scenario on `SQLite`, and the same scenario on `Postgres`.
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

fn ana() -> common::Who<'static> {
    member("ana", "acme")
}
fn bob() -> common::Who<'static> {
    member("bob", "acme")
}
fn cy() -> common::Who<'static> {
    member("cy", "acme")
}
fn zed() -> common::Who<'static> {
    member("zed", "acme")
}

/// ana (the organization's owner) builds catalog > schema > volume and returns their ids.
async fn tree(s: &Server) -> (String, String, String) {
    let c = s.make(&ana(), "catalog", "", "sales").await;
    let sc = s.make(&ana(), "schema", &c, "raw").await;
    let v = s.make(&ana(), "volume", &sc, "uploads").await;
    (c, sc, v)
}

fn problem(r: &common::Reply) -> serde_json::Value {
    assert_eq!(
        r.content_type,
        "application/problem+json",
        "{}",
        String::from_utf8_lossy(&r.bytes)
    );
    r.json()
}

// ---- identity ----------------------------------------------------------------------------------------------------

async fn identity(s: Server) {
    assert_eq!(
        s.send("GET", "/healthz", None, axum::body::Body::empty())
            .await
            .status,
        200
    );
    assert_eq!(
        s.send("GET", "/readyz", None, axum::body::Body::empty())
            .await
            .status,
        200
    );

    let none = s
        .send("GET", "/api/v1/me", None, axum::body::Body::empty())
        .await;
    assert_eq!(none.status, 401);
    assert_eq!(problem(&none)["status"], 401);

    for bad in [forged(&ana()), wrong_audience(&ana()), "garbage".to_owned()] {
        let r = s
            .send("GET", "/api/v1/me", Some(&bad), axum::body::Body::empty())
            .await;
        assert_eq!(r.status, 401, "a token we cannot trust is refused");
    }

    let me = s.get("/api/v1/me", &ana()).await;
    assert_eq!(me.status, 200);
    let me = me.json();
    assert_eq!(
        (
            me["subject"].as_str(),
            me["tenant"].as_str(),
            me["tenant_owner"].as_bool()
        ),
        (Some("ana"), Some("o-acme"), Some(true))
    );
    assert_eq!(
        s.get("/api/v1/me", &bob()).await.json()["tenant_owner"],
        false
    );
    assert_eq!(
        s.get("/api/v1/me", &person("pia")).await.json()["tenant"],
        "u-pia"
    );

    let unknown = s.get("/api/v1/nothing", &ana()).await;
    assert_eq!(unknown.status, 404);
    problem(&unknown);
}
both!(identity);

// ---- catalogs ----------------------------------------------------------------------------------------------------

async fn catalog_rules(s: Server) {
    let (c, sc, v) = tree(&s).await;

    let bad_name = s
        .post(
            "/api/v1/securables",
            &ana(),
            json!({"kind": "catalog", "name": "Sales"}),
        )
        .await;
    assert_eq!(bad_name.status, 400);
    problem(&bad_name);
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &ana(),
            json!({"kind": "catalog", "name": "sales"})
        )
        .await
        .status,
        409,
        "same name twice"
    );
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &ana(),
            json!({"kind": "volume", "parent_id": c, "name": "x"})
        )
        .await
        .status,
        400,
        "a volume goes in a schema"
    );
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &ana(),
            json!({"kind": "volume", "parent_id": sc, "name": "t", "consistency": "transaction"})
        )
        .await
        .status,
        400,
        "a volume cannot promise transactions"
    );
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &ana(),
            json!({"kind": "table", "parent_id": sc, "name": "t", "placement": "s3"})
        )
        .await
        .status,
        400,
        "no such placement"
    );
    assert_eq!(
        s.post("/api/v1/securables", &ana(), "not json".into())
            .await
            .status,
        400
    );

    let table = s.post("/api/v1/securables", &ana(), json!({"kind": "table", "parent_id": sc, "name": "orders", "classification": "personal"})).await;
    assert_eq!(table.status, 201);
    let t = table.json();
    assert_eq!(
        (
            t["classification"].as_str(),
            t["placement"].as_str(),
            t["consistency"].as_str(),
            t["owner"].as_str()
        ),
        (
            Some("personal"),
            Some("local"),
            Some("transaction"),
            Some("ana")
        )
    );

    let top = s.get("/api/v1/securables", &ana()).await.json();
    assert_eq!(top["items"].as_array().unwrap().len(), 1);
    let inside = s
        .get(&format!("/api/v1/securables?parent_id={sc}"), &ana())
        .await
        .json();
    assert_eq!(inside["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        s.get(&format!("/api/v1/securables/{v}"), &ana())
            .await
            .json()["name"],
        "uploads"
    );
}
both!(catalog_rules);

// ---- who may do what ---------------------------------------------------------------------------------------------

async fn roles(s: Server) {
    let (c, sc, v) = tree(&s).await;
    s.grant(&ana(), &c, "bob", "reader").await;
    s.grant(&ana(), &sc, "cy", "contributor").await;
    assert_eq!(s.upload(&v, "a.txt", &ana(), b"hello").await.status, 201);

    // Reader: can look and read; cannot write, create, or manage.
    assert_eq!(
        s.get(&format!("/api/v1/securables/{v}"), &bob())
            .await
            .status,
        200
    );
    assert_eq!(s.download(&v, "a.txt", &bob()).await.bytes, b"hello");
    assert_eq!(s.upload(&v, "b.txt", &bob(), b"x").await.status, 403);
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &bob(),
            json!({"kind": "volume", "parent_id": sc, "name": "mine"})
        )
        .await
        .status,
        403
    );
    let denied = s
        .call(
            "PUT",
            &format!("/api/v1/securables/{sc}/grants"),
            &bob(),
            Some(json!({"principal": "bob", "role": "owner"})),
        )
        .await;
    assert_eq!(
        denied.status, 403,
        "a reader cannot make themselves an owner"
    );
    problem(&denied);
    assert_eq!(
        s.delete(&format!("/api/v1/securables/{v}"), &bob())
            .await
            .status,
        403
    );

    // Contributor: can write and create inside the schema; cannot manage.
    assert_eq!(s.upload(&v, "b.txt", &cy(), b"x").await.status, 201);
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &cy(),
            json!({"kind": "volume", "parent_id": sc, "name": "cys"})
        )
        .await
        .status,
        201
    );
    assert_eq!(
        s.delete(&format!("/api/v1/securables/{v}"), &cy())
            .await
            .status,
        403
    );
    assert_eq!(
        s.get(&format!("/api/v1/securables/{sc}/grants"), &cy())
            .await
            .status,
        403
    );
    // cy has no right at the catalog, above the schema they were given, so it does not exist for them.
    assert_eq!(
        s.get(&format!("/api/v1/securables/{c}/grants"), &cy())
            .await
            .status,
        404
    );
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &cy(),
            json!({"kind": "schema", "parent_id": c, "name": "other"})
        )
        .await
        .status,
        404,
        "cy cannot even see the catalog"
    );

    // A stranger is told nothing exists, whether they ask for a read, a write or a drop.
    for r in [
        s.get(&format!("/api/v1/securables/{v}"), &zed()).await,
        s.download(&v, "a.txt", &zed()).await,
        s.upload(&v, "z.txt", &zed(), b"x").await,
        s.delete(&format!("/api/v1/securables/{v}"), &zed()).await,
        s.get(&format!("/api/v1/volumes/{v}/files"), &zed()).await,
    ] {
        assert_eq!(r.status, 404);
    }
    assert_eq!(
        s.get("/api/v1/securables", &zed()).await.json()["items"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "and sees nothing in lists"
    );

    // Owner role: can manage; revoking takes the access away again.
    s.grant(&ana(), &v, "zed", "owner").await;
    assert_eq!(
        s.get(&format!("/api/v1/securables/{v}/grants"), &zed())
            .await
            .status,
        200
    );
    let r = s
        .delete(&format!("/api/v1/securables/{v}/grants/zed"), &ana())
        .await;
    assert_eq!(r.status, 204);
    assert_eq!(
        s.get(&format!("/api/v1/securables/{v}"), &zed())
            .await
            .status,
        404
    );

    // The grants list names roles.
    let list = s
        .get(&format!("/api/v1/securables/{c}/grants"), &ana())
        .await
        .json();
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["principal"] == "bob" && i["role"] == "reader")
    );

    // Refusals are in the audit trail, with who asked.
    let trail = s.get("/api/v1/audit?limit=500", &ana()).await.json();
    let denied: Vec<_> = trail["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["outcome"] == "denied")
        .collect();
    assert!(
        denied.iter().any(|e| e["actor"] == "bob"),
        "the reader's refused write is recorded"
    );
}
both!(roles);

async fn tenants_are_separate(s: Server) {
    let (_, _, v) = tree(&s).await;
    s.upload(&v, "a.txt", &ana(), b"secret").await;
    // The same person name in another organization, and an organization owner of another tenant, see nothing.
    let other = member("ana", "rival");
    assert_eq!(
        s.get(&format!("/api/v1/securables/{v}"), &other)
            .await
            .status,
        404
    );
    assert_eq!(s.download(&v, "a.txt", &other).await.status, 404);
    assert_eq!(
        s.get("/api/v1/securables", &other).await.json()["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        s.get(&format!("/api/v1/securables/{v}"), &person("ana"))
            .await
            .status,
        404,
        "ana's personal space is not acme's"
    );
    // A grant cannot reach across either: naming acme's id from another tenant finds nothing.
    let r = s
        .call(
            "PUT",
            &format!("/api/v1/securables/{v}/grants"),
            &other,
            Some(json!({"principal": "ana", "role": "owner"})),
        )
        .await;
    assert_eq!(r.status, 404);
}
both!(tenants_are_separate);

async fn agent_for_a_person(s: Server) {
    let (c, _, v) = tree(&s).await;
    s.upload(&v, "a.txt", &ana(), b"hello").await;
    s.grant(&ana(), &c, "bob", "contributor").await;
    let agent_for_bob = common::Who {
        sub: "bob",
        org: Some("acme"),
        acting: Some("agent-1"),
    };

    // The agent has been given nothing yet: bob can read, but the agent cannot, so the pair cannot.
    assert_eq!(s.download(&v, "a.txt", &bob()).await.status, 200);
    assert_eq!(s.download(&v, "a.txt", &agent_for_bob).await.status, 404);

    // Given read, the agent can read for bob but still cannot write, though bob can.
    s.grant(&ana(), &c, "agent-1", "reader").await;
    assert_eq!(s.download(&v, "a.txt", &agent_for_bob).await.status, 200);
    assert_eq!(
        s.upload(&v, "b.txt", &agent_for_bob, b"x").await.status,
        403
    );
    assert_eq!(s.upload(&v, "b.txt", &bob(), b"x").await.status, 201);

    // The agent cannot lean on someone else's rights: acting for zed (who has nothing) gets nothing.
    let agent_for_zed = common::Who {
        sub: "zed",
        org: Some("acme"),
        acting: Some("agent-1"),
    };
    assert_eq!(s.download(&v, "a.txt", &agent_for_zed).await.status, 404);
    // An owner's agent does not inherit the owner's tenant-wide rights.
    let agent_for_ana = common::Who {
        sub: "ana",
        org: Some("acme"),
        acting: Some("agent-2"),
    };
    assert_eq!(s.download(&v, "a.txt", &agent_for_ana).await.status, 404);

    // Both identities are in the audit trail.
    let trail = s.get("/api/v1/audit?limit=500", &ana()).await.json();
    assert!(
        trail["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["actor"] == "agent-1" && e["on_behalf_of"] == "bob")
    );
}
both!(agent_for_a_person);

// ---- files -------------------------------------------------------------------------------------------------------

async fn files(s: Server) {
    let (_, _, v) = tree(&s).await;
    assert_eq!(s.upload(&v, "a/b/c.txt", &ana(), b"deep").await.status, 201);
    let got = s.download(&v, "a/b/c.txt", &ana()).await;
    assert_eq!(
        (got.status, got.bytes.as_slice()),
        (200, b"deep".as_slice())
    );
    let list = s
        .get(&format!("/api/v1/volumes/{v}/files"), &ana())
        .await
        .json();
    assert_eq!(list["items"][0]["path"], "a/b/c.txt");
    assert_eq!(
        s.upload(&v, "a/b/c.txt", &ana(), b"replaced").await.status,
        201
    );
    assert_eq!(s.download(&v, "a/b/c.txt", &ana()).await.bytes, b"replaced");

    // A path never leaves the volume.
    for evil in [
        "..%2Fx",
        "a%2F..%2F..%2Fx",
        "%2Fetc%2Fpasswd",
        "a%5Cb",
        ".upload-1",
    ] {
        let r = s.upload(&v, evil, &ana(), b"x").await;
        assert!(
            r.status == 400 || r.status == 404,
            "{evil} gave {}",
            r.status
        );
        assert!(!s.volume_dir(&v).join("x").exists());
    }

    assert_eq!(
        s.delete(&format!("/api/v1/volumes/{v}/files/a/b/c.txt"), &ana())
            .await
            .status,
        204
    );
    assert_eq!(s.download(&v, "a/b/c.txt", &ana()).await.status, 404);

    // Not a volume.
    let (c, _, _) = (s.make(&ana(), "catalog", "", "other").await, 0, 0);
    assert_eq!(s.upload(&c, "x", &ana(), b"x").await.status, 404);
}
both!(files);

#[tokio::test]
async fn an_upload_over_the_limit_is_refused_and_leaves_nothing() {
    let s = server_with(fresh_db(false).await, 10).await;
    let (_, _, v) = tree(&s).await;
    assert_eq!(s.upload(&v, "ok.bin", &ana(), &[1; 10]).await.status, 201);
    let r = s.upload(&v, "big.bin", &ana(), &[1; 11]).await;
    assert_eq!(r.status, 413);
    problem(&r);
    let list = s
        .get(&format!("/api/v1/volumes/{v}/files"), &ana())
        .await
        .json();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn downloads_are_attachments_the_browser_may_not_sniff() {
    let s = server(fresh_db(false).await).await;
    let (_, _, v) = tree(&s).await;
    s.upload(&v, "page.html", &ana(), b"<script>1</script>")
        .await;
    let r = s
        .send(
            "GET",
            &format!("/api/v1/volumes/{v}/files/page.html"),
            Some(&token(&ana())),
            axum::body::Body::empty(),
        )
        .await;
    assert_eq!(r.content_type, "application/octet-stream");
}

// ---- lifecycle ---------------------------------------------------------------------------------------------------

async fn drop_is_a_durable_task(s: Server) {
    let (_, sc, v) = tree(&s).await;
    s.upload(&v, "a.txt", &ana(), b"bytes").await;
    assert!(s.volume_dir(&v).exists());

    let r = s.delete(&format!("/api/v1/securables/{sc}"), &ana()).await;
    assert_eq!(r.status, 202);
    let task = r.json()["task_id"].as_str().unwrap().to_owned();

    // Gone from view at once, before any bytes are removed.
    assert_eq!(
        s.get(&format!("/api/v1/securables/{sc}"), &ana())
            .await
            .status,
        404
    );
    assert_eq!(s.download(&v, "a.txt", &ana()).await.status, 404);
    assert!(
        s.volume_dir(&v).exists(),
        "the bytes are removed by the task, not by the request"
    );
    assert_eq!(
        s.post(
            "/api/v1/securables",
            &ana(),
            json!({"kind": "schema", "parent_id": "", "name": "x"})
        )
        .await
        .status,
        400
    );
    assert_eq!(
        s.get(&format!("/api/v1/tasks/{task}"), &ana()).await.json()["state"],
        "pending"
    );

    assert_eq!(s.work().await, 1);
    assert!(!s.volume_dir(&v).exists(), "the files are gone");
    assert_eq!(
        s.get(&format!("/api/v1/tasks/{task}"), &ana()).await.json()["state"],
        "done"
    );
    let trail = s.get("/api/v1/audit?limit=500", &ana()).await.json();
    assert!(
        trail["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "drop.done")
    );
}
both!(drop_is_a_durable_task);

async fn a_task_killed_half_way_is_taken_up_again(s: Server) {
    let (_, sc, v) = tree(&s).await;
    s.upload(&v, "a.txt", &ana(), b"bytes").await;
    s.delete(&format!("/api/v1/securables/{sc}"), &ana()).await;

    // A worker leases the task and dies: nothing finishes, and the lease is still held.
    let t0 = now_ms();
    sqlx::query("UPDATE tasks SET lease_until = $1, attempts = 1")
        .bind(t0 + 60_000)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    assert!(
        !run_once(&s.app, t0).await.unwrap(),
        "while the lease is held nobody else takes it"
    );
    assert!(s.volume_dir(&v).exists());

    // Half the work had been done before the crash: the files are already gone.
    std::fs::remove_dir_all(s.volume_dir(&v)).unwrap();

    // The lease runs out; another worker finishes it, and the repeat of a step does no harm.
    assert!(run_once(&s.app, t0 + 61_000).await.unwrap());
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM securables")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(left, 1, "only the catalog is left");
    let state: String = sqlx::query_scalar("SELECT state FROM tasks")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(state, "done");
}
both!(a_task_killed_half_way_is_taken_up_again);

async fn a_task_that_keeps_failing_is_given_up_on_and_says_so(s: Server) {
    let (_, sc, _) = tree(&s).await;
    s.delete(&format!("/api/v1/securables/{sc}"), &ana()).await;
    sqlx::query("UPDATE tasks SET kind = 'unknown'")
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let mut now = now_ms();
    for _ in 0..10 {
        now += 120_000;
        run_once(&s.app, now).await.unwrap();
    }
    let (state, attempts): (String, i64) = sqlx::query_as("SELECT state, attempts FROM tasks")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!((state.as_str(), attempts), ("failed", 5));
    let trail = s.get("/api/v1/audit?limit=500", &ana()).await.json();
    assert!(
        trail["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "unknown.failed")
    );
}
both!(a_task_that_keeps_failing_is_given_up_on_and_says_so);

async fn erase_removes_a_person_and_keeps_the_audit(s: Server) {
    let c = s.make(&ana(), "catalog", "", "shared").await;
    s.grant(&ana(), &c, "bob", "contributor").await;
    let bobs_schema = s.make(&bob(), "schema", &c, "bobs").await;
    let bobs_volume = s.make(&bob(), "volume", &bobs_schema, "diary").await;
    s.upload(&bobs_volume, "me.txt", &bob(), b"private").await;
    let anas_schema = s.make(&ana(), "schema", &c, "anas").await;
    let anas_volume = s.make(&ana(), "volume", &anas_schema, "keep").await;
    s.upload(&anas_volume, "x.txt", &ana(), b"mine").await;

    // Only a tenant owner may erase someone.
    assert_eq!(
        s.post("/api/v1/erase", &bob(), json!({"subject": "bob"}))
            .await
            .status,
        403
    );

    let r = s
        .post("/api/v1/erase", &ana(), json!({"subject": "bob"}))
        .await;
    assert_eq!(r.status, 202);
    assert_eq!(s.work().await, 1);

    assert!(!s.volume_dir(&bobs_volume).exists(), "bob's bytes are gone");
    assert_eq!(
        s.get(&format!("/api/v1/securables/{bobs_volume}"), &ana())
            .await
            .status,
        404
    );
    assert_eq!(
        s.get(&format!("/api/v1/securables/{c}"), &bob())
            .await
            .status,
        404,
        "bob's grants are gone too"
    );
    let grants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grants WHERE principal = 'bob'")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(grants, 0);
    assert_eq!(
        s.download(&anas_volume, "x.txt", &ana()).await.bytes,
        b"mine",
        "what is not bob's is untouched"
    );

    // The audit trail still says what happened, and it still holds together.
    let trail = s.get("/api/v1/audit?limit=500", &ana()).await.json();
    let actions: Vec<&str> = trail["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"erase.requested") && actions.contains(&"erase.done"));
    assert!(
        actions.contains(&"file.write"),
        "bob's earlier actions are still recorded"
    );
    assert_eq!(
        s.get("/api/v1/audit/verify", &ana()).await.json()["intact"],
        true
    );
}
both!(erase_removes_a_person_and_keeps_the_audit);

// ---- audit -------------------------------------------------------------------------------------------------------

async fn audit_chain(s: Server) {
    let (_, _, v) = tree(&s).await;
    s.upload(&v, "a.txt", &ana(), b"x").await;
    let ok = s.get("/api/v1/audit/verify", &ana()).await.json();
    assert_eq!(ok["intact"], true);
    assert!(ok["entries"].as_i64().unwrap() >= 4);

    // Reading the trail needs manage on the tenant.
    assert_eq!(s.get("/api/v1/audit", &bob()).await.status, 403);
    s.grant(&ana(), "root", "bob", "reader").await;
    assert_eq!(s.get("/api/v1/audit", &bob()).await.status, 403);

    // Changing an old entry, or taking one out, is found.
    sqlx::query("UPDATE audit SET outcome = 'ok' WHERE seq = 2")
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert!(still > 2);
    sqlx::query("UPDATE audit SET action = 'create.nothing' WHERE seq = 2")
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let broken = s.get("/api/v1/audit/verify", &ana()).await.json();
    assert_eq!(
        (broken["intact"].clone(), broken["first_broken"].clone()),
        (json!(false), json!(2))
    );
}
both!(audit_chain);

// ---- the contract ------------------------------------------------------------------------------------------------

/// Every route the server has is in the `OpenAPI` document, and every path in the document is a route.
#[tokio::test]
async fn the_routes_and_the_spec_agree() {
    let s = server(fresh_db(false).await).await;
    let doc: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../../../specs/data/v1/openapi.yaml")).unwrap();
    let mut in_spec = std::collections::BTreeSet::new();
    for (path, item) in doc["paths"].as_mapping().unwrap() {
        for (method, _) in item.as_mapping().unwrap() {
            let m = method.as_str().unwrap();
            if ["get", "put", "post", "delete"].contains(&m) {
                in_spec.insert((m.to_uppercase(), path.as_str().unwrap().to_owned()));
            }
        }
    }
    // The router writes a wildcard as `{*path}`; the document says `{path}`.
    let in_router: std::collections::BTreeSet<_> = s
        .routes
        .iter()
        .map(|(m, p)| ((*m).to_owned(), p.replace("{*path}", "{path}")))
        .collect();
    assert_eq!(
        in_router, in_spec,
        "left: the server's routes, right: the spec"
    );
}
