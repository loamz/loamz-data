//! Attempts to cross organization, workspace, and token boundaries through the public router.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod common;

use axum::body::Body;
use common::{ACME, FakeDirectory, RIVAL, Server, Who, ana, fresh_db, person, server};
use serde_json::json;

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

fn rival() -> Who {
    Who {
        orgs: vec![(RIVAL.into(), "rival".into())],
        ..person("rita")
    }
    .with_role("account-admin")
}

async fn two_workspaces(s: &Server) -> (String, Who, String) {
    let acme = s.acme().await;
    let rita = rival();
    let rival_ws = create_rival_workspace(s, &rita).await;
    (acme, rita, rival_ws)
}

async fn create_rival_workspace(s: &Server, rita: &Who) -> String {
    let made = s
        .post(&format!("/api/v1/orgs/{RIVAL}/account"), rita, json!({}))
        .await;
    assert_eq!(made.status, 201, "creating rival: {}", made.text());
    s.workspace(rita, RIVAL, "Theirs").await
}

async fn tree(s: &Server, workspace: &str, who: &Who) -> (String, String) {
    let catalog = s.make(workspace, who, "catalog", "", "sales").await;
    let schema = s.make(workspace, who, "schema", &catalog, "raw").await;
    (catalog, schema)
}

async fn compute(s: &Server, workspace: &str, who: &Who, name: &str) -> (String, String) {
    let made = s
        .post(
            &format!("/api/v1/workspaces/{workspace}/compute/targets"),
            who,
            json!({"name": name, "max_memory_mb": 2048, "max_cpu_millis": 1000}),
        )
        .await;
    assert_eq!(made.status, 201, "creating compute: {}", made.text());
    let target = (
        made.json()["id"].as_str().unwrap().to_owned(),
        made.json()["secret"].as_str().unwrap().to_owned(),
    );
    loamz_data_server::compute::service::authenticate_agent(&s.app, &target.1)
        .await
        .unwrap();
    target
}

async fn credential(s: &Server, workspace: &str, who: &Who, name: &str) -> String {
    let made = s
        .post(
            &format!("/api/v1/workspaces/{workspace}/storage/credentials"),
            who,
            json!({
                "name": name,
                "endpoint": "http://127.0.0.1:9000",
                "provider": "rustfs",
                "secret": {"kind": "sts", "access_key_id": "fake-key", "secret_access_key": "fake-secret"}
            }),
        )
        .await;
    assert_eq!(made.status, 201, "creating credential: {}", made.text());
    made.json()["id"].as_str().unwrap().to_owned()
}

async fn location(
    s: &Server,
    workspace: &str,
    who: &Who,
    name: &str,
    credential: &str,
    prefix: &str,
) -> common::Reply {
    s.post(
        &format!("/api/v1/workspaces/{workspace}/storage/locations"),
        who,
        json!({
            "name": name,
            "credential_id": credential,
            "bucket": "loamz-data",
            "prefix": prefix
        }),
    )
    .await
}

fn directory_members(directory: &FakeDirectory) -> Vec<(String, String)> {
    directory.members.lock().unwrap().clone()
}

async fn audit_count(s: &Server, org: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE org_id = $1")
        .bind(org)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap()
}

async fn lease(s: &Server, secret: &str) -> common::Reply {
    s.send(
        "POST",
        "/api/v1/agent/lease?wait_s=0",
        Some(secret),
        Body::empty(),
    )
    .await
}

