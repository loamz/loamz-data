//! Erasing a person without erasing the evidence that it happened.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod common;

use axum::body::Body;
use common::{ACME, RIVAL, Server, Who, ana, bob, fresh_db, person, server};
use loamz_data_server::kernel::{hash_secret, new_id};
use serde_json::json;

type AuditRow = (
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

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

async fn request_erase(s: &Server, who: &Who, subject: &str) -> String {
    let response = s
        .post(
            &format!("/api/v1/orgs/{ACME}/erase"),
            who,
            json!({"subject": subject}),
        )
        .await;
    assert_eq!(response.status, 202, "{}", response.text());
    response.json()["task_id"].as_str().unwrap().to_owned()
}

async fn seed_run(
    s: &Server,
    workspace: &str,
    requester: &str,
    behalf: &str,
    state: &str,
    target: &str,
    output: &str,
) -> String {
    let id = new_id();
    let spec = json!({
        "as": {
            "subject": "bob",
            "name": "Bob Example",
            "email": "bob@example.test",
            "groups": ["people"],
            "actor": null
        },
        "script": "print('private script')",
        "output": output
    })
    .to_string();
    sqlx::query(
        "INSERT INTO runs (id, org_id, workspace_id, kind, requested_by, on_behalf_of, target_id, state, title, spec, result, stdout, stderr, exit_code, error, cancel, lease_until, created_ms, started_ms, finished_ms) \
         VALUES ($1,$2,$3,'job',$4,$5,$6,$7,'Bob private run',$8,'private result','private stdout','private stderr',1,'private error',0,0,1,1,0)",
    )
    .bind(&id)
    .bind(ACME)
    .bind(workspace)
    .bind(requester)
    .bind(behalf)
    .bind(target)
    .bind(state)
    .bind(spec)
    .execute(s.app.db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO run_tokens (token_hash, run_id, org_id, workspace_id, identity, reads, writes, expires_ms) \
         VALUES ($1,$2,$3,$4,$5,'[]','[]',9999999999999)",
    )
    .bind(hash_secret(&format!("token-{id}")))
    .bind(&id)
    .bind(ACME)
    .bind(workspace)
    .bind(json!({"subject":"bob","name":"Bob Example","email":"bob@example.test"}).to_string())
    .execute(s.app.db.pool())
    .await
    .unwrap();
    id
}

async fn owned_catalog_files_and_other_content(s: Server) {
    let workspace = s.acme().await;
    let catalog = s
        .make(&workspace, &ana(), "catalog", "", "owned_by_bob")
        .await;
    let schema = s
        .make(&workspace, &ana(), "schema", &catalog, "private")
        .await;
    let volume = s.make(&workspace, &ana(), "volume", &schema, "diary").await;
    sqlx::query("UPDATE securables SET owner = 'user:bob' WHERE org_id = $1 AND id = $2")
        .bind(ACME)
        .bind(&catalog)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    s.upload(&workspace, &volume, "private.txt", &ana(), b"private")
        .await;
    let other_catalog = s
        .make(&workspace, &ana(), "catalog", "", "keep_catalog")
        .await;
    let other_schema = s
        .make(&workspace, &ana(), "schema", &other_catalog, "main")
        .await;
    let other = s
        .make(&workspace, &ana(), "volume", &other_schema, "keep")
        .await;
    s.upload(&workspace, &other, "keep.txt", &ana(), b"keep")
        .await;

    request_erase(&s, &ana(), "user:bob").await;
    assert_eq!(s.work().await, 1);
    assert!(!s.store_dir(&volume).exists());
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM securables WHERE org_id = $1 AND id IN ($2,$3,$4)",
    )
    .bind(ACME)
    .bind(&catalog)
    .bind(&schema)
    .bind(&volume)
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    assert_eq!(
        remaining, 0,
        "the owned catalog and its descendants are gone"
    );
    assert_eq!(
        s.download(&workspace, &other, "keep.txt", &ana())
            .await
            .bytes,
        b"keep"
    );
}
both!(owned_catalog_files_and_other_content);

