//! The server end to end, through the router, on `SQLite` and on `Postgres`.
//! `Postgres` tests are `#[ignore]`d; run them with `LOAMZ_TEST_POSTGRES_URL=... cargo test -- --include-ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod common;

use common::{
    ACME, RIVAL, Server, Who, ana, bob, cy, forged, fresh_db, member, person, server, server_with,
    token, wrong_audience, wrong_issuer, zed,
};
use loamz_data_server::{governance::lifecycle::service::run_once, kernel::now_ms};
use serde_json::{Value, json};

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

fn problem(r: &common::Reply) -> Value {
    assert_eq!(r.content_type, "application/problem+json", "{}", r.text());
    r.json()
}

fn items(r: &common::Reply) -> Vec<Value> {
    r.json()["items"].as_array().unwrap().clone()
}

/// ana (the account admin) builds catalog > schema > volume in the workspace and returns their ids.
async fn tree(s: &Server, ws: &str) -> (String, String, String) {
    let c = s.make(ws, &ana(), "catalog", "", "sales").await;
    let sc = s.make(ws, &ana(), "schema", &c, "raw").await;
    let v = s.make(ws, &ana(), "volume", &sc, "uploads").await;
    (c, sc, v)
}

fn securables(ws: &str) -> String {
    format!("/api/v1/workspaces/{ws}/securables")
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

    for bad in [
        forged(&ana()),
        wrong_audience(&ana()),
        wrong_issuer(&ana()),
        "garbage".to_owned(),
    ] {
        let r = s
            .send("GET", "/api/v1/me", Some(&bad), axum::body::Body::empty())
            .await;
        assert_eq!(r.status, 401, "a token we cannot trust is refused");
    }

    // A person with no organization has their own space and a workspace in it.
    let me = s.get("/api/v1/me", &person("pia")).await.json();
    assert_eq!(me["subject"], "pia");
    assert_eq!(me["orgs"][0]["kind"], "personal");
    assert_eq!(me["workspaces"][0]["name"], "Personal");
    assert_eq!(me["workspaces"][0]["admin"], true);
    let again = s.get("/api/v1/me", &person("pia")).await.json();
    assert_eq!(
        again["workspaces"].as_array().unwrap().len(),
        1,
        "asking again does not make another"
    );

    // A member of an organization without an account is told they may make one, if they hold the role.
    let me = s.get("/api/v1/me", &ana()).await.json();
    let acme = me["orgs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["alias"] == "acme")
        .unwrap()
        .clone();
    assert_eq!(
        (
            acme["has_account"].clone(),
            acme["can_create_account"].clone()
        ),
        (json!(false), json!(true))
    );
    let me = s.get("/api/v1/me", &bob()).await.json();
    assert_eq!(
        me["orgs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["alias"] == "acme")
            .unwrap()["can_create_account"],
        false
    );

    let unknown = s.get("/api/v1/nothing", &ana()).await;
    assert_eq!(unknown.status, 404);
    problem(&unknown);
}
both!(identity);

async fn a_persons_own_space_is_theirs_alone(s: Server) {
    let me = s.get("/api/v1/me", &person("pia")).await.json();
    let ws = me["workspaces"][0]["id"].as_str().unwrap().to_owned();
    let c = s
        .post(
            &securables(&ws),
            &person("pia"),
            json!({"kind": "catalog", "name": "mine"}),
        )
        .await;
    assert_eq!(c.status, 201, "{}", c.text());
    // Nobody else can open the workspace, or name its id as their own.
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{ws}"), &person("pete"))
            .await
            .status,
        404
    );
    assert_eq!(s.get(&securables(&ws), &person("pete")).await.status, 404);
    assert_eq!(s.get(&securables(&ws), &ana()).await.status, 404);
}
both!(a_persons_own_space_is_theirs_alone);

// ---- the account and its workspaces ------------------------------------------------------------------------------

async fn accounts(s: Server) {
    let path = format!("/api/v1/orgs/{ACME}/account");
    assert_eq!(
        s.post(&path, &bob(), json!({})).await.status,
        403,
        "a member without the role cannot create the account"
    );
    assert_eq!(
        s.post(&format!("/api/v1/orgs/{RIVAL}/account"), &ana(), json!({}))
            .await
            .status,
        404,
        "nor can anyone for an organization they are not in"
    );
    assert_eq!(
        s.post(&path, &ana().acting_as("agent-1"), json!({}))
            .await
            .status,
        403,
        "an agent never creates an account"
    );
    assert_eq!(s.post(&path, &ana(), json!({})).await.status, 201);
    assert_eq!(
        s.post(&path, &ana(), json!({})).await.status,
        409,
        "only once"
    );

    let admins = format!("/api/v1/orgs/{ACME}/admins");
    assert_eq!(
        s.get(&admins, &ana()).await.json()["items"],
        json!(["user:ana"])
    );
    assert_eq!(
        s.get(&admins, &bob()).await.status,
        403,
        "members cannot read who the admins are"
    );
    assert_eq!(
        s.get(&admins, &person("pete")).await.status,
        404,
        "outsiders cannot tell the account exists"
    );

    // Admins can be added and removed, and there is always one.
    assert_eq!(
        s.put(
            &format!("/api/v1/orgs/{ACME}/admins"),
            &ana(),
            json!({"principal": "user:bob"})
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.delete(&format!("/api/v1/orgs/{ACME}/admins/user:ana"), &bob())
            .await
            .status,
        204
    );
    let last = s
        .delete(&format!("/api/v1/orgs/{ACME}/admins/user:bob"), &bob())
        .await;
    assert_eq!(last.status, 409);
    problem(&last);
    assert_eq!(
        s.put(
            &format!("/api/v1/orgs/{ACME}/admins"),
            &bob(),
            json!({"principal": "nobody"})
        )
        .await
        .status,
        400
    );
}
both!(accounts);

async fn workspaces(s: Server) {
    let main = s.acme().await;
    let org = format!("/api/v1/orgs/{ACME}/workspaces");

    // Only account admins create workspaces; names are plain and unique.
    assert_eq!(
        s.post(&org, &bob(), json!({"name": "Rogue"})).await.status,
        403
    );
    assert_eq!(
        s.post(&org, &ana(), json!({"name": "<b>"})).await.status,
        400
    );
    assert_eq!(
        s.post(&org, &ana(), json!({"name": "Main"})).await.status,
        409
    );

    // Not assigned: it is not there. Assigned as a user: you can work in it, not manage it.
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}"), &bob())
            .await
            .status,
        404
    );
    assert_eq!(
        s.get(&org, &bob()).await.json()["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    s.assign(&main, &[&bob()]).await;
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}"), &bob())
            .await
            .status,
        200
    );
    assert_eq!(
        s.get(&org, &bob()).await.json()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}/members"), &bob())
            .await
            .status,
        403
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/members"),
            &bob(),
            json!({"principal": "user:cy", "role": "user"})
        )
        .await
        .status,
        403
    );

    // A workspace admin manages who is assigned.
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/members"),
            &ana(),
            json!({"principal": "user:bob", "role": "admin"})
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/members"),
            &bob(),
            json!({"principal": "user:cy", "role": "user"})
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}/members"), &bob())
            .await
            .status,
        200
    );
    assert_eq!(
        s.delete(
            &format!("/api/v1/workspaces/{main}/members/user:cy"),
            &bob()
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}"), &cy())
            .await
            .status,
        404,
        "unassigned, it is gone for them again"
    );

    // Groups are assigned like people, and leaving the group ends it.
    s.put(
        &format!("/api/v1/workspaces/{main}/members"),
        &ana(),
        json!({"principal": "group:/eng", "role": "user"}),
    )
    .await;
    assert_eq!(
        s.get(
            &format!("/api/v1/workspaces/{main}"),
            &cy().in_group("/eng")
        )
        .await
        .status,
        200
    );
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}"), &cy())
            .await
            .status,
        404
    );

    // A workspace always has an admin, and an account admin is one even when not assigned.
    s.put(
        &format!("/api/v1/workspaces/{main}/members"),
        &ana(),
        json!({"principal": "user:ana", "role": "user"}),
    )
    .await;
    s.delete(
        &format!("/api/v1/workspaces/{main}/members/user:ana"),
        &ana(),
    )
    .await;
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}"), &ana())
            .await
            .json()["account_admin"],
        true
    );
    let last = s
        .delete(
            &format!("/api/v1/workspaces/{main}/members/user:bob"),
            &ana(),
        )
        .await;
    assert_eq!(
        last.status,
        409,
        "bob is the last assigned admin: {}",
        last.text()
    );

    // Entitlements are for assigned principals, and from a known list.
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/entitlements"),
            &ana(),
            json!({"principal": "user:zed", "entitlement": "sql-access"})
        )
        .await
        .status,
        400
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/entitlements"),
            &ana(),
            json!({"principal": "group:/eng", "entitlement": "sql-access"})
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/entitlements"),
            &ana(),
            json!({"principal": "group:/eng", "entitlement": "fly"})
        )
        .await
        .status,
        400
    );
    assert_eq!(
        items(
            &s.get(&format!("/api/v1/workspaces/{main}/entitlements"), &ana())
                .await
        )
        .len(),
        1
    );

    // Rename and delete are for the account.
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}"),
            &bob(),
            json!({"name": "Mine"})
        )
        .await
        .status,
        403
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}"),
            &ana(),
            json!({"name": "Core"})
        )
        .await
        .json()["name"],
        "Core"
    );
    assert_eq!(
        s.delete(&format!("/api/v1/workspaces/{main}"), &bob())
            .await
            .status,
        403
    );
    assert_eq!(
        s.delete(&format!("/api/v1/workspaces/{main}"), &ana())
            .await
            .status,
        204
    );
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{main}"), &ana())
            .await
            .status,
        404
    );
}
both!(workspaces);

