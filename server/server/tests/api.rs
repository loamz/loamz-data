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
use std::sync::atomic::Ordering;

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

async fn customer_storage_secrets_are_write_only_and_rotation_is_fenced(s: Server) {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob()]).await;
    let base = format!("/api/v1/workspaces/{ws}/storage/credentials");
    let create = json!({"name":"customer", "provider":"rustfs", "endpoint":"http://127.0.0.1:9000", "region":"us-east-1", "secret":{"kind":"sts", "access_key_id":"CANARY_KEY_123", "secret_access_key":"CANARY_SECRET_123"}});
    let mut unverified = create.clone();
    unverified["name"] = json!("unverified");
    unverified["provider"] = json!("minio");
    let unsupported = s.post(&base, &ana(), unverified).await;
    assert_eq!(unsupported.status, 422);
    assert!(!unsupported.text().contains("CANARY_"));
    let refused = s.post(&base, &bob(), create.clone()).await;
    assert_eq!(refused.status, 404);
    assert_eq!(refused.cache_control, "no-store");
    assert!(!refused.text().contains("CANARY_"));
    assert_eq!(s.post(&base, &bob(), json!({})).await.status, 404);
    let made = s.post(&base, &ana(), create).await;
    assert_eq!(made.status, 201, "{}", made.text());
    assert_eq!(made.cache_control, "no-store");
    assert!(!made.text().contains("CANARY_"));
    assert!(!made.text().contains("secret_ref"));
    let credential = made.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(made.json()["generation"], 1);
    let service = s
        .post(
            &format!("/api/v1/workspaces/{ws}/service-principals"),
            &ana(),
            json!({"name":"storage-test-service"}),
        )
        .await;
    assert_eq!(service.status, 201);
    let principal = service.json()["principal"].as_str().unwrap().to_owned();
    let service_token = service.json()["token"].as_str().unwrap().to_owned();
    let assigned = s
        .put(
            &format!("/api/v1/workspaces/{ws}/members"),
            &ana(),
            json!({"principal":principal,"role":"admin"}),
        )
        .await;
    assert_eq!(assigned.status, 204);
    let denied_service = s.bearer("POST", &base, &service_token, Some(json!({"name":"service", "provider":"rustfs", "endpoint":"http://127.0.0.1:9000", "secret":{"kind":"sts", "access_key_id":"CANARY_SVC_KEY", "secret_access_key":"CANARY_SVC_SECRET"}}))).await;
    assert_eq!(denied_service.status, 404);
    assert!(!denied_service.text().contains("CANARY_"));
    assert_eq!(
        s.bearer(
            "PUT",
            &format!("{base}/{credential}/secret"),
            &service_token,
            Some(json!({}))
        )
        .await
        .status,
        404
    );

    let listed = s.get(&base, &ana()).await;
    assert!(!listed.text().contains("CANARY_"));
    assert!(!listed.text().contains("secret_ref"));
    assert_eq!(
        s.post(&format!("{base}/{credential}/check"), &ana(), json!({}))
            .await
            .status,
        400
    );
    let no_location = s
        .post(
            &format!("{base}/{credential}/check"),
            &ana(),
            json!({"location_id":"missing"}),
        )
        .await;
    assert_eq!(no_location.status, 404);
    assert_eq!(
        s.post(&format!("{base}/{credential}/check"), &bob(), json!({}))
            .await
            .status,
        404
    );
    let denied_rotation = s
        .send_with_header(
            "PUT",
            &format!("{base}/{credential}/secret"),
            Some(&token(&bob())),
            axum::body::Body::from("{}"),
            Some(("if-match", "\"1\"")),
        )
        .await;
    assert_eq!(denied_rotation.status, 404);
    let path = format!("{base}/{credential}/secret");
    let payload = json!({"kind":"sts", "access_key_id":"CANARY_KEY_456", "secret_access_key":"CANARY_SECRET_456"});
    let rotate = s
        .send_with_header(
            "PUT",
            &path,
            Some(&token(&ana())),
            axum::body::Body::from(payload.to_string()),
            Some(("if-match", "\"1\"")),
        )
        .await;
    assert_eq!(rotate.status, 204, "{}", rotate.text());
    assert!(!rotate.text().contains("CANARY_"));
    let stale = s
        .send_with_header(
            "PUT",
            &path,
            Some(&token(&ana())),
            axum::body::Body::from(payload.to_string()),
            Some(("if-match", "\"1\"")),
        )
        .await;
    assert_eq!(stale.status, 412);
    assert!(!stale.text().contains("CANARY_"));
    let again = s.get(&format!("{base}/{credential}"), &ana()).await;
    assert_eq!(again.json()["generation"], 2);
    assert_eq!(again.etag, "\"2\"");
    assert!(!again.text().contains("CANARY_"));
    let (vault_path, pinned_version, generation): (String, i64, i64) = sqlx::query_as(
        "SELECT secret_path, secret_version, generation FROM storage_credentials WHERE id = $1",
    )
    .bind(&credential)
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    assert!(vault_path.starts_with("customer-storage/"));
    assert_eq!((pinned_version, generation), (2, 2));
    let vault = s.app.vault.as_ref().unwrap();
    assert!(vault.read_version(&vault_path, 1).await.is_err());
    assert_eq!(
        vault.read_version(&vault_path, 2).await.unwrap()["secret_access_key"],
        "CANARY_SECRET_456"
    );
    let auth = token(&ana());
    let a = s.send_with_header(
        "PUT",
        &path,
        Some(&auth),
        axum::body::Body::from(payload.to_string()),
        Some(("if-match", "\"2\"")),
    );
    let b = s.send_with_header(
        "PUT",
        &path,
        Some(&auth),
        axum::body::Body::from(payload.to_string()),
        Some(("if-match", "\"2\"")),
    );
    let (a, b) = tokio::join!(a, b);
    let mut statuses = [a.status, b.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [204, 412]);
    assert!(vault.read_version(&vault_path, 2).await.is_err());
    assert_eq!(
        s.get(&format!("{base}/{credential}"), &ana()).await.json()["generation"],
        3
    );
    s.vault.fail_write.store(true, Ordering::SeqCst);
    let failed = s
        .send_with_header(
            "PUT",
            &path,
            Some(&auth),
            axum::body::Body::from(payload.to_string()),
            Some(("if-match", "\"3\"")),
        )
        .await;
    assert_eq!(failed.status, 503);
    assert!(!failed.text().contains("CANARY"));
    assert_eq!(
        s.get(&format!("{base}/{credential}"), &ana()).await.json()["generation"],
        3
    );
    s.vault.fail_write.store(false, Ordering::SeqCst);
    s.vault.fail_destroy.store(true, Ordering::SeqCst);
    let cleanup_failed = s
        .send_with_header(
            "PUT",
            &path,
            Some(&auth),
            axum::body::Body::from(payload.to_string()),
            Some(("if-match", "\"3\"")),
        )
        .await;
    assert_eq!(cleanup_failed.status, 503);
    assert!(!cleanup_failed.text().contains("CANARY"));
    assert_eq!(
        s.get(&format!("{base}/{credential}"), &ana()).await.json()["generation"],
        4
    );
    s.vault.fail_destroy.store(false, Ordering::SeqCst);
    let repaired = s
        .send_with_header(
            "PUT",
            &path,
            Some(&auth),
            axum::body::Body::from(payload.to_string()),
            Some(("if-match", "\"4\"")),
        )
        .await;
    assert_eq!(repaired.status, 204);
    assert!(vault.read_version(&vault_path, 3).await.is_err());
    assert!(vault.read_version(&vault_path, 4).await.is_err());
    let audits: Vec<String> = sqlx::query_scalar("SELECT target FROM audit WHERE org_id = $1")
        .bind(ACME)
        .fetch_all(s.app.db.pool())
        .await
        .unwrap();
    assert!(audits.iter().all(|line| !line.contains("CANARY_")));
    assert!(vault.read_version(&vault_path, 5).await.is_ok());
    assert_eq!(
        s.delete(&format!("{base}/{credential}"), &ana())
            .await
            .status,
        204
    );
    assert!(vault.read_version(&vault_path, 5).await.is_err());
}
both!(customer_storage_secrets_are_write_only_and_rotation_is_fenced);

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

    // An organization is set up the first time its admin (the role `account-admin`) signs in, with a workspace to start in. Nobody
    // asks for it, and nobody else sets it up.
    let org_of = |me: &Value| {
        me["orgs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["alias"] == "acme")
            .unwrap()
            .clone()
    };
    let me = s.get("/api/v1/me", &bob()).await.json();
    assert_eq!(
        org_of(&me)["has_account"],
        false,
        "a member who is not the admin does not set it up"
    );
    let me = s.get("/api/v1/me", &ana()).await.json();
    assert_eq!(org_of(&me)["has_account"], true);
    assert_eq!(org_of(&me)["account_admin"], true);
    assert!(
        me["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["org_alias"] == "acme" && w["name"] == "Main"),
        "and it starts with a workspace"
    );
    let again = s.get("/api/v1/me", &ana()).await.json();
    assert_eq!(
        again["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["org_alias"] == "acme")
            .count(),
        1,
        "once"
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

// ---- the installation's admin ------------------------------------------------------------------------------------

fn root() -> Who {
    person("root").with_role("platform-admin")
}

async fn the_platform_admin_makes_organizations(s: Server) {
    let body =
        json!({"alias": "globex", "admin_email": "Gia@Globex.test", "admin_name": "Gia Globex"});
    assert_eq!(
        s.post("/api/v1/orgs", &ana(), body.clone()).await.status,
        403,
        "an account admin is not the installation's admin"
    );
    assert_eq!(
        s.post("/api/v1/orgs", &root().acting_as("agent-1"), body.clone())
            .await
            .status,
        403,
        "an agent never creates an organization"
    );
    for bad in [
        json!({"alias": "Not Valid", "admin_email": "g@globex.test", "admin_name": "G"}),
        json!({"alias": "globex", "admin_email": "nobody", "admin_name": "G"}),
        json!({"alias": "globex", "admin_email": "g@globex.test", "admin_name": " "}),
    ] {
        assert_eq!(s.post("/api/v1/orgs", &root(), bad).await.status, 400);
    }
    assert!(
        s.directory.organizations.lock().unwrap().is_empty(),
        "nothing was made for a bad request"
    );

    let made = s.post("/api/v1/orgs", &root(), body.clone()).await;
    assert_eq!(made.status, 201, "{}", made.text());
    let made = made.json();
    assert_eq!(made["id"], "globex");
    assert_eq!(made["admin"]["email"], "gia@globex.test");
    assert!(
        made["admin"]["temporary_password"]
            .as_str()
            .is_some_and(|p| p.len() >= 20),
        "a new person is given a password to change"
    );
    assert_eq!(
        s.post("/api/v1/orgs", &root(), body).await.status,
        409,
        "only once"
    );

    // The first admin can sign in and manage the account.
    let subject = made["admin"]["subject"].as_str().unwrap();
    let gia = person(subject).also_in("globex", "globex");
    assert_eq!(
        s.get("/api/v1/orgs/globex/admins", &gia).await.json()["items"],
        json!([format!("user:{subject}")])
    );
    assert_eq!(
        s.directory.members.lock().unwrap().as_slice(),
        [("globex".to_owned(), subject.to_owned())]
    );
    assert_eq!(
        s.get("/api/v1/me", &root()).await.json()["platform_admin"],
        true
    );
    assert_eq!(
        s.get("/api/v1/me", &ana()).await.json()["platform_admin"],
        false
    );
}
both!(the_platform_admin_makes_organizations);

async fn admins_add_people(s: Server) {
    let path = format!("/api/v1/orgs/{ACME}/people");
    let dee = json!({"email": "Dee@Acme.test", "name": "Dee Acme"});
    assert_eq!(
        s.post(&format!("/api/v1/orgs/{ACME}/account"), &ana(), json!({}))
            .await
            .status,
        201
    );
    assert_eq!(
        s.post(&path, &bob(), dee.clone()).await.status,
        403,
        "a member who is not an admin cannot add people"
    );
    assert_eq!(
        s.post(&path, &person("pete"), dee.clone()).await.status,
        404,
        "an outsider cannot tell the account exists"
    );
    assert_eq!(
        s.post(&path, &ana(), json!({"email": "x", "name": "X"}))
            .await
            .status,
        400
    );

    let added = s.post(&path, &ana(), dee.clone()).await;
    assert_eq!(added.status, 201, "{}", added.text());
    assert_eq!(added.json()["email"], "dee@acme.test");
    assert!(added.json()["temporary_password"].is_string());
    let again = s.post(&path, &ana(), dee).await.json();
    assert!(
        again["temporary_password"].is_null(),
        "a person who exists keeps their own password"
    );
    assert_eq!(again["subject"], added.json()["subject"]);
    assert_eq!(s.directory.members.lock().unwrap().len(), 1, "added once");
}
both!(admins_add_people);

#[tokio::test]
async fn without_a_directory_people_cannot_be_made() {
    let s = common::server_without_directory(fresh_db(false).await).await;
    let body = json!({"alias": "globex", "admin_email": "g@globex.test", "admin_name": "G"});
    assert_eq!(s.post("/api/v1/orgs", &root(), body).await.status, 503);
}

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

async fn a_catalog_belongs_to_one_workspace(s: Server) {
    let main = s.acme().await;
    let other = s.workspace(&ana(), ACME, "Other").await;
    s.assign(&main, &[&bob()]).await;
    s.assign(&other, &[&bob()]).await;
    let (c, schema, v) = tree(&s, &main).await;
    s.grant(&main, &ana(), &c, "user:bob", "reader").await;
    s.upload(&main, &v, "a.txt", &ana(), b"x").await;

    // Its own workspace sees it, and its files.
    let inside = |ws: &str, id: &str| format!("{}/{id}", securables(ws));
    assert_eq!(s.get(&inside(&main, &c), &bob()).await.status, 200);
    assert_eq!(s.download(&main, &v, "a.txt", &bob()).await.status, 200);

    // Another workspace of the same organization does not have it: as if it did not exist, for everything inside it too, and even for
    // someone with a grant or the account admin.
    for who in [bob(), ana()] {
        for id in [&c, &schema, &v] {
            assert_eq!(s.get(&inside(&other, id), &who).await.status, 404);
        }
    }
    assert_eq!(items(&s.get(&securables(&other), &bob()).await).len(), 0);
    assert_eq!(s.download(&other, &v, "a.txt", &bob()).await.status, 404);

    // Catalogs are not bound or shared: there is no such route.
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{other}/catalogs/{c}"),
            &ana(),
            json!({})
        )
        .await
        .status,
        404
    );

    // A workspace that owns catalogs cannot be deleted; an empty one can.
    assert_eq!(
        s.delete(&format!("/api/v1/workspaces/{main}"), &ana())
            .await
            .status,
        409
    );
    assert_eq!(
        s.delete(&format!("/api/v1/workspaces/{other}"), &ana())
            .await
            .status,
        204
    );
}
both!(a_catalog_belongs_to_one_workspace);

#[tokio::test]
async fn metrics_are_counts_behind_a_scrape_token() {
    let s = server(fresh_db(false).await).await;
    s.get("/api/v1/me", &ana()).await;
    assert_eq!(
        s.send("GET", "/metrics", None, axum::body::Body::empty())
            .await
            .status,
        401
    );
    assert_eq!(s.bearer("GET", "/metrics", "wrong", None).await.status, 401);
    let ok = s.bearer("GET", "/metrics", &"m".repeat(40), None).await;
    assert_eq!(ok.status, 200);
    let text = ok.text();
    assert!(
        text.contains("loamz_http_requests_total{method=\"GET\",status=\"200\"}"),
        "{text}"
    );
    assert!(text.contains("# TYPE loamz_runs gauge"));
    assert!(!text.contains("ana"), "counts only: no names or ids");
}

async fn bytes_live_under_the_workspaces_prefix(s: Server) {
    let main = s.acme().await;
    let other = s.workspace(&ana(), ACME, "Other").await;
    let (c, schema, v) = tree(&s, &main).await;
    s.upload(&main, &v, "a.txt", &ana(), b"x").await;
    let folder = |id: String| {
        let s = &s;
        async move {
            sqlx::query_scalar::<_, String>("SELECT folder FROM securables WHERE id = $1")
                .bind(id)
                .fetch_one(s.app.db.pool())
                .await
                .unwrap()
        }
    };
    // A catalog's root is in its workspace's prefix; a schema shares it; a volume has a folder of its own under it.
    assert_eq!(folder(c.clone()).await, format!("{main}/{c}"));
    assert_eq!(folder(schema).await, format!("{main}/{c}"));
    assert_eq!(folder(v.clone()).await, format!("{main}/{c}/{v}"));
    assert!(
        s.store_dir(&v).join("a.txt").exists()
            && s.store_dir(&v).ends_with(format!("{main}/{c}/{v}")),
        "the bytes are in that folder"
    );
    // Another workspace's catalog is under its own prefix.
    let (c2, _, v2) = tree(&s, &other).await;
    assert_eq!(folder(v2.clone()).await, format!("{other}/{c2}/{v2}"));
    assert_ne!(s.store_dir(&v), s.store_dir(&v2));
}
both!(bytes_live_under_the_workspaces_prefix);

async fn service_principals_work_in_their_workspace(s: Server) {
    let main = s.acme().await;
    let other = s.workspace(&ana(), ACME, "Other").await;
    s.assign(&main, &[&bob()]).await;
    let (c, _, _) = tree(&s, &main).await;
    let path = format!("/api/v1/workspaces/{main}/service-principals");

    // A workspace's admins make them; a member cannot.
    assert_eq!(
        s.post(&path, &bob(), json!({"name": "bi"})).await.status,
        403
    );
    assert_eq!(s.get(&path, &bob()).await.status, 403);
    assert_eq!(
        s.post(&path, &ana(), json!({"name": "bad/name"}))
            .await
            .status,
        400
    );
    let made = s.post(&path, &ana(), json!({"name": "bi"})).await;
    assert_eq!(made.status, 201, "{}", made.text());
    let made = made.json();
    let (id, sp, token) = (
        made["id"].as_str().unwrap().to_owned(),
        made["principal"].as_str().unwrap().to_owned(),
        made["token"].as_str().unwrap().to_owned(),
    );
    assert!(token.starts_with("lsp_") && token.len() > 60);
    assert_eq!(sp, format!("sp:{id}"));
    assert_eq!(
        s.post(&path, &ana(), json!({"name": "bi"})).await.status,
        409
    );

    // The list never shows a token.
    let listed = s.get(&path, &ana()).await.json();
    assert_eq!(listed["items"][0]["principal"], sp);
    assert!(listed["items"][0].get("token").is_none());

    // It is a principal like any other: no grant, no sight; with a grant, it reads.
    let catalog = format!("{}/{c}", securables(&main));
    assert_eq!(s.bearer("GET", &catalog, &token, None).await.status, 404);
    s.grant(&main, &ana(), &c, &sp, "reader").await;
    assert_eq!(s.bearer("GET", &catalog, &token, None).await.status, 200);
    let me = s.bearer("GET", "/api/v1/me", &token, None).await;
    assert_eq!(me.status, 200, "{}", me.text());
    assert_eq!(me.json()["kind"], "service");

    // It belongs to one workspace: assigned to another, it still cannot open it.
    let put = s
        .put(
            &format!("/api/v1/workspaces/{other}/members"),
            &ana(),
            json!({"principal": sp, "role": "user"}),
        )
        .await;
    assert_eq!(put.status, 204);
    assert_eq!(
        s.bearer("GET", &securables(&other), &token, None)
            .await
            .status,
        404
    );
    // It does not make service principals, and a token that is not one of ours is no one.
    assert_eq!(
        s.bearer("POST", &path, &token, Some(json!({"name": "more"})))
            .await
            .status,
        403
    );
    assert_eq!(
        s.bearer("GET", &catalog, "lsp_not-a-token", None)
            .await
            .status,
        401
    );

    // A new token replaces the old at once.
    let renewed = s
        .post(&format!("{path}/{id}/token"), &ana(), json!({}))
        .await;
    assert_eq!(renewed.status, 200, "{}", renewed.text());
    let fresh = renewed.json()["token"].as_str().unwrap().to_owned();
    assert_eq!(s.bearer("GET", &catalog, &token, None).await.status, 401);
    assert_eq!(s.bearer("GET", &catalog, &fresh, None).await.status, 200);

    // Deleting it ends the token and the assignment.
    assert_eq!(s.delete(&format!("{path}/{id}"), &ana()).await.status, 204);
    assert_eq!(s.bearer("GET", &catalog, &fresh, None).await.status, 401);
    assert_eq!(s.delete(&format!("{path}/{id}"), &ana()).await.status, 404);
    assert!(items(&s.get(&path, &ana()).await).is_empty());
}
both!(service_principals_work_in_their_workspace);

async fn storage_is_set_per_workspace(s: Server) {
    let main = s.acme().await;
    let other = s.workspace(&ana(), ACME, "Other").await;
    s.assign(&main, &[&bob()]).await;
    let base = format!("/api/v1/workspaces/{main}/storage");
    let credential = |name: &str, key: &str| json!({"name": name, "provider": "rustfs", "endpoint": "http://127.0.0.1:9000", "secret": {"kind": "sts", "access_key_id": if key.contains("nope") { "" } else { key }, "secret_access_key": "fake-secret"}});

    // A workspace's admins set storage; a member cannot even look.
    assert_eq!(
        s.get(&format!("{base}/credentials"), &bob()).await.status,
        404
    );
    assert_eq!(
        s.post(
            &format!("{base}/credentials"),
            &bob(),
            credential("rustfs", "loamz-data/bucket#key_id")
        )
        .await
        .status,
        404
    );

    // The vault must have the secrets; the endpoint must be https or local; names are plain.
    let missing = s
        .post(
            &format!("{base}/credentials"),
            &ana(),
            credential("rustfs", "loamz-data/nope#key_id"),
        )
        .await;
    assert_eq!(missing.status, 400);
    assert!(!missing.text().contains("fake-secret"));
    let mut remote = credential("rustfs", "loamz-data/bucket#key_id");
    remote["endpoint"] = json!("http://example.test");
    assert_eq!(
        s.post(&format!("{base}/credentials"), &ana(), remote)
            .await
            .status,
        400
    );
    assert_eq!(
        s.post(
            &format!("{base}/credentials"),
            &ana(),
            credential("bad/name", "loamz-data/bucket#key_id")
        )
        .await
        .status,
        400
    );

    let made = s
        .post(
            &format!("{base}/credentials"),
            &ana(),
            credential("rustfs", "loamz-data/bucket#key_id"),
        )
        .await;
    assert_eq!(made.status, 201, "{}", made.text());
    let cred = made.json()["id"].as_str().unwrap().to_owned();
    assert!(made.json().get("secret_ref").is_none());
    assert_eq!(
        s.post(
            &format!("{base}/credentials"),
            &ana(),
            credential("rustfs", "loamz-data/bucket#key_id")
        )
        .await
        .status,
        409
    );

    // Locations: a bucket or a prefix; none may overlap another, in any workspace of the installation.
    let location = |name: &str, credential: &str, prefix: &str| json!({"name": name, "credential_id": credential, "bucket": "loamz-data", "prefix": prefix});
    let team_a = s
        .post(
            &format!("{base}/locations"),
            &ana(),
            location("team-a", &cred, "/team-a/"),
        )
        .await;
    assert_eq!(team_a.status, 201, "{}", team_a.text());
    assert_eq!(team_a.json()["prefix"], "team-a");
    for (name, prefix) in [("inside", "team-a/raw"), ("whole", ""), ("same", "team-a")] {
        let r = s
            .post(
                &format!("{base}/locations"),
                &ana(),
                location(name, &cred, prefix),
            )
            .await;
        assert_eq!(r.status, 409, "{prefix:?}: {}", r.text());
    }
    assert_eq!(
        s.post(
            &format!("{base}/locations"),
            &ana(),
            location("team-ab", &cred, "team-ab")
        )
        .await
        .status,
        201,
        "a folder name is not a text prefix"
    );
    assert_eq!(
        s.post(
            &format!("{base}/locations"),
            &ana(),
            location("up", &cred, "../x")
        )
        .await
        .status,
        400
    );
    assert_eq!(
        s.post(
            &format!("{base}/locations"),
            &ana(),
            location("lost", "no-such", "x")
        )
        .await
        .status,
        404
    );

    // Another workspace using the same store and bucket cannot take the same path.
    let other_base = format!("/api/v1/workspaces/{other}/storage");
    let theirs = s
        .post(
            &format!("{other_base}/credentials"),
            &ana(),
            credential("rustfs", "loamz-data/bucket#key_id"),
        )
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        s.post(
            &format!("{other_base}/locations"),
            &ana(),
            location("t", &theirs, "team-a/x")
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.post(
            &format!("{other_base}/locations"),
            &ana(),
            location("t", &theirs, "team-b")
        )
        .await
        .status,
        201
    );
    assert_eq!(
        s.get(&format!("{base}/credentials"), &ana()).await.json()["items"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "each workspace has its own"
    );

    // A credential a location uses stays until the location goes.
    assert_eq!(
        s.delete(&format!("{base}/credentials/{cred}"), &ana())
            .await
            .status,
        409
    );
    for l in items(&s.get(&format!("{base}/locations"), &ana()).await) {
        assert_eq!(
            s.delete(
                &format!("{base}/locations/{}", l["id"].as_str().unwrap()),
                &ana()
            )
            .await
            .status,
            204
        );
    }
    assert_eq!(
        s.delete(&format!("{base}/credentials/{cred}"), &ana())
            .await
            .status,
        204
    );
    assert_eq!(
        s.delete(&format!("{base}/credentials/{cred}"), &ana())
            .await
            .status,
        404
    );
}
both!(storage_is_set_per_workspace);

async fn location_like_wildcards_do_not_match_other_catalog_prefixes(s: Server) {
    let ws = s.acme().await;
    let base = format!("/api/v1/workspaces/{ws}/storage");
    let cred = s
        .post(
            &format!("{base}/credentials"),
            &ana(),
            json!({"name": "rustfs", "endpoint": "http://127.0.0.1:9000", "provider":"rustfs", "secret":{"kind":"sts", "access_key_id":"fake-key", "secret_access_key":"fake-secret"}}),
        )
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let location = |name: &str, prefix: &str| json!({"name": name, "credential_id": cred, "bucket": "loamz-data", "prefix": prefix});
    let pattern = s
        .post(
            &format!("{base}/locations"),
            &ana(),
            location("team-under", "team_"),
        )
        .await;
    assert_eq!(pattern.status, 201, "{}", pattern.text());
    let pattern = pattern.json()["id"].as_str().unwrap().to_owned();
    let sibling = s
        .post(
            &format!("{base}/locations"),
            &ana(),
            location("team-x", "teamX"),
        )
        .await;
    assert_eq!(sibling.status, 201, "{}", sibling.text());
    let catalog = s
        .post(
            &securables(&ws),
            &ana(),
            json!({"kind": "catalog", "name": "sibling", "location": "team-x"}),
        )
        .await;
    assert_eq!(catalog.status, 201, "{}", catalog.text());

    assert_eq!(
        s.delete(&format!("{base}/locations/{pattern}"), &ana())
            .await
            .status,
        204,
        "the underscore in team_ is a literal, not a SQL LIKE wildcard"
    );
}
both!(location_like_wildcards_do_not_match_other_catalog_prefixes);

async fn a_catalog_keeps_its_data_where_its_location_says(s: Server) {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob()]).await;
    let base = format!("/api/v1/workspaces/{ws}/storage");
    let cred = s
        .post(
            &format!("{base}/credentials"),
            &ana(),
            json!({"name": "rustfs", "endpoint": "http://127.0.0.1:9000", "provider":"rustfs", "secret":{"kind":"sts", "access_key_id":"fake-key", "secret_access_key":"fake-secret"}}),
        )
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let made = s
        .post(
            &format!("{base}/locations"),
            &ana(),
            json!({"name": "lake", "credential_id": cred, "bucket": "loamz-data", "prefix": "acme/lake"}),
        )
        .await;
    assert_eq!(made.status, 201, "{}", made.text());
    let location = made.json()["id"].as_str().unwrap().to_owned();
    let folder = |id: String| {
        let s = &s;
        async move {
            sqlx::query_scalar::<_, String>("SELECT folder FROM securables WHERE id = $1")
                .bind(id)
                .fetch_one(s.app.db.pool())
                .await
                .unwrap()
        }
    };
    let url = format!("/api/v1/workspaces/{ws}/securables");

    // Without a location a catalog is in its workspace's default storage; with one, in the location's folder, and what is inside follows.
    let plain = s
        .post(&url, &ana(), json!({"kind": "catalog", "name": "plain"}))
        .await;
    let plain = plain.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(folder(plain.clone()).await, format!("{ws}/{plain}"));
    let on = s
        .post(
            &url,
            &ana(),
            json!({"kind": "catalog", "name": "sales", "location": "lake"}),
        )
        .await;
    assert_eq!(on.status, 201, "{}", on.text());
    let on = on.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(folder(on.clone()).await, format!("acme/lake/{on}"));
    let schema = s.make(&ws, &ana(), "schema", &on, "raw").await;
    let table = s.make(&ws, &ana(), "table", &schema, "orders").await;
    assert_eq!(folder(schema).await, format!("acme/lake/{on}"));
    assert!(folder(table).await.starts_with(&format!("acme/lake/{on}/")));

    // Only a catalog is put on a location, only the workspace's admins may, and only one that exists.
    let parent = json!({"kind": "schema", "parent_id": on, "name": "x", "location": "lake"});
    assert_eq!(s.post(&url, &ana(), parent).await.status, 400);
    assert_eq!(
        s.post(
            &url,
            &ana(),
            json!({"kind": "catalog", "name": "lost", "location": "nowhere"})
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.post(
            &url,
            &bob(),
            json!({"kind": "catalog", "name": "mine", "location": "lake"})
        )
        .await
        .status,
        404,
        "someone who may not even create a catalog is told there is nothing to create it in"
    );

    // A location that holds a catalog cannot be removed until the catalog is gone.
    let del = format!("{base}/locations/{location}");
    assert_eq!(s.delete(&del, &ana()).await.status, 409);
    assert_eq!(s.delete(&format!("{url}/{on}"), &ana()).await.status, 202);
    s.work().await;
    assert_eq!(s.delete(&del, &ana()).await.status, 204);
}
both!(a_catalog_keeps_its_data_where_its_location_says);

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
    assert_eq!(s.get(&task_path, &bob()).await.status, 404);

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
        404
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

async fn rustfs_live_storage_credential_check(s: Server) {
    let endpoint =
        std::env::var("LOAMZ_TEST_STS_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:9000".into());
    let ws = s.acme().await;
    let base = format!("/api/v1/workspaces/{ws}/storage");
    let made = s.post(&format!("{base}/credentials"), &ana(), json!({
        "name":"live-check", "provider":"rustfs", "endpoint":endpoint,
        "region":"us-east-1", "secret":{"kind":"sts", "access_key_id":"loamz-dev", "secret_access_key":"loamz-dev-only"}
    })).await;
    assert_eq!(made.status, 201);
    let credential = made.json()["id"].as_str().unwrap().to_owned();
    let location = s.post(&format!("{base}/locations"), &ana(), json!({
        "name":"live-location", "credential_id":credential, "bucket":"loamz-data", "prefix":format!("check{}", std::process::id())
    })).await;
    assert_eq!(location.status, 201);
    let location_id = location.json()["id"].as_str().unwrap().to_owned();
    let checked = s
        .post(
            &format!("{base}/credentials/{credential}/check"),
            &ana(),
            json!({"location_id":location_id}),
        )
        .await;
    assert_eq!(checked.status, 200);
    assert_eq!(checked.json()["status"], "usable");
    assert_eq!(checked.json()["generation"], 1);
    let rotated = s.send_with_header("PUT", &format!("{base}/credentials/{credential}/secret"), Some(&token(&ana())), axum::body::Body::from(json!({"kind":"sts", "access_key_id":"bad-test-key", "secret_access_key":"bad-test-secret"}).to_string()), Some(("if-match", "\"1\""))).await;
    assert_eq!(rotated.status, 204);
    let denied = s
        .post(
            &format!("{base}/credentials/{credential}/check"),
            &ana(),
            json!({"location_id":location_id}),
        )
        .await;
    assert_eq!(denied.status, 200);
    assert_eq!(denied.json()["status"], "denied");
    assert_eq!(denied.json()["generation"], 2);
}

#[tokio::test]
#[ignore = "needs the local RustFS development service"]
async fn rustfs_live_storage_credential_check_sqlite() {
    rustfs_live_storage_credential_check(server(fresh_db(false).await).await).await;
}

#[tokio::test]
#[ignore = "needs the local RustFS and Postgres development services"]
async fn rustfs_live_storage_credential_check_postgres() {
    rustfs_live_storage_credential_check(server(fresh_db(true).await).await).await;
}

async fn location_creation_and_credential_deletion_do_not_orphan_a_location(s: Server) {
    let ws = s.acme().await;
    let base = format!("/api/v1/workspaces/{ws}/storage");
    let credential = s.post(&format!("{base}/credentials"), &ana(), json!({
        "name":"race-credential", "provider":"rustfs", "endpoint":"http://127.0.0.1:9000",
        "secret":{"kind":"sts", "access_key_id":"race-key", "secret_access_key":"race-secret"}
    })).await;
    assert_eq!(credential.status, 201);
    let id = credential.json()["id"].as_str().unwrap().to_owned();
    let location_path = format!("{base}/locations");
    let credential_path = format!("{base}/credentials/{id}");
    let admin = ana();
    let create = s.post(&location_path, &admin, json!({"name":"race-location", "credential_id":id, "bucket":"race-bucket", "prefix":"safe"}));
    let delete = s.delete(&credential_path, &admin);
    let (created, deleted) = tokio::join!(create, delete);
    assert!(matches!(
        (created.status, deleted.status),
        (201, 409) | (404, 204)
    ));
    let orphans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM storage_locations l LEFT JOIN storage_credentials c ON c.id = l.credential_id WHERE c.id IS NULL")
        .fetch_one(s.app.db.pool()).await.unwrap();
    assert_eq!(orphans, 0);
}
both!(location_creation_and_credential_deletion_do_not_orphan_a_location);

async fn failed_vault_cleanup_after_deletion_is_reported_without_secrets(s: Server) {
    let ws = s.acme().await;
    let base = format!("/api/v1/workspaces/{ws}/storage/credentials");
    let made = s.post(&base, &ana(), json!({"name":"delete-failure", "provider":"rustfs", "endpoint":"http://127.0.0.1:9000", "secret":{"kind":"sts", "access_key_id":"CANARY_DELETE_KEY", "secret_access_key":"CANARY_DELETE_SECRET"}})).await;
    assert_eq!(made.status, 201);
    let id = made.json()["id"].as_str().unwrap().to_owned();
    s.vault.fail_destroy.store(true, Ordering::SeqCst);
    let deleted = s.delete(&format!("{base}/{id}"), &ana()).await;
    assert_eq!(deleted.status, 503);
    assert!(!deleted.text().contains("CANARY"));
    assert_eq!(s.get(&format!("{base}/{id}"), &ana()).await.status, 404);
    assert!(
        s.app
            .vault
            .as_ref()
            .unwrap()
            .read_version(&format!("customer-storage/{id}"), 1)
            .await
            .is_ok()
    );
}
both!(failed_vault_cleanup_after_deletion_is_reported_without_secrets);