async fn all_person_references_are_removed_or_tombstoned(s: Server) {
    let workspace = s.acme().await;
    s.assign(&workspace, &[&bob()]).await;
    let catalog = s.make(&workspace, &ana(), "catalog", "", "shared").await;
    let schema = s.make(&workspace, &ana(), "schema", &catalog, "main").await;
    let volume = s
        .make(&workspace, &ana(), "volume", &schema, "shared")
        .await;

    sqlx::query("UPDATE orgs SET created_by = 'bob' WHERE id = $1")
        .bind(ACME)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE workspaces SET created_by = 'bob' WHERE id = $1")
        .bind(&workspace)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO account_admins (org_id, principal, added_by, created_ms) VALUES ($1,'user:bob','bob',1)")
        .bind(ACME)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO entitlements (workspace_id, principal, entitlement) VALUES ($1,'user:bob','sql-access')")
        .bind(&workspace)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO grants (org_id, principal, securable, privilege, granted_by, created_ms) VALUES ($1,'user:cy',$2,'SELECT','bob',1)",
    )
    .bind(ACME)
    .bind(&catalog)
    .execute(s.app.db.pool())
    .await
    .unwrap();

    let target = new_id();
    sqlx::query(
        "INSERT INTO compute_targets (id,org_id,workspace_id,name,secret_hash,kind,endpoint,max_memory_mb,max_cpu_millis,created_by,created_ms,last_seen_ms) \
         VALUES ($1,$2,$3,'worker',$4,'agent','',1,1,'bob',1,0)",
    )
    .bind(&target)
    .bind(ACME)
    .bind(&workspace)
    .bind(hash_secret("compute-secret"))
    .execute(s.app.db.pool())
    .await
    .unwrap();

    let principal = new_id();
    sqlx::query(
        "INSERT INTO service_principals (id,org_id,workspace_id,name,secret_hash,created_by,created_ms) \
         VALUES ($1,$2,$3,'service',$4,'bob',1)",
    )
    .bind(&principal)
    .bind(ACME)
    .bind(&workspace)
    .bind(hash_secret("service-secret"))
    .execute(s.app.db.pool())
    .await
    .unwrap();
    let credential = new_id();
    sqlx::query(
        "INSERT INTO storage_credentials (id,org_id,workspace_id,name,endpoint,region,key_id_ref,secret_ref,vending,role_arn,issuer_token_ref,created_by,created_ms) \
         VALUES ($1,$2,$3,'credential','https://s3.test','auto','vault#key','vault#secret','none','','','bob',1)",
    )
    .bind(&credential)
    .bind(ACME)
    .bind(&workspace)
    .execute(s.app.db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO storage_locations (id,org_id,workspace_id,name,credential_id,bucket,prefix,created_by,created_ms) \
         VALUES ($1,$2,$3,'location',$4,'bucket','prefix','bob',1)",
    )
    .bind(new_id())
    .bind(ACME)
    .bind(&workspace)
    .bind(&credential)
    .execute(s.app.db.pool())
    .await
    .unwrap();

    let run = seed_run(&s, &workspace, "bob", "bob", "succeeded", &target, &volume).await;
    let task = request_erase(&s, &ana(), "user:bob").await;
    sqlx::query("UPDATE tasks SET requested_by = 'bob', on_behalf_of = 'bob', last_error = 'bob' WHERE id = $1")
        .bind(&task)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO tasks (id,org_id,kind,subject,requested_by,on_behalf_of,state,attempts,lease_until,created_ms,last_error) VALUES ($1,$2,'drop','other','cy','','done',1,0,1,'diagnostic mentions bob and user:bob')")
        .bind(new_id())
        .bind(ACME)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(s.work().await, 1);

    for (table, column, value) in [
        ("orgs", "created_by", "bob"),
        ("account_admins", "principal", "user:bob"),
        ("account_admins", "added_by", "bob"),
        ("workspaces", "created_by", "bob"),
        ("workspace_members", "principal", "user:bob"),
        ("entitlements", "principal", "user:bob"),
        ("grants", "principal", "user:bob"),
        ("grants", "granted_by", "bob"),
        ("compute_targets", "created_by", "bob"),
        ("service_principals", "created_by", "bob"),
        ("storage_credentials", "created_by", "bob"),
        ("storage_locations", "created_by", "bob"),
        ("runs", "requested_by", "bob"),
        ("runs", "on_behalf_of", "bob"),
        ("tasks", "subject", "user:bob"),
        ("tasks", "requested_by", "bob"),
        ("tasks", "on_behalf_of", "bob"),
        ("tasks", "last_error", "bob"),
    ] {
        let query = format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1");
        let count: i64 = sqlx::query_scalar(&query)
            .bind(value)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "{table}.{column} retained {value}");
    }
    let task_errors_with_subject: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE org_id = $1 AND last_error LIKE '%bob%'",
    )
    .bind(ACME)
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    assert_eq!(
        task_errors_with_subject, 0,
        "task diagnostics must not retain the erased identity"
    );

    let (run_requester, run_spec, run_result): (String, String, String) =
        sqlx::query_as("SELECT requested_by, spec, result FROM runs WHERE id = $1")
            .bind(run)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_ne!(run_requester, "bob");
    assert!(!run_spec.contains("bob"));
    assert!(!run_result.contains("private"));
}
both!(all_person_references_are_removed_or_tombstoned);

