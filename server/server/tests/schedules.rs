//! Durable schedules and failure alerts on `SQLite` and `Postgres`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Server, ana, bob, fresh_db, server};
use loamz_data_server::{
    compute::service as compute, governance::lifecycle::service as lifecycle, kernel::now_ms,
};
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

fn items(reply: &common::Reply) -> Vec<Value> {
    reply.json()["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn sql_schedule(name: &str) -> Value {
    json!({
        "name": name,
        "interval_seconds": 60,
        "sql": "SELECT 1"
    })
}

async fn schedules_can_be_managed_without_returning_the_webhook_reference(s: Server) {
    let ws = s.acme().await;
    let request = json!({
        "name": "morning",
        "cron": "0 9 * * 1-5",
        "sql": "SELECT 1",
        "webhook_ref": "loamz-data/alerts#hook"
    });
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            request,
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    assert!(!created.text().contains("loamz-data/alerts#hook"));
    let id = created.json()["id"].as_str().unwrap().to_owned();

    let listed = s
        .get(&format!("/api/v1/workspaces/{ws}/schedules"), &ana())
        .await;
    assert_eq!(listed.status, 200, "{}", listed.text());
    assert_eq!(items(&listed).len(), 1);
    assert_eq!(items(&listed)[0]["id"], id);
    assert_eq!(items(&listed)[0]["alerts_enabled"], true);
    assert!(!listed.text().contains("loamz-data/alerts#hook"));

    let paused = s
        .put(
            &format!("/api/v1/workspaces/{ws}/schedules/{id}/pause"),
            &ana(),
            json!({}),
        )
        .await;
    assert_eq!(paused.status, 204, "{}", paused.text());
    assert_eq!(
        items(
            &s.get(&format!("/api/v1/workspaces/{ws}/schedules"), &ana())
                .await
        )[0]["state"],
        "paused"
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{ws}/schedules/{id}/resume"),
            &ana(),
            json!({}),
        )
        .await
        .status,
        204
    );
    assert_eq!(
        s.delete(&format!("/api/v1/workspaces/{ws}/schedules/{id}"), &ana())
            .await
            .status,
        204
    );
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{ws}/schedules/{id}/resume"),
            &ana(),
            json!({}),
        )
        .await
        .status,
        404
    );
    assert!(
        items(
            &s.get(&format!("/api/v1/workspaces/{ws}/schedules"), &ana())
                .await
        )
        .is_empty()
    );
    assert_eq!(
        s.post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            sql_schedule("morning"),
        )
        .await
        .status,
        201
    );
}
both!(schedules_can_be_managed_without_returning_the_webhook_reference);

async fn invalid_cadence_or_definition_is_refused(s: Server) {
    let ws = s.acme().await;
    for bad in [
        json!({"name":"bad","cron":"not cron","sql":"SELECT 1"}),
        json!({"name":"bad","interval_seconds":0,"sql":"SELECT 1"}),
        json!({"name":"bad","interval_seconds":60,"sql":"SELECT 1","job":{"script":"pass"}}),
        json!({"name":"bad","interval_seconds":60}),
    ] {
        let reply = s
            .post(&format!("/api/v1/workspaces/{ws}/schedules"), &ana(), bad)
            .await;
        assert_eq!(reply.status, 400, "{}", reply.text());
    }
}
both!(invalid_cadence_or_definition_is_refused);

async fn webhook_references_are_validated_but_never_reflected(s: Server) {
    let ws = s.acme().await;
    let mut request = sql_schedule("webhook");
    request["webhook_ref"] = json!("loamz-data/alerts#hook");
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            request,
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    assert!(created.json()["alerts_enabled"].as_bool().unwrap());
    assert!(!created.text().contains("loamz-data/alerts#hook"));

    let mut invalid = sql_schedule("insecure webhook");
    invalid["webhook_ref"] = json!("loamz-data/alerts#insecure");
    let refused = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            invalid,
        )
        .await;
    assert_eq!(refused.status, 400, "{}", refused.text());
}
both!(webhook_references_are_validated_but_never_reflected);

async fn a_member_cannot_list_or_change_another_persons_schedule(s: Server) {
    let ws = s.acme().await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            sql_schedule("private"),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    s.assign(&ws, &[&bob()]).await;
    let listed = s
        .get(&format!("/api/v1/workspaces/{ws}/schedules"), &bob())
        .await;
    assert_eq!(listed.status, 200, "{}", listed.text());
    assert!(items(&listed).is_empty());
    assert_eq!(
        s.put(
            &format!(
                "/api/v1/workspaces/{ws}/schedules/{}/pause",
                created.json()["id"].as_str().unwrap()
            ),
            &bob(),
            json!({}),
        )
        .await
        .status,
        404
    );
}
both!(a_member_cannot_list_or_change_another_persons_schedule);