// ---- catalogs ----------------------------------------------------------------------------------------------------

async fn catalog_rules(s: Server) {
    let ws = s.acme().await;
    let (c, sc, v) = tree(&s, &ws).await;
    let make = |body: Value| async { s.post(&securables(&ws), &ana(), body).await };

    let bad_name = make(json!({"kind": "catalog", "name": "Sales"})).await;
    assert_eq!(bad_name.status, 400);
    problem(&bad_name);
    assert_eq!(
        make(json!({"kind": "catalog", "name": "sales"}))
            .await
            .status,
        409,
        "same name twice"
    );
    assert_eq!(
        make(json!({"kind": "volume", "parent_id": c, "name": "x"}))
            .await
            .status,
        400,
        "a volume goes in a schema"
    );
    assert_eq!(
        make(json!({"kind": "volume", "parent_id": sc, "name": "t", "consistency": "transaction"}))
            .await
            .status,
        400,
        "a volume cannot promise transactions"
    );
    assert_eq!(
        make(json!({"kind": "volume", "parent_id": sc, "name": "t", "placement": "s3"}))
            .await
            .status,
        400,
        "no such placement"
    );
    assert_eq!(
        s.send(
            "POST",
            &securables(&ws),
            Some(&token(&ana())),
            "not json".into()
        )
        .await
        .status,
        400
    );

    // A table is made with its columns, not as a bare entry.
    assert_eq!(
        make(json!({"kind": "table", "parent_id": sc, "name": "bare"}))
            .await
            .status,
        400
    );
    let id = s.table(&ws, &ana(), &sc, "orders", "personal").await;
    let t = s
        .get(&format!("{}/{id}", securables(&ws)), &ana())
        .await
        .json();
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
            Some("user:ana")
        )
    );

    assert_eq!(items(&s.get(&securables(&ws), &ana()).await).len(), 1);
    assert_eq!(
        items(
            &s.get(&format!("{}?parent_id={sc}", securables(&ws)), &ana())
                .await
        )
        .len(),
        2
    );
    assert_eq!(
        s.get(&format!("{}/{v}", securables(&ws)), &ana())
            .await
            .json()["name"],
        "uploads"
    );
}
both!(catalog_rules);