async fn run_results_tokens_and_shared_output_files_are_erased(s: Server) {
    let workspace = s.acme().await;
    let catalog = s.make(&workspace, &ana(), "catalog", "", "data").await;
    let schema = s.make(&workspace, &ana(), "schema", &catalog, "main").await;
    let output = s
        .make(&workspace, &ana(), "volume", &schema, "output")
        .await;
    let other_run = new_id();
    let run = seed_run(&s, &workspace, "bob", "", "succeeded", "worker", &output).await;
    s.upload(
        &workspace,
        &output,
        &format!("runs/{run}/private.txt"),
        &ana(),
        b"private output",
    )
    .await;
    s.upload(
        &workspace,
        &output,
        &format!("runs/{other_run}/keep.txt"),
        &ana(),
        b"other run",
    )
    .await;
    s.upload(&workspace, &output, "keep.txt", &ana(), b"keep")
        .await;

    request_erase(&s, &ana(), "user:bob").await;
    s.work().await;

    let token_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM run_tokens WHERE run_id = $1")
        .bind(&run)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(token_count, 0, "serialized run identities are credentials");
    let (spec, result, stdout, stderr, error): (String, String, String, String, String) =
        sqlx::query_as("SELECT spec, result, stdout, stderr, error FROM runs WHERE id = $1")
            .bind(&run)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert!(
        spec.is_empty()
            && result.is_empty()
            && stdout.is_empty()
            && stderr.is_empty()
            && error.is_empty()
    );
    assert_eq!(
        s.download(
            &workspace,
            &output,
            &format!("runs/{run}/private.txt"),
            &ana()
        )
        .await
        .status,
        404
    );
    assert_eq!(
        s.download(
            &workspace,
            &output,
            &format!("runs/{other_run}/keep.txt"),
            &ana()
        )
        .await
        .bytes,
        b"other run"
    );
    assert_eq!(
        s.download(&workspace, &output, "keep.txt", &ana())
            .await
            .bytes,
        b"keep"
    );
}
both!(run_results_tokens_and_shared_output_files_are_erased);