async fn erasing_the_owner_clears_the_saved_definition_and_webhook_reference(s: Server) {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{ws}/entitlements"),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "sql-access"}),
    )
    .await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &bob(),
            json!({
                "name": "bob-private",
                "interval_seconds": 60,
                "sql": "SELECT 1",
                "webhook_ref": "loamz-data/alerts#hook"
            }),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());

    let erased = s
        .post(
            "/api/v1/orgs/acme/erase",
            &ana(),
            json!({"subject": "user:bob"}),
        )
        .await;
    assert_eq!(erased.status, 202, "{}", erased.text());
    assert_eq!(
        s.post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &bob(),
            sql_schedule("too late"),
        )
        .await
        .status,
        409
    );
    assert_eq!(
        s.put(
            &format!(
                "/api/v1/workspaces/{ws}/schedules/{}/resume",
                created.json()["id"].as_str().unwrap()
            ),
            &bob(),
            json!({}),
        )
        .await
        .status,
        409
    );
    let pending_state: String = sqlx::query_scalar("SELECT state FROM schedules WHERE id = $1")
        .bind(created.json()["id"].as_str().unwrap())
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(pending_state, "paused");
    lifecycle::run_once(&s.app, created.json()["next_run_ms"].as_i64().unwrap() + 1)
        .await
        .unwrap();

    let (state, definition, webhook_ref, owner): (String, String, String, String) =
        sqlx::query_as("SELECT state, definition, webhook_ref, owner FROM schedules WHERE id = $1")
            .bind(created.json()["id"].as_str().unwrap())
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(state, "deleted");
    assert!(definition.is_empty());
    assert!(webhook_ref.is_empty());
    assert!(!owner.contains("bob"));
    let runs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE schedule_task_id IS NOT NULL")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(runs, 0);
}
both!(erasing_the_owner_clears_the_saved_definition_and_webhook_reference);

async fn schedule_task_creates_one_run_and_coalesces_missed_intervals(s: Server) {
    let ws = s.acme().await;
    let target = create_online_target(&s, &ws).await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            sql_schedule("periodic"),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let schedule = created.json();
    let id = schedule["id"].as_str().unwrap();
    let due = schedule["next_run_ms"].as_i64().unwrap() + 5 * 60_000;

    assert!(lifecycle::run_once(&s.app, due).await.unwrap());
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM runs WHERE schedule_task_id IS NOT NULL AND target_id = $1",
    )
    .bind(target)
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    assert_eq!(count, 1, "missed periods coalesce into one run");

    assert!(!lifecycle::run_once(&s.app, due + 60_000).await.unwrap());
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE schedule_task_id IS NOT NULL")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(count, 1, "a queued run prevents an overlap");

    let state: String = sqlx::query_scalar("SELECT state FROM schedules WHERE id = $1")
        .bind(id)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(state, "active");
}
both!(schedule_task_creates_one_run_and_coalesces_missed_intervals);

async fn scheduled_jobs_use_the_normal_job_submission_path(s: Server) {
    let ws = s.acme().await;
    let catalog = s.make(&ws, &ana(), "catalog", "", "data").await;
    let schema = s.make(&ws, &ana(), "schema", &catalog, "raw").await;
    let volume = s.make(&ws, &ana(), "volume", &schema, "output").await;
    let target = create_online_target(&s, &ws).await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            json!({
                "name": "nightly-job",
                "interval_seconds": 60,
                "job": {"script": "pass", "output": volume}
            }),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let due = created.json()["next_run_ms"].as_i64().unwrap();
    lifecycle::run_once(&s.app, due).await.unwrap();

    let (kind, run_target): (String, String) =
        sqlx::query_as("SELECT kind, target_id FROM runs WHERE schedule_task_id IS NOT NULL")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(kind, "job");
    assert_eq!(run_target, target);
}
both!(scheduled_jobs_use_the_normal_job_submission_path);