// ---- who may do what ---------------------------------------------------------------------------------------------

async fn privileges(s: Server) {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob(), &cy(), &zed()]).await;
    let (c, sc, v) = tree(&s, &ws).await;
    s.grant(&ws, &ana(), &c, "user:bob", "reader").await;
    s.grant(&ws, &ana(), &sc, "user:cy", "contributor").await;
    assert_eq!(
        s.upload(&ws, &v, "a.txt", &ana(), b"hello").await.status,
        201
    );
    let at = |id: &str| format!("{}/{id}", securables(&ws));

    // Reader: can look and read; cannot write, create, or manage.
    assert_eq!(s.get(&at(&v), &bob()).await.status, 200);
    assert_eq!(s.download(&ws, &v, "a.txt", &bob()).await.bytes, b"hello");
    assert_eq!(s.upload(&ws, &v, "b.txt", &bob(), b"x").await.status, 403);
    assert_eq!(
        s.post(
            &securables(&ws),
            &bob(),
            json!({"kind": "volume", "parent_id": sc, "name": "mine"})
        )
        .await
        .status,
        403
    );
    let denied = s
        .put(
            &format!("{}/grants", at(&sc)),
            &bob(),
            json!({"principal": "user:bob", "role": "owner"}),
        )
        .await;
    assert_eq!(
        denied.status, 403,
        "a reader cannot make themselves an owner"
    );
    problem(&denied);
    assert_eq!(s.delete(&at(&v), &bob()).await.status, 403);

    // Contributor on the schema: can write and create inside it; cannot manage, and has nothing above it.
    assert_eq!(s.upload(&ws, &v, "b.txt", &cy(), b"x").await.status, 201);
    assert_eq!(
        s.post(
            &securables(&ws),
            &cy(),
            json!({"kind": "volume", "parent_id": sc, "name": "cys"})
        )
        .await
        .status,
        201
    );
    assert_eq!(s.delete(&at(&v), &cy()).await.status, 403);
    assert_eq!(
        s.get(&format!("{}/grants", at(&sc)), &cy()).await.status,
        403
    );
    assert_eq!(
        s.post(
            &securables(&ws),
            &cy(),
            json!({"kind": "schema", "parent_id": c, "name": "other"})
        )
        .await
        .status,
        403,
        "cy may use the catalog but not create in it"
    );

    // Assigned to the workspace but given nothing: it does not exist for them, whether they ask to read, write or drop.
    for r in [
        s.get(&at(&v), &zed()).await,
        s.download(&ws, &v, "a.txt", &zed()).await,
        s.upload(&ws, &v, "z.txt", &zed(), b"x").await,
        s.delete(&at(&v), &zed()).await,
        s.get(
            &format!("/api/v1/workspaces/{ws}/volumes/{v}/files"),
            &zed(),
        )
        .await,
        s.get(&at(&c), &zed()).await,
    ] {
        assert_eq!(r.status, 404);
    }
    assert_eq!(
        items(&s.get(&securables(&ws), &zed()).await).len(),
        0,
        "and they see nothing in lists"
    );

    // Owning a volume (granted on it alone) opens the way down to it, and only it.
    s.grant(&ws, &ana(), &v, "user:zed", "owner").await;
    assert_eq!(
        s.get(&format!("{}/grants", at(&v)), &zed()).await.status,
        200
    );
    assert_eq!(s.download(&ws, &v, "a.txt", &zed()).await.bytes, b"hello");
    assert_eq!(
        items(&s.get(&securables(&ws), &zed()).await).len(),
        1,
        "the catalog above it is shown so it can be reached"
    );
    assert_eq!(
        s.get(&format!("{}?parent_id={sc}", securables(&ws)), &zed())
            .await
            .json()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.delete(&format!("{}/grants/user:zed", at(&v)), &ana())
            .await
            .status,
        204
    );
    assert_eq!(
        s.get(&at(&v), &zed()).await.status,
        404,
        "taking the grant away takes the access away"
    );

    // The grants list is for people who manage the thing.
    let list = s.get(&format!("{}/grants", at(&c)), &ana()).await.json();
    assert!(list["items"].as_array().unwrap().iter().any(|i| {
        i["principal"] == "user:bob"
            && i["privileges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "SELECT")
    }));

    // Ownership can be handed over, and the new owner manages it.
    assert_eq!(
        s.put(
            &format!("{}/owner", at(&sc)),
            &cy(),
            json!({"owner": "user:cy"})
        )
        .await
        .status,
        403
    );
    assert_eq!(
        s.put(
            &format!("{}/owner", at(&sc)),
            &ana(),
            json!({"owner": "user:cy"})
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.get(&format!("{}/grants", at(&sc)), &cy()).await.status,
        200
    );

    // Refusals are in the audit trail, with who asked.
    let trail = items(
        &s.get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
            .await,
    );
    assert!(
        trail
            .iter()
            .any(|e| e["outcome"] == "denied" && e["actor"] == "bob"),
        "the reader's refused write is recorded"
    );
}
both!(privileges);

async fn groups_hold_privileges(s: Server) {
    let ws = s.acme().await;
    s.put(
        &format!("/api/v1/workspaces/{ws}/members"),
        &ana(),
        json!({"principal": "group:/finance/analysts", "role": "user"}),
    )
    .await;
    s.assign(&ws, &[&cy()]).await;
    let (c, _, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "a.txt", &ana(), b"hello").await;
    s.grant(&ws, &ana(), &c, "group:/finance/analysts", "reader")
        .await;
    let analyst = member("dee").in_group("/finance/analysts");
    assert_eq!(
        s.download(&ws, &v, "a.txt", &analyst).await.status,
        200,
        "a member of the group reads"
    );
    assert_eq!(
        s.download(&ws, &v, "a.txt", &cy()).await.status,
        404,
        "someone who is assigned but not in the group does not"
    );
    assert_eq!(
        s.download(&ws, &v, "a.txt", &member("dee")).await.status,
        404,
        "and leaving the group ends it at once"
    );
}
both!(groups_hold_privileges);

async fn catalog_bindings(s: Server) {
    let main = s.acme().await;
    let other = s.workspace(&ana(), ACME, "Other").await;
    s.assign(&main, &[&bob()]).await;
    s.assign(&other, &[&bob()]).await;
    let (c, _, v) = tree(&s, &main).await;
    s.grant(&main, &ana(), &c, "user:bob", "reader").await;
    s.upload(&main, &v, "a.txt", &ana(), b"x").await;

    // Unbound: visible from every workspace.
    assert_eq!(
        s.get(&format!("{}/{c}", securables(&other)), &bob())
            .await
            .status,
        200
    );
    // Bound to Main: gone from Other, as if it did not exist, and its files with it.
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/catalogs/{c}"),
            &bob(),
            json!({})
        )
        .await
        .status,
        403,
        "binding is for the account"
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/catalogs/{c}"),
            &ana(),
            json!({})
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.get(&format!("{}/{c}", securables(&other)), &bob())
            .await
            .status,
        404
    );
    assert_eq!(items(&s.get(&securables(&other), &bob()).await).len(), 0);
    assert_eq!(s.download(&other, &v, "a.txt", &bob()).await.status, 404);
    assert_eq!(
        s.get(&format!("{}/{c}", securables(&main)), &bob())
            .await
            .status,
        200
    );
    assert_eq!(s.download(&main, &v, "a.txt", &bob()).await.status, 200);
    // A catalog of another organization cannot be bound.
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{main}/catalogs/not-a-catalog"),
            &ana(),
            json!({})
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.delete(&format!("/api/v1/workspaces/{main}/catalogs/{c}"), &ana())
            .await
            .status,
        204
    );
    assert_eq!(
        s.get(&format!("{}/{c}", securables(&other)), &bob())
            .await
            .status,
        200,
        "unbound again, visible again"
    );
}
both!(catalog_bindings);