async fn foreign_catalog_and_run_ids_are_hidden(s: Server) {
    let (acme, rita, rival_ws) = two_workspaces(&s).await;
    let (catalog, schema) = tree(&s, &acme, &ana()).await;
    let output = s.make(&acme, &ana(), "volume", &schema, "output").await;
    let (target, _) = compute(&s, &acme, &ana(), "worker").await;
    let submitted = s
        .post(
            &format!("/api/v1/workspaces/{acme}/runs"),
            &ana(),
            json!({"script": "print(1)", "inputs": [], "output": output, "target": target}),
        )
        .await;
    assert_eq!(submitted.status, 202, "queueing run: {}", submitted.text());
    let run = submitted.json()["id"].as_str().unwrap().to_owned();

    let directory_before = directory_members(&s.directory);
    assert_eq!(
        s.get(
            &format!("/api/v1/workspaces/{rival_ws}/securables/{catalog}"),
            &rita
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.delete(
            &format!("/api/v1/workspaces/{rival_ws}/securables/{catalog}"),
            &rita
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{rival_ws}/securables/{catalog}/grants"),
            &rita,
            json!({"principal": "user:rita", "role": "reader"}),
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.get(&format!("/api/v1/workspaces/{rival_ws}/runs/{run}"), &rita)
            .await
            .status,
        404
    );
    assert_eq!(
        s.post(
            &format!("/api/v1/workspaces/{rival_ws}/runs/{run}/cancel"),
            &rita,
            json!({}),
        )
        .await
        .status,
        404
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM runs WHERE id = $1")
            .bind(&run)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        "queued",
        "a hidden run was not cancelled"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM grants WHERE org_id = $1 AND securable = $2"
        )
        .bind(RIVAL)
        .bind(&catalog)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap(),
        0,
        "a hidden securable received no grant"
    );
    assert_eq!(directory_members(&s.directory), directory_before);
}
both!(foreign_catalog_and_run_ids_are_hidden);

async fn foreign_compute_and_storage_ids_are_hidden(s: Server) {
    let (acme, rita, rival_ws) = two_workspaces(&s).await;
    let (target, _) = compute(&s, &acme, &ana(), "worker").await;
    let compute_path = format!("/api/v1/workspaces/{rival_ws}/compute/targets/{target}");
    let secret_hash: String =
        sqlx::query_scalar("SELECT secret_hash FROM compute_targets WHERE id = $1")
            .bind(&target)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(
        s.post(&format!("{compute_path}/secret"), &rita, json!({}))
            .await
            .status,
        404
    );
    assert_eq!(s.delete(&compute_path, &rita).await.status, 404);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT secret_hash FROM compute_targets WHERE id = $1")
            .bind(&target)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        secret_hash,
        "a hidden target's secret was not rotated"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM compute_targets WHERE id = $1")
            .bind(&target)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        1,
        "a foreign compute target remains"
    );

    let credential = credential(&s, &acme, &ana(), "rustfs").await;
    let made = location(&s, &acme, &ana(), "private", &credential, "private").await;
    assert_eq!(made.status, 201, "creating location: {}", made.text());
    let location_id = made.json()["id"].as_str().unwrap().to_owned();
    let foreign_credential = s
        .delete(
            &format!("/api/v1/workspaces/{rival_ws}/storage/credentials/{credential}"),
            &rita,
        )
        .await;
    assert_eq!(
        foreign_credential.status,
        404,
        "{}",
        foreign_credential.text()
    );
    assert_eq!(
        location(&s, &rival_ws, &rita, "foreign", &credential, "elsewhere")
            .await
            .status,
        404
    );
    assert_eq!(
        s.delete(
            &format!("/api/v1/workspaces/{rival_ws}/storage/locations/{location_id}"),
            &rita,
        )
        .await
        .status,
        404
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM storage_credentials WHERE id = $1")
            .bind(&credential)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        1,
        "the credential remains"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM storage_locations WHERE workspace_id = $1"
        )
        .bind(&rival_ws)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap(),
        0,
        "no foreign location was created"
    );
}
both!(foreign_compute_and_storage_ids_are_hidden);