async fn scheduled_maintenance_uses_the_same_table_write_checks(s: Server) {
    let ws = s.acme().await;
    let catalog = s.make(&ws, &ana(), "catalog", "", "data").await;
    let schema = s.make(&ws, &ana(), "schema", &catalog, "raw").await;
    let table = s.make(&ws, &ana(), "table", &schema, "events").await;
    let target = create_online_target(&s, &ws).await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            json!({
                "name": "compact-events",
                "interval_seconds": 60,
                "maintenance": {"table_id": table, "action": "compact"}
            }),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());

    let due = created.json()["next_run_ms"].as_i64().unwrap();
    lifecycle::run_once(&s.app, due).await.unwrap();
    let (kind, run_target, spec): (String, String, String) =
        sqlx::query_as("SELECT kind, target_id, spec FROM runs WHERE schedule_task_id IS NOT NULL")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(kind, "job");
    assert_eq!(run_target, target);
    let spec: Value = serde_json::from_str(&spec).unwrap();
    assert_eq!(spec["table_writes"][0]["id"], table);
    assert!(
        spec["script"]
            .as_str()
            .unwrap()
            .contains("iceberg_rewrite_data_files")
    );
}
both!(scheduled_maintenance_uses_the_same_table_write_checks);

async fn scheduled_maintenance_pauses_when_modify_is_revoked(s: Server) {
    let ws = s.acme().await;
    let catalog = s.make(&ws, &ana(), "catalog", "", "data").await;
    let schema = s.make(&ws, &ana(), "schema", &catalog, "raw").await;
    let table = s.make(&ws, &ana(), "table", &schema, "events").await;
    s.assign(&ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{ws}/entitlements"),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "compute-create"}),
    )
    .await;
    s.grant(&ws, &ana(), &table, "user:bob", "contributor")
        .await;
    create_online_target(&s, &ws).await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &bob(),
            json!({
                "name": "compact-events",
                "interval_seconds": 60,
                "maintenance": {"table_id": table, "action": "compact"}
            }),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let schedule = created.json();
    let id = schedule["id"].as_str().unwrap();
    let due = schedule["next_run_ms"].as_i64().unwrap();
    assert_eq!(
        s.delete(
            &format!("/api/v1/workspaces/{ws}/securables/{table}/grants/user:bob"),
            &ana(),
        )
        .await
        .status,
        204
    );

    lifecycle::run_once(&s.app, due).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM schedules WHERE id = $1")
        .bind(id)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(state, "paused");
    let runs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE schedule_task_id IS NOT NULL")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(runs, 0);
}
both!(scheduled_maintenance_pauses_when_modify_is_revoked);

async fn a_paused_schedule_discards_an_already_queued_firing(s: Server) {
    let ws = s.acme().await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            sql_schedule("pause race"),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let id = created.json()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        s.put(
            &format!("/api/v1/workspaces/{ws}/schedules/{id}/pause"),
            &ana(),
            json!({}),
        )
        .await
        .status,
        204
    );
    let task = loamz_data_server::kernel::new_id();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO tasks (id, org_id, kind, subject, requested_by, on_behalf_of, state, attempts, lease_until, created_ms, last_error, alert_ref) \
         VALUES ($1, 'acme', 'schedule.run', $2, 'ana', 'user:ana', 'pending', 0, 0, $3, '', '')",
    )
    .bind(&task)
    .bind(&id)
    .bind(now)
    .execute(s.app.db.pool())
    .await
    .unwrap();

    assert!(lifecycle::run_once(&s.app, now + 1).await.unwrap());
    let runs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE schedule_task_id IS NOT NULL")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(runs, 0);
    assert_eq!(state, "done");
}
both!(a_paused_schedule_discards_an_already_queued_firing);

async fn two_workers_and_a_reclaimed_task_do_not_duplicate_a_firing(s: Server) {
    let ws = s.acme().await;
    create_online_target(&s, &ws).await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            sql_schedule("once"),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let due = created.json()["next_run_ms"].as_i64().unwrap();
    let second = server(s.app.db.clone()).await;
    let (a, b) = tokio::join!(
        lifecycle::run_once(&s.app, due),
        lifecycle::run_once(&second.app, due)
    );
    assert!(a.unwrap() || b.unwrap());

    let task: String = sqlx::query_scalar(
        "SELECT id FROM tasks WHERE kind = 'schedule.run' ORDER BY created_ms LIMIT 1",
    )
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET state = 'pending', lease_until = 0 WHERE id = $1")
        .bind(task)
        .execute(s.app.db.pool())
        .await
        .unwrap();
    lifecycle::run_once(&s.app, due + 120_000).await.unwrap();

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE schedule_task_id IS NOT NULL")
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
}
both!(two_workers_and_a_reclaimed_task_do_not_duplicate_a_firing);