async fn organizations_are_separate(s: Server) {
    let ws = s.acme().await;
    let (c, _, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "a.txt", &ana(), b"secret").await;

    // Another organization, with its own admin, account and workspace.
    let rita = Who {
        orgs: vec![(RIVAL.into(), "rival".into())],
        ..person("rita")
    }
    .with_role("account-admin");
    assert_eq!(
        s.post(&format!("/api/v1/orgs/{RIVAL}/account"), &rita, json!({}))
            .await
            .status,
        201
    );
    let theirs = s.workspace(&rita, RIVAL, "Theirs").await;
    for r in [
        s.get(&format!("/api/v1/workspaces/{ws}"), &rita).await,
        s.get(&format!("{}/{c}", securables(&ws)), &rita).await,
        s.download(&ws, &v, "a.txt", &rita).await,
        s.get(&format!("/api/v1/orgs/{ACME}/audit"), &rita).await,
        s.get(&format!("/api/v1/orgs/{ACME}/workspaces"), &rita)
            .await,
    ] {
        assert_eq!(r.status, 404);
    }
    // Their own workspace, naming our ids, finds nothing.
    assert_eq!(
        s.get(&format!("{}/{c}", securables(&theirs)), &rita)
            .await
            .status,
        404
    );
    assert_eq!(s.download(&theirs, &v, "a.txt", &rita).await.status, 404);
    // A person of both organizations works in each separately.
    let both = ana().also_in(RIVAL, "rival");
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{theirs}"), &both)
            .await
            .status,
        404,
        "belonging to an organization is not being assigned to its workspaces"
    );
}
both!(organizations_are_separate);