async fn an_active_run_cannot_restore_results_after_erase(s: Server) {
    let workspace = s.acme().await;
    let catalog = s
        .make(&workspace, &ana(), "catalog", "", "active_run")
        .await;
    let schema = s.make(&workspace, &ana(), "schema", &catalog, "main").await;
    let output = s
        .make(&workspace, &ana(), "volume", &schema, "output")
        .await;
    let target = new_id();
    let secret = "erase-test-compute-secret";
    sqlx::query(
        "INSERT INTO compute_targets (id,org_id,workspace_id,name,secret_hash,kind,endpoint,max_memory_mb,max_cpu_millis,created_by,created_ms,last_seen_ms) \
         VALUES ($1,$2,$3,'worker',$4,'agent','',1,1,'ana',1,0)",
    )
    .bind(&target)
    .bind(ACME)
    .bind(&workspace)
    .bind(hash_secret(secret))
    .execute(s.app.db.pool())
    .await
    .unwrap();
    let run = seed_run(&s, &workspace, "bob", "", "running", &target, &output).await;
    s.upload(
        &workspace,
        &output,
        &format!("runs/{run}/partial.txt"),
        &ana(),
        b"partial output",
    )
    .await;

    request_erase(&s, &ana(), "user:bob").await;
    s.work().await;

    let response = s
        .send(
            "POST",
            &format!("/api/v1/agent/runs/{run}/complete"),
            Some(secret),
            Body::from(json!({"state":"succeeded","result":"restored private rows"}).to_string()),
        )
        .await;
    assert_ne!(
        response.status, 204,
        "a late runner must not restore results"
    );
    let result: String = sqlx::query_scalar("SELECT result FROM runs WHERE id = $1")
        .bind(&run)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert!(result.is_empty());
    assert_eq!(
        s.download(
            &workspace,
            &output,
            &format!("runs/{run}/partial.txt"),
            &ana()
        )
        .await
        .status,
        404
    );
}
both!(an_active_run_cannot_restore_results_after_erase);

async fn a_last_admin_erase_leaves_owned_data_untouched(s: Server) {
    let workspace = s.acme().await;
    let catalog = s
        .make(&workspace, &ana(), "catalog", "", "ana_private")
        .await;
    let schema = s.make(&workspace, &ana(), "schema", &catalog, "main").await;
    let volume = s.make(&workspace, &ana(), "volume", &schema, "keep").await;
    s.upload(&workspace, &volume, "keep.txt", &ana(), b"keep")
        .await;

    request_erase(&s, &ana(), "user:ana").await;
    assert_eq!(s.work().await, 1);

    assert_eq!(
        s.download(&workspace, &volume, "keep.txt", &ana())
            .await
            .bytes,
        b"keep",
        "the last-admin check must happen before any deletion"
    );
    assert!(s.store_dir(&volume).exists());
}
both!(a_last_admin_erase_leaves_owned_data_untouched);

async fn erasure_is_scoped_and_repeatable(s: Server) {
    s.acme().await;
    let rita = Who {
        sub: "rita".into(),
        orgs: vec![(RIVAL.into(), "rival".into())],
        groups: Vec::new(),
        roles: vec!["account-admin".into()],
        acting: None,
        client: None,
    };
    assert_eq!(
        s.post(&format!("/api/v1/orgs/{RIVAL}/account"), &rita, json!({}))
            .await
            .status,
        201
    );
    let rival_ws = s.workspace(&rita, RIVAL, "Theirs").await;
    let catalog = s.make(&rival_ws, &rita, "catalog", "", "bob_data").await;
    sqlx::query("UPDATE securables SET owner = 'user:bob' WHERE id = $1")
        .bind(&catalog)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let bob_in_rival = s.make(&rival_ws, &rita, "schema", &catalog, "kept").await;

    request_erase(&s, &ana(), "user:bob").await;
    assert_eq!(s.work().await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM securables WHERE id = $1")
            .bind(&bob_in_rival)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        1,
        "the same subject's other-organization data is not touched"
    );
    request_erase(&s, &ana(), "user:bob").await;
    assert_eq!(s.work().await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM securables WHERE id = $1")
            .bind(&bob_in_rival)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap(),
        1
    );
}
both!(erasure_is_scoped_and_repeatable);

async fn a_grant_cannot_race_a_pending_erase(s: Server) {
    let workspace = s.acme().await;
    let catalog = s.make(&workspace, &ana(), "catalog", "", "sales").await;
    let erase = s
        .post(
            &format!("/api/v1/orgs/{ACME}/erase"),
            &ana(),
            json!({"subject":"user:bob"}),
        )
        .await;
    assert_eq!(erase.status, 202);

    let grant = s
        .put(
            &format!("/api/v1/workspaces/{workspace}/securables/{catalog}/grants"),
            &ana(),
            json!({"principal":"user:bob","role":"reader"}),
        )
        .await;
    assert_eq!(grant.status, 404, "pending erasure hides the principal");
    s.work().await;
    let grants: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM grants WHERE org_id = $1 AND principal = 'user:bob'",
    )
    .bind(ACME)
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    assert_eq!(grants, 0);
}
both!(a_grant_cannot_race_a_pending_erase);