async fn a_person_who_loses_input_access_has_the_schedule_paused(s: Server) {
    let ws = s.acme().await;
    let catalog = s.make(&ws, &ana(), "catalog", "", "data").await;
    let schema = s.make(&ws, &ana(), "schema", &catalog, "raw").await;
    let table = s.table(&ws, &ana(), &schema, "orders", "").await;
    s.assign(&ws, &[&bob()]).await;
    s.put(
        &format!("/api/v1/workspaces/{ws}/entitlements"),
        &ana(),
        json!({"principal": "user:bob", "entitlement": "sql-access"}),
    )
    .await;
    s.grant(&ws, &ana(), &table, "user:bob", "reader").await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &bob(),
            json!({
                "name": "orders",
                "interval_seconds": 60,
                "sql": "SELECT * FROM data.raw.orders"
            }),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let id = created.json()["id"].as_str().unwrap().to_owned();
    let due = created.json()["next_run_ms"].as_i64().unwrap();
    s.delete(
        &format!("/api/v1/workspaces/{ws}/securables/{table}/grants/user:bob"),
        &ana(),
    )
    .await;

    lifecycle::run_once(&s.app, due).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM schedules WHERE id = $1")
        .bind(id)
        .fetch_one(s.app.db.pool())
        .await
        .unwrap();
    assert_eq!(state, "paused");
}
both!(a_person_who_loses_input_access_has_the_schedule_paused);

async fn a_timed_out_scheduled_run_is_recorded_as_an_alert(s: Server) {
    let ws = s.acme().await;
    create_online_target(&s, &ws).await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            sql_schedule("alert me"),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let due = created.json()["next_run_ms"].as_i64().unwrap();
    lifecycle::run_once(&s.app, due).await.unwrap();
    let run: String = sqlx::query_scalar(
        "SELECT id FROM runs WHERE schedule_task_id IS NOT NULL ORDER BY created_ms DESC LIMIT 1",
    )
    .fetch_one(s.app.db.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE runs SET state = 'timed_out', finished_ms = $2 WHERE id = $1")
        .bind(&run)
        .bind(now_ms())
        .execute(s.app.db.pool())
        .await
        .unwrap();
    lifecycle::run_once(&s.app, now_ms() + 1).await.unwrap();

    let alerts = s
        .get(
            &format!(
                "/api/v1/workspaces/{ws}/schedules/{}/alerts",
                created.json()["id"].as_str().unwrap()
            ),
            &ana(),
        )
        .await;
    assert_eq!(alerts.status, 200, "{}", alerts.text());
    assert_eq!(items(&alerts).len(), 1);
    assert_eq!(items(&alerts)[0]["run_id"], run);
    assert_eq!(items(&alerts)[0]["state"], "recorded");
}
both!(a_timed_out_scheduled_run_is_recorded_as_an_alert);

async fn webhook_delivery_rejects_private_addresses_and_stops_after_bounded_retries(s: Server) {
    let ws = s.acme().await;
    let created = s
        .post(
            &format!("/api/v1/workspaces/{ws}/schedules"),
            &ana(),
            sql_schedule("private destination"),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let id = created.json()["id"].as_str().unwrap().to_owned();
    let alert = loamz_data_server::kernel::new_id();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO schedule_alerts (id, org_id, workspace_id, schedule_id, run_id, destination_ref, state, attempts, next_try_ms, created_ms, sent_ms, last_error) \
         VALUES ($1, 'acme', $2, $3, 'run-test', 'loamz-data/alerts#private', 'pending', 7, $4, $4, 0, '')",
    )
    .bind(&alert)
    .bind(&ws)
    .bind(&id)
    .bind(now)
    .execute(s.app.db.pool())
    .await
    .unwrap();

    assert!(lifecycle::run_once(&s.app, now).await.unwrap());
    let (state, attempts, last_error): (String, i64, String) =
        sqlx::query_as("SELECT state, attempts, last_error FROM schedule_alerts WHERE id = $1")
            .bind(alert)
            .fetch_one(s.app.db.pool())
            .await
            .unwrap();
    assert_eq!(state, "failed");
    assert_eq!(attempts, 8);
    assert_eq!(last_error, "delivery failed");
}
both!(webhook_delivery_rejects_private_addresses_and_stops_after_bounded_retries);

async fn create_online_target(s: &Server, ws: &str) -> String {
    let target = s
        .post(
            &format!("/api/v1/workspaces/{ws}/compute/targets"),
            &ana(),
            json!({"name": "schedule-worker", "max_memory_mb": 512, "max_cpu_millis": 1000}),
        )
        .await;
    assert_eq!(target.status, 201, "{}", target.text());
    let secret = target.json()["secret"].as_str().unwrap().to_owned();
    compute::authenticate_agent(&s.app, &secret).await.unwrap();
    target.json()["id"].as_str().unwrap().to_owned()
}