async fn an_agent_acting_for_a_person(s: Server) {
    let ws = s.acme().await;
    // The agent is a service principal, assigned to the workspace like anyone.
    for who in [&bob(), &zed()] {
        s.assign(&ws, &[who]).await;
    }
    for agent in ["agent-1", "agent-2"] {
        s.put(
            &format!("/api/v1/workspaces/{ws}/members"),
            &ana(),
            json!({"principal": format!("sp:{agent}"), "role": "user"}),
        )
        .await;
    }
    let (c, _, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "a.txt", &ana(), b"hello").await;
    s.grant(&ws, &ana(), &c, "user:bob", "contributor").await;
    let for_bob = bob().acting_as("agent-1");

    // bob may read, the agent has been given nothing: the pair cannot.
    assert_eq!(s.download(&ws, &v, "a.txt", &bob()).await.status, 200);
    assert_eq!(s.download(&ws, &v, "a.txt", &for_bob).await.status, 404);

    // Given read, the agent can read for bob, but not write, though bob can.
    s.grant(&ws, &ana(), &c, "sp:agent-1", "reader").await;
    assert_eq!(s.download(&ws, &v, "a.txt", &for_bob).await.status, 200);
    assert_eq!(s.upload(&ws, &v, "b.txt", &for_bob, b"x").await.status, 403);
    assert_eq!(s.upload(&ws, &v, "b.txt", &bob(), b"x").await.status, 201);

    // It cannot borrow someone else's rights: for zed (who has nothing) it gets nothing, and an admin's agent is not an admin.
    assert_eq!(
        s.download(&ws, &v, "a.txt", &zed().acting_as("agent-1"))
            .await
            .status,
        404
    );
    assert_eq!(
        s.download(&ws, &v, "a.txt", &ana().acting_as("agent-2"))
            .await
            .status,
        404
    );
    // An agent that was never assigned to the workspace cannot even find it.
    assert_eq!(
        s.get(
            &format!("/api/v1/workspaces/{ws}"),
            &bob().acting_as("agent-9")
        )
        .await
        .status,
        404
    );

    // What an agent makes is its person's, and it keeps working with it.
    let made = s
        .post(
            &securables(&ws),
            &for_bob,
            json!({"kind": "schema", "parent_id": c, "name": "scratch"}),
        )
        .await;
    assert_eq!(
        made.status, 403,
        "agent-1 only reads, so it cannot create even for a contributor"
    );
    s.grant(&ws, &ana(), &c, "sp:agent-1", "contributor").await;
    let made = s
        .post(
            &securables(&ws),
            &for_bob,
            json!({"kind": "schema", "parent_id": c, "name": "scratch"}),
        )
        .await;
    assert_eq!(
        (made.status, made.json()["owner"].clone()),
        (201, json!("user:bob"))
    );

    // Both identities are in the audit trail.
    let trail = items(
        &s.get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
            .await,
    );
    assert!(
        trail
            .iter()
            .any(|e| e["actor"] == "agent-1" && e["on_behalf_of"] == "bob")
    );
}
both!(an_agent_acting_for_a_person);