async fn non_admins_and_non_people_are_hidden(s: Server) {
    let workspace = s.acme().await;
    s.assign(&workspace, &[&bob()]).await;
    let erase = format!("/api/v1/orgs/{ACME}/erase");
    assert_eq!(
        s.post(&erase, &bob(), json!({"subject":"user:cy"}))
            .await
            .status,
        404
    );
    assert_eq!(
        s.get(&format!("/api/v1/orgs/{ACME}/audit/verify"), &bob())
            .await
            .status,
        404
    );
    assert_eq!(
        s.post(&erase, &person("stranger"), json!({"subject":"user:cy"}))
            .await
            .status,
        404
    );
    for subject in ["group:/finance", "sp:service"] {
        assert_eq!(
            s.post(&erase, &ana(), json!({"subject":subject}))
                .await
                .status,
            400,
            "only people can be erased"
        );
    }
}
both!(non_admins_and_non_people_are_hidden);

async fn audit_stays_verifiable_and_names_the_first_tampered_entry(s: Server) {
    let workspace = s.acme().await;
    s.assign(&workspace, &[&bob()]).await;
    let catalog = s.make(&workspace, &ana(), "catalog", "", "shared").await;
    s.grant(&workspace, &ana(), &catalog, "user:bob", "reader")
        .await;
    request_erase(&s, &ana(), "user:bob").await;
    s.work().await;

    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT action, actor, on_behalf_of, target, hash FROM audit WHERE org_id = $1",
    )
    .bind(ACME)
    .fetch_all(s.app.db.pool())
    .await
    .unwrap();
    let rendered = format!("{rows:?}");
    assert!(
        !rendered.contains("bob"),
        "audit rows must not retain the raw subject"
    );
    let verify = format!("/api/v1/orgs/{ACME}/audit/verify");
    let intact = s.get(&verify, &ana()).await.json();
    assert_eq!(intact["intact"], true);
    let actions: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    assert!(actions.contains(&"erase.requested".into()));
    assert!(actions.contains(&"erase.done".into()));

    sqlx::query("UPDATE audit SET action = 'tampered' WHERE org_id = $1 AND seq = 2")
        .bind(ACME)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    let broken = s.get(&verify, &ana()).await.json();
    assert_eq!(broken["intact"], false);
    assert_eq!(broken["first_broken"], 2);
}
both!(audit_stays_verifiable_and_names_the_first_tampered_entry);