async fn foreign_service_principal_ids_are_hidden(s: Server) {
    let (acme, rita, rival_ws) = two_workspaces(&s).await;
    let path = format!("/api/v1/workspaces/{acme}/service-principals");
    let made = s.post(&path, &ana(), json!({"name": "pipeline"})).await;
    assert_eq!(
        made.status,
        201,
        "creating service principal: {}",
        made.text()
    );
    let id = made.json()["id"].as_str().unwrap().to_owned();
    let token = made.json()["token"].as_str().unwrap().to_owned();

    assert_eq!(
        s.post(
            &format!("/api/v1/workspaces/{rival_ws}/service-principals/{id}/token"),
            &rita,
            json!({}),
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.delete(
            &format!("/api/v1/workspaces/{rival_ws}/service-principals/{id}"),
            &rita,
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.bearer("GET", "/api/v1/me", &token, None).await.status,
        200,
        "foreign attempts did not revoke the token"
    );
}
both!(foreign_service_principal_ids_are_hidden);

async fn service_principal_tokens_are_confined_to_one_workspace(s: Server) {
    let acme = s.acme().await;
    let other = s.workspace(&ana(), ACME, "Elsewhere").await;
    let made = s
        .post(
            &format!("/api/v1/workspaces/{acme}/service-principals"),
            &ana(),
            json!({"name": "pipeline"}),
        )
        .await;
    assert_eq!(made.status, 201, "{}", made.text());
    let response = made.json();
    let token = response["token"].as_str().unwrap().to_owned();
    assert_eq!(
        s.bearer("GET", &format!("/api/v1/workspaces/{acme}"), &token, None)
            .await
            .status,
        200
    );
    assert_eq!(
        s.bearer("GET", &format!("/api/v1/workspaces/{other}"), &token, None)
            .await
            .status,
        404
    );
    let rita = rival();
    let rival_ws = create_rival_workspace(&s, &rita).await;
    assert_eq!(
        s.bearer(
            "GET",
            &format!("/api/v1/workspaces/{rival_ws}"),
            &token,
            None
        )
        .await
        .status,
        404
    );
}
both!(service_principal_tokens_are_confined_to_one_workspace);

async fn run_tokens_only_read_inputs_and_write_the_output(s: Server) {
    let workspace = s.acme().await;
    let (_, schema) = tree(&s, &workspace, &ana()).await;
    let input = s.make(&workspace, &ana(), "volume", &schema, "input").await;
    let output = s
        .make(&workspace, &ana(), "volume", &schema, "output")
        .await;
    let other = s.make(&workspace, &ana(), "volume", &schema, "other").await;
    assert_eq!(
        s.upload(&workspace, &input, "data.txt", &ana(), b"input")
            .await
            .status,
        201
    );
    assert_eq!(
        s.upload(&workspace, &other, "data.txt", &ana(), b"other")
            .await
            .status,
        201
    );
    assert_eq!(
        s.upload(
            &workspace,
            &output,
            "runs/another-run/data.txt",
            &ana(),
            b"preserve"
        )
        .await
        .status,
        201
    );
    let (target, secret) = compute(&s, &workspace, &ana(), "worker").await;
    let queued = s
        .post(
            &format!("/api/v1/workspaces/{workspace}/runs"),
            &ana(),
            json!({"script": "print(1)", "inputs": [input], "output": output, "target": target}),
        )
        .await;
    assert_eq!(queued.status, 202, "{}", queued.text());
    let leased = lease(&s, &secret).await;
    assert_eq!(leased.status, 200, "{}", leased.text());
    let run_token = leased.json()["token"].as_str().unwrap().to_owned();
    let run_id = leased.json()["run"]["id"].as_str().unwrap().to_owned();
    let own_output = format!("runs/{run_id}/data.txt");

    let read = format!("/api/v1/workspaces/{workspace}/volumes/{input}/files/data.txt");
    assert_eq!(s.bearer("GET", &read, &run_token, None).await.status, 200);
    let write = |id: &str, path: &str, bytes: &'static str| {
        let (s, run_token) = (&s, run_token.clone());
        let path = format!("/api/v1/workspaces/{workspace}/volumes/{id}/files/{path}");
        async move {
            s.send("PUT", &path, Some(&run_token), Body::from(bytes))
                .await
        }
    };
    assert_eq!(
        write(&output, &own_output, "changed").await.status,
        201,
        "the declared output is writable"
    );
    let rita = rival();
    let rival_ws = create_rival_workspace(&s, &rita).await;

    let before = audit_count(&s, ACME).await;
    assert_eq!(
        s.bearer(
            "GET",
            &format!("/api/v1/workspaces/{workspace}/volumes/{other}/files/data.txt"),
            &run_token,
            None
        )
        .await
        .status,
        404
    );
    assert_eq!(
        write(&output, "runs/another-run/data.txt", "overwrite")
            .await
            .status,
        404
    );
    assert_eq!(write(&output, "root.txt", "outside").await.status, 404);
    assert_eq!(write(&input, "data.txt", "changed").await.status, 404);
    assert_eq!(write(&other, "data.txt", "changed").await.status, 404);
    assert_eq!(
        s.send(
            "DELETE",
            &format!("/api/v1/workspaces/{workspace}/volumes/{input}/files/data.txt"),
            Some(&run_token),
            Body::empty(),
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.send(
            "DELETE",
            &format!(
                "/api/v1/workspaces/{workspace}/volumes/{output}/files/runs/another-run/data.txt"
            ),
            Some(&run_token),
            Body::empty(),
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.bearer(
            "GET",
            &format!("/api/v1/workspaces/{workspace}/volumes/{output}/files/{own_output}"),
            &run_token,
            None
        )
        .await
        .status,
        404,
        "write access to output does not imply read access"
    );
    assert_eq!(
        s.bearer(
            "GET",
            &format!("/api/v1/workspaces/{workspace}"),
            &run_token,
            None
        )
        .await
        .status,
        401,
        "run tokens are accepted only on file routes"
    );
    assert_eq!(
        s.send(
            "PUT",
            &format!(
                "/api/v1/workspaces/{rival_ws}/volumes/{output}/files/runs/{run_id}/foreign.txt"
            ),
            Some(&run_token),
            Body::from("foreign"),
        )
        .await
        .status,
        404
    );
    assert!(audit_count(&s, ACME).await >= before + 8);
    assert_eq!(
        s.download(&workspace, &input, "data.txt", &ana())
            .await
            .bytes,
        b"input",
        "denied writes/deletes left the input untouched"
    );
    assert_eq!(
        s.download(&workspace, &other, "data.txt", &ana())
            .await
            .bytes,
        b"other"
    );
    assert_eq!(
        s.download(&workspace, &output, "runs/another-run/data.txt", &ana())
            .await
            .bytes,
        b"preserve",
        "a run token cannot overwrite another run's output"
    );
    assert_eq!(
        s.download(&workspace, &output, &own_output, &ana())
            .await
            .bytes,
        b"changed"
    );
}
both!(run_tokens_only_read_inputs_and_write_the_output);

async fn expired_and_finished_run_tokens_are_rejected(s: Server) {
    let workspace = s.acme().await;
    let (_, schema) = tree(&s, &workspace, &ana()).await;
    let output = s
        .make(&workspace, &ana(), "volume", &schema, "output")
        .await;
    let (target, secret) = compute(&s, &workspace, &ana(), "worker").await;
    let submit = || {
        let (s, workspace, output, target) = (&s, &workspace, &output, &target);
        async move {
            s.post(
                &format!("/api/v1/workspaces/{workspace}/runs"),
                &ana(),
                json!({"script": "print(1)", "inputs": [], "output": output, "target": target}),
            )
            .await
        }
    };

    let first = submit().await;
    assert_eq!(first.status, 202, "{}", first.text());
    let first_id = first.json()["id"].as_str().unwrap().to_owned();
    let first_lease = lease(&s, &secret).await;
    assert_eq!(first_lease.status, 200, "{}", first_lease.text());
    let expired = first_lease.json()["token"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE run_tokens SET expires_ms = $1 WHERE run_id = $2")
        .bind(loamz_data_server::kernel::now_ms() - 1)
        .bind(&first_id)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let expired_file = format!("/api/v1/workspaces/{workspace}/volumes/{output}/files/expired.txt");
    assert_eq!(
        s.send("PUT", &expired_file, Some(&expired), Body::from("no"))
            .await
            .status,
        401
    );
    sqlx::query("UPDATE run_tokens SET expires_ms = $1 WHERE run_id = $2")
        .bind(loamz_data_server::kernel::now_ms() + 60_000)
        .bind(&first_id)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let completed = s
        .send(
            "POST",
            &format!("/api/v1/agent/runs/{first_id}/complete"),
            Some(&secret),
            Body::from(json!({"state": "succeeded"}).to_string()),
        )
        .await;
    assert_eq!(completed.status, 204, "{}", completed.text());
    assert_eq!(
        s.send("PUT", &expired_file, Some(&expired), Body::from("after"))
            .await
            .status,
        401
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM runs WHERE id = $1")
            .bind(first_id)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        "succeeded"
    );
    assert_eq!(
        s.download(&workspace, &output, "expired.txt", &ana())
            .await
            .status,
        404
    );
}
both!(expired_and_finished_run_tokens_are_rejected);

async fn volume_paths_cannot_escape_through_dot_segments_or_encoded_slashes(s: Server) {
    let workspace = s.acme().await;
    let (_, schema) = tree(&s, &workspace, &ana()).await;
    let volume = s.make(&workspace, &ana(), "volume", &schema, "files").await;
    assert_eq!(
        s.upload(&workspace, &volume, "inside.txt", &ana(), b"inside")
            .await
            .status,
        201
    );
    let folder = s.store_dir(&volume);
    let sentinel = folder.parent().unwrap().join("outside.txt");
    std::fs::write(&sentinel, b"sentinel").unwrap();

    assert_eq!(
        s.upload(&workspace, &volume, "../outside.txt", &ana(), b"escaped")
            .await
            .status,
        404
    );
    let encoded =
        format!("/api/v1/workspaces/{workspace}/volumes/{volume}/files/child%2F..%2Foutside.txt");
    assert_eq!(
        s.send(
            "PUT",
            &encoded,
            Some(&common::token(&ana())),
            Body::from("escaped")
        )
        .await
        .status,
        404
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"sentinel");
    assert!(
        !folder.join("outside.txt").exists(),
        "no path variant created a file in the volume"
    );
    std::fs::remove_file(sentinel).unwrap();
}
both!(volume_paths_cannot_escape_through_dot_segments_or_encoded_slashes);

async fn a_catalog_policy_hides_a_named_disallowed_target(s: Server) {
    let workspace = s.acme().await;
    let (catalog, schema) = tree(&s, &workspace, &ana()).await;
    s.table(&workspace, &ana(), &schema, "orders", "normal")
        .await;
    let (allowed, _) = compute(&s, &workspace, &ana(), "allowed").await;
    let (blocked, _) = compute(&s, &workspace, &ana(), "blocked").await;
    let policy = format!("/api/v1/workspaces/{workspace}/catalogs/{catalog}/compute");
    assert_eq!(
        s.put(&policy, &ana(), json!({"compute": [allowed]}))
            .await
            .status,
        204
    );
    sqlx::query("UPDATE compute_targets SET last_seen_ms = 0 WHERE id = $1")
        .bind(&blocked)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let runs_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    let refused = s
        .post(
            &format!("/api/v1/workspaces/{workspace}/sql/statements"),
            &ana(),
            json!({
                "sql": "SELECT * FROM sales.raw.orders",
                "wait_s": 0,
                "target": blocked
            }),
        )
        .await;
    assert_eq!(refused.status, 404, "{}", refused.text());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM runs")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        runs_before,
        "the refused target did not queue a run"
    );
}
both!(a_catalog_policy_hides_a_named_disallowed_target);

async fn a_queued_run_is_rechecked_against_the_catalog_policy(s: Server) {
    let workspace = s.acme().await;
    let (catalog, schema) = tree(&s, &workspace, &ana()).await;
    s.table(&workspace, &ana(), &schema, "orders", "normal")
        .await;
    let (first, secret) = compute(&s, &workspace, &ana(), "first").await;
    let (second, _) = compute(&s, &workspace, &ana(), "second").await;
    let policy = format!("/api/v1/workspaces/{workspace}/catalogs/{catalog}/compute");
    assert_eq!(
        s.put(&policy, &ana(), json!({"compute": [first.clone()]}))
            .await
            .status,
        204
    );
    let queued = s
        .post(
            &format!("/api/v1/workspaces/{workspace}/sql/statements"),
            &ana(),
            json!({
                "sql": "SELECT * FROM sales.raw.orders",
                "wait_s": 0,
                "target": first.clone()
            }),
        )
        .await;
    assert_eq!(queued.status, 202, "{}", queued.text());
    let run_id = queued.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        s.put(&policy, &ana(), json!({"compute": [second]}))
            .await
            .status,
        204
    );
    let no_work = lease(&s, &secret).await;
    assert_eq!(no_work.status, 204, "a disallowed target receives no run");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        "failed"
    );
}
both!(a_queued_run_is_rechecked_against_the_catalog_policy);

async fn overlapping_locations_are_hidden_and_not_created(s: Server) {
    let (acme, rita, rival_ws) = two_workspaces(&s).await;
    let acme_credential = credential(&s, &acme, &ana(), "acme-store").await;
    let rival_credential = credential(&s, &rival_ws, &rita, "rival-store").await;
    let first = location(
        &s,
        &acme,
        &ana(),
        "private",
        &acme_credential,
        "shared/data",
    )
    .await;
    assert_eq!(first.status, 201, "{}", first.text());
    let overlap = location(&s, &rival_ws, &rita, "overlap", &rival_credential, "shared").await;
    assert_eq!(overlap.status, 404, "{}", overlap.text());
    let sibling = location(
        &s,
        &rival_ws,
        &rita,
        "sibling",
        &rival_credential,
        "shared2",
    )
    .await;
    assert_eq!(sibling.status, 201, "{}", sibling.text());
    let listed = s
        .get(
            &format!("/api/v1/workspaces/{rival_ws}/storage/locations"),
            &rita,
        )
        .await;
    let response = listed.json();
    let items = response["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "sibling");
}
both!(overlapping_locations_are_hidden_and_not_created);