/// The identity provider issues a person's token to the agents client only by exchange, with no `act` claim: that is the agent
/// acting for the person, and gets only what both may do.
async fn a_token_exchanged_for_the_agents_client_is_the_agent_acting_for_the_person(s: Server) {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{ws}/members"),
        &ana(),
        json!({"principal": "sp:loamz-agents", "role": "user"}),
    )
    .await;
    let (c, _, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "a.txt", &ana(), b"hello").await;
    s.grant(&ws, &ana(), &c, "user:bob", "contributor").await;
    let exchanged = bob().via_client("loamz-agents");

    // bob can read; the agents client has been given nothing, so the pair cannot.
    assert_eq!(s.download(&ws, &v, "a.txt", &bob()).await.status, 200);
    assert_eq!(s.download(&ws, &v, "a.txt", &exchanged).await.status, 404);
    // Given read, the pair can read, but not write, though bob can.
    s.grant(&ws, &ana(), &c, "sp:loamz-agents", "reader").await;
    assert_eq!(s.download(&ws, &v, "a.txt", &exchanged).await.status, 200);
    assert_eq!(
        s.upload(&ws, &v, "b.txt", &exchanged, b"x").await.status,
        403
    );
    // A client that is not an agent client is just where bob signed in.
    assert_eq!(
        s.download(&ws, &v, "a.txt", &bob().via_client("some-web-app"))
            .await
            .status,
        200
    );
}
both!(a_token_exchanged_for_the_agents_client_is_the_agent_acting_for_the_person);

// ---- files -------------------------------------------------------------------------------------------------------

async fn files(s: Server) {
    let ws = s.acme().await;
    let (_, _, v) = tree(&s, &ws).await;
    assert_eq!(
        s.upload(&ws, &v, "a/b/c.txt", &ana(), b"deep").await.status,
        201
    );
    let got = s.download(&ws, &v, "a/b/c.txt", &ana()).await;
    assert_eq!(
        (got.status, got.bytes.as_slice()),
        (200, b"deep".as_slice())
    );
    let list = s
        .get(
            &format!("/api/v1/workspaces/{ws}/volumes/{v}/files"),
            &ana(),
        )
        .await
        .json();
    assert_eq!(list["items"][0]["path"], "a/b/c.txt");
    assert_eq!(
        s.upload(&ws, &v, "a/b/c.txt", &ana(), b"replaced")
            .await
            .status,
        201
    );
    assert_eq!(
        s.download(&ws, &v, "a/b/c.txt", &ana()).await.bytes,
        b"replaced"
    );

    // A path never leaves the volume.
    for evil in [
        "..%2Fx",
        "a%2F..%2F..%2Fx",
        "%2Fetc%2Fpasswd",
        "a%5Cb",
        ".upload-1",
    ] {
        let r = s.upload(&ws, &v, evil, &ana(), b"x").await;
        assert!(
            r.status == 400 || r.status == 404,
            "{evil} gave {}",
            r.status
        );
        assert!(!s.store_dir(&v).join("x").exists());
    }

    assert_eq!(
        s.delete(
            &format!("/api/v1/workspaces/{ws}/volumes/{v}/files/a/b/c.txt"),
            &ana()
        )
        .await
        .status,
        204
    );
    assert_eq!(s.download(&ws, &v, "a/b/c.txt", &ana()).await.status, 404);

    // Not a volume.
    let c = s.make(&ws, &ana(), "catalog", "", "other").await;
    assert_eq!(s.upload(&ws, &c, "x", &ana(), b"x").await.status, 404);
}
both!(files);