async fn audit_names_are_org_scoped_and_deleted_without_rewriting_history(s: Server) {
    let workspace = s.acme().await;
    s.assign(&workspace, &[&bob()]).await;
    let catalog = s.make(&workspace, &ana(), "catalog", "", "shared").await;
    s.grant(&workspace, &ana(), &catalog, "user:bob", "contributor")
        .await;
    s.make(&workspace, &bob(), "schema", &catalog, "bob_data")
        .await;

    let rita = Who {
        sub: "rita".into(),
        orgs: vec![(RIVAL.into(), "rival".into())],
        groups: Vec::new(),
        roles: vec!["account-admin".into()],
        acting: None,
        client: None,
    };
    assert_eq!(
        s.post(&format!("/api/v1/orgs/{RIVAL}/account"), &rita, json!({}))
            .await
            .status,
        201
    );
    let rival_workspace = s.workspace(&rita, RIVAL, "Theirs").await;
    let rival_bob = person("bob").also_in(RIVAL, "rival");
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{rival_workspace}/members"),
            &rita,
            json!({"principal":"user:bob","role":"user"}),
        )
        .await
        .status,
        204
    );
    let rival_catalog = s
        .make(&rival_workspace, &rita, "catalog", "", "shared")
        .await;
    s.grant(
        &rival_workspace,
        &rita,
        &rival_catalog,
        "user:bob",
        "contributor",
    )
    .await;
    s.make(
        &rival_workspace,
        &rival_bob,
        "schema",
        &rival_catalog,
        "bob_data",
    )
    .await;

    let before = s
        .get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
        .await;
    assert!(
        before.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["actor"] == "bob")
    );
    assert!(
        before.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["target"]
                    .as_str()
                    .is_some_and(|target| target.contains("user:bob"))
            })
    );
    let before_rows: Vec<AuditRow> = sqlx::query_as(
        "SELECT org_id,seq,at_ms,actor,on_behalf_of,action,target,outcome,prev_hash,hash FROM audit WHERE org_id = $1 ORDER BY seq",
    )
    .bind(ACME)
    .fetch_all(s.app.db.pool())
    .await
    .unwrap();
    let refs: Vec<(String, String)> = sqlx::query_as(
        "SELECT org_id,ref FROM audit_names WHERE name = 'bob' AND org_id IN ($1,$2)",
    )
    .bind(ACME)
    .bind(RIVAL)
    .fetch_all(s.app.db.pool())
    .await
    .unwrap();
    assert_eq!(refs.len(), 2);
    assert_ne!(
        refs[0].1, refs[1].1,
        "refs must not correlate organizations"
    );
    assert!(
        refs.iter()
            .all(|(_, reference)| reference.starts_with("p_"))
    );
    let rival_ref = refs.iter().find(|(org, _)| org == RIVAL).unwrap().1.clone();

    request_erase(&s, &ana(), "user:bob").await;
    s.work().await;

    let after_rows: Vec<AuditRow> = sqlx::query_as(
        "SELECT org_id,seq,at_ms,actor,on_behalf_of,action,target,outcome,prev_hash,hash FROM audit WHERE org_id = $1 ORDER BY seq",
    )
    .bind(ACME)
    .fetch_all(s.app.db.pool())
    .await
    .unwrap();
    assert_eq!(&after_rows[..before_rows.len()], before_rows.as_slice());
    let after = s
        .get(&format!("/api/v1/orgs/{ACME}/audit?limit=500"), &ana())
        .await
        .json();
    assert!(
        after["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["actor"] == "erased person")
    );
    assert!(after["items"].as_array().unwrap().iter().any(|entry| {
        entry["target"]
            .as_str()
            .is_some_and(|target| target.contains("user:erased person"))
    }));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_names WHERE org_id = $1 AND name = 'bob'"
        )
        .bind(ACME)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT ref FROM audit_names WHERE org_id = $1 AND name = 'bob'"
        )
        .bind(RIVAL)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap(),
        rival_ref
    );
    assert_eq!(
        s.get(&format!("/api/v1/orgs/{ACME}/audit/verify"), &ana())
            .await
            .json()["intact"],
        true
    );
    assert!(
        s.get(&format!("/api/v1/orgs/{RIVAL}/audit?limit=500"), &rita)
            .await
            .json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["actor"] == "bob")
    );
}
both!(audit_names_are_org_scoped_and_deleted_without_rewriting_history);

async fn erase_request_and_completion_are_audited_as_their_changes(s: Server) {
    let _workspace = s.acme().await;
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE org_id = $1")
        .bind(ACME)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    let response = s
        .post(
            &format!("/api/v1/orgs/{ACME}/erase"),
            &ana(),
            json!({"subject":"user:bob"}),
        )
        .await;
    assert_eq!(response.status, 202);
    let task = response.json()["task_id"].as_str().unwrap().to_owned();
    let paired: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE org_id = $1 AND action = 'erase.requested'",
    )
    .bind(ACME)
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    assert_eq!(paired, 1);
    assert_eq!(s.work().await, 1);
    let done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit WHERE org_id = $1 AND action = 'erase.done'",
    )
    .bind(ACME)
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit WHERE org_id = $1")
        .bind(ACME)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(state, "done");
    assert_eq!(done, 1);
    assert!(after >= before + 2);
}
both!(erase_request_and_completion_are_audited_as_their_changes);