#[tokio::test]
async fn an_upload_over_the_limit_is_refused_and_leaves_nothing() {
    let s = server_with(fresh_db(false).await, 10).await;
    let ws = s.acme().await;
    let (_, _, v) = tree(&s, &ws).await;
    assert_eq!(
        s.upload(&ws, &v, "ok.bin", &ana(), &[1; 10]).await.status,
        201
    );
    let r = s.upload(&ws, &v, "big.bin", &ana(), &[1; 11]).await;
    assert_eq!(r.status, 413);
    problem(&r);
    let list = s
        .get(
            &format!("/api/v1/workspaces/{ws}/volumes/{v}/files"),
            &ana(),
        )
        .await
        .json();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn downloads_are_attachments_the_browser_may_not_sniff() {
    let s = server(fresh_db(false).await).await;
    let ws = s.acme().await;
    let (_, _, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "page.html", &ana(), b"<script>1</script>")
        .await;
    let r = s
        .send(
            "GET",
            &format!("/api/v1/workspaces/{ws}/volumes/{v}/files/page.html"),
            Some(&token(&ana())),
            axum::body::Body::empty(),
        )
        .await;
    assert_eq!(r.content_type, "application/octet-stream");
}

// ---- lifecycle ---------------------------------------------------------------------------------------------------

async fn drop_is_a_durable_task(s: Server) {
    let ws = s.acme().await;
    let (_, sc, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "a.txt", &ana(), b"bytes").await;
    assert!(s.store_dir(&v).exists());

    let r = s.delete(&format!("{}/{sc}", securables(&ws)), &ana()).await;
    assert_eq!(r.status, 202);
    let task = r.json()["task_id"].as_str().unwrap().to_owned();

    // Gone from view at once, before any bytes are removed.
    assert_eq!(
        s.get(&format!("{}/{sc}", securables(&ws)), &ana())
            .await
            .status,
        404
    );
    assert_eq!(s.download(&ws, &v, "a.txt", &ana()).await.status, 404);
    assert!(
        s.store_dir(&v).exists(),
        "the bytes are removed by the task, not by the request"
    );
    let task_path = format!("/api/v1/orgs/{ACME}/tasks/{task}");
    assert_eq!(s.get(&task_path, &ana()).await.json()["state"], "pending");
    assert_eq!(s.get(&task_path, &bob()).await.status, 403);

    assert_eq!(s.work().await, 1);
    assert!(!s.store_dir(&v).exists(), "the files are gone");
    assert_eq!(s.get(&task_path, &ana()).await.json()["state"], "done");
    let trail = items(
        &s.get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
            .await,
    );
    assert!(trail.iter().any(|e| e["action"] == "drop.done"));
}
both!(drop_is_a_durable_task);

async fn a_task_killed_half_way_is_taken_up_again(s: Server) {
    let ws = s.acme().await;
    let (_, sc, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "a.txt", &ana(), b"bytes").await;
    s.delete(&format!("{}/{sc}", securables(&ws)), &ana()).await;

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
    assert!(s.store_dir(&v).exists());

    // Half the work had been done before the crash: the files are already gone.
    std::fs::remove_dir_all(s.store_dir(&v)).unwrap();

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
    let ws = s.acme().await;
    let (_, sc, _) = tree(&s, &ws).await;
    s.delete(&format!("{}/{sc}", securables(&ws)), &ana()).await;
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
    let trail = items(
        &s.get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
            .await,
    );
    assert!(trail.iter().any(|e| e["action"] == "unknown.failed"));
}
both!(a_task_that_keeps_failing_is_given_up_on_and_says_so);

async fn erase_removes_a_person_and_keeps_the_audit(s: Server) {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob()]).await;
    let c = s.make(&ws, &ana(), "catalog", "", "shared").await;
    s.grant(&ws, &ana(), &c, "user:bob", "contributor").await;
    let bobs_schema = s.make(&ws, &bob(), "schema", &c, "bobs").await;
    let bobs_volume = s.make(&ws, &bob(), "volume", &bobs_schema, "diary").await;
    s.upload(&ws, &bobs_volume, "me.txt", &bob(), b"private")
        .await;
    let anas_schema = s.make(&ws, &ana(), "schema", &c, "anas").await;
    let anas_volume = s.make(&ws, &ana(), "volume", &anas_schema, "keep").await;
    s.upload(&ws, &anas_volume, "x.txt", &ana(), b"mine").await;
    let erase = format!("/api/v1/orgs/{ACME}/erase");

    // Only an account admin may erase someone.
    assert_eq!(
        s.post(&erase, &bob(), json!({"subject": "user:bob"}))
            .await
            .status,
        403
    );
    assert_eq!(
        s.post(&erase, &ana(), json!({"subject": "bob"}))
            .await
            .status,
        400,
        "the subject is a typed principal"
    );

    let r = s.post(&erase, &ana(), json!({"subject": "user:bob"})).await;
    assert_eq!(r.status, 202);
    assert_eq!(s.work().await, 1);

    assert!(!s.store_dir(&bobs_volume).exists(), "bob's bytes are gone");
    assert_eq!(
        s.get(&format!("{}/{bobs_volume}", securables(&ws)), &ana())
            .await
            .status,
        404
    );
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{ws}"), &bob())
            .await
            .status,
        404,
        "he is no longer assigned to the workspace"
    );
    let grants: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM grants WHERE principal = 'user:bob'")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(grants, 0);
    assert_eq!(
        s.download(&ws, &anas_volume, "x.txt", &ana()).await.bytes,
        b"mine",
        "what is not bob's is untouched"
    );

    // The audit trail still says what happened, and it still holds together.
    let trail = items(
        &s.get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
            .await,
    );
    let actions: Vec<&str> = trail
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"erase.requested") && actions.contains(&"erase.done"));
    assert!(
        actions.contains(&"file.write"),
        "bob's earlier actions are still recorded"
    );
    assert_eq!(
        s.get(&format!("/api/v1/orgs/{ACME}/audit/verify"), &ana())
            .await
            .json()["intact"],
        true
    );
}
both!(erase_removes_a_person_and_keeps_the_audit);

async fn the_last_admin_cannot_be_erased(s: Server) {
    s.acme().await;
    s.post(
        &format!("/api/v1/orgs/{ACME}/erase"),
        &ana(),
        json!({"subject": "user:ana"}),
    )
    .await;
    s.work().await;
    let (state, error): (String, String) = sqlx::query_as("SELECT state, last_error FROM tasks")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(state, "pending", "it is retried, not marked done");
    assert!(error.contains("only admin"), "{error}");
    assert_eq!(
        s.get(&format!("/api/v1/orgs/{ACME}/admins"), &ana())
            .await
            .json()["items"],
        json!(["user:ana"])
    );
}
both!(the_last_admin_cannot_be_erased);

// ---- audit -------------------------------------------------------------------------------------------------------

async fn audit_chains(s: Server) {
    let ws = s.acme().await;
    let (_, _, v) = tree(&s, &ws).await;
    s.upload(&ws, &v, "a.txt", &ana(), b"x").await;
    let rita = Who {
        orgs: vec![(RIVAL.into(), "rival".into())],
        ..person("rita")
    }
    .with_role("account-admin");
    s.post(&format!("/api/v1/orgs/{RIVAL}/account"), &rita, json!({}))
        .await;
    s.workspace(&rita, RIVAL, "Theirs").await;

    let verify = format!("/api/v1/orgs/{ACME}/audit/verify");
    let ok = s.get(&verify, &ana()).await.json();
    assert_eq!(ok["intact"], true);
    assert!(ok["entries"].as_i64().unwrap() >= 5);

    // Reading the trail is for account admins; members are told no, outsiders that there is nothing.
    assert_eq!(
        s.get(&format!("/api/v1/orgs/{ACME}/audit"), &bob())
            .await
            .status,
        403
    );
    assert_eq!(
        s.get(&format!("/api/v1/orgs/{ACME}/audit"), &person("pete"))
            .await
            .status,
        404
    );

    // Changing an old entry is found, in this organization's chain only.
    sqlx::query("UPDATE audit SET action = 'create.nothing' WHERE org_id = $1 AND seq = 2")
        .bind(ACME)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let broken = s.get(&verify, &ana()).await.json();
    assert_eq!(
        (broken["intact"].clone(), broken["first_broken"].clone()),
        (json!(false), json!(2))
    );
    assert_eq!(
        s.get(&format!("/api/v1/orgs/{RIVAL}/audit/verify"), &rita)
            .await
            .json()["intact"],
        true,
        "another organization's chain is its own"
    );
}
both!(audit_chains);

async fn a_change_that_fails_leaves_no_record(s: Server) {
    let ws = s.acme().await;
    let (_, sc, _) = tree(&s, &ws).await;
    let count = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap()
    };
    let before = count().await;
    // Same name twice: refused after the work began, so nothing of it may remain, in the data or in the trail.
    assert_eq!(
        s.post(
            &securables(&ws),
            &ana(),
            json!({"kind": "volume", "parent_id": sc, "name": "uploads"})
        )
        .await
        .status,
        409
    );
    assert_eq!(count().await, before);
    let volumes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM securables WHERE kind = 'volume'")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(volumes, 1);
}
both!(a_change_that_fails_leaves_no_record);

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
