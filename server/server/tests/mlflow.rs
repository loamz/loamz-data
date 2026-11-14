//! The `MLflow` gateway (proposal 0008), against a fake upstream `MLflow` that records what it was asked. What matters here is what the
//! gateway lets through and what it keeps back: the upstream is never reachable except through the grants of the caller.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod common;

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use common::{RIVAL, Server, Who, ana, bob, fresh_db, person};
use serde_json::{Value, json};

/// A stand-in for an `MLflow` server: it keeps experiments and runs in memory and remembers every request it receives.
#[derive(Default)]
struct Fake {
    experiments: Vec<(String, String, bool)>,
    runs: Vec<(String, String, Value)>,
    seen: Vec<(String, Value)>,
}

type Shared = Arc<Mutex<Fake>>;

async fn start() -> (String, Shared) {
    let fake: Shared = Arc::default();
    let router = Router::new()
        .route("/version", get(|| async { "3.18.0" }))
        .route(
            "/api/2.0/mlflow/experiments/create",
            post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                let mut f = f.lock().unwrap();
                f.seen.push(("experiments/create".into(), b.clone()));
                let id = (f.experiments.len() + 1).to_string();
                f.experiments.push((id.clone(), b["name"].as_str().unwrap().to_owned(), true));
                Json(json!({"experiment_id": id}))
            }),
        )
        .route(
            "/api/2.0/mlflow/experiments/get",
            get(|State(f): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let f = f.lock().unwrap();
                let id = &q["experiment_id"];
                let e = f.experiments.iter().find(|e| &e.0 == id).unwrap();
                Json(json!({"experiment": {"experiment_id": e.0, "name": e.1, "lifecycle_stage": "active"}}))
            }),
        )
        .route(
            "/api/2.0/mlflow/experiments/get-by-name",
            get(|State(f): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let f = f.lock().unwrap();
                let e = f.experiments.iter().find(|e| e.1 == q["experiment_name"]).unwrap();
                Json(json!({"experiment": {"experiment_id": e.0, "name": e.1, "lifecycle_stage": "active"}}))
            }),
        )
        .route(
            "/api/2.0/mlflow/experiments/search",
            post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                let mut f = f.lock().unwrap();
                f.seen.push(("experiments/search".into(), b.clone()));
                let prefix = b["filter"].as_str().unwrap().trim_start_matches("name LIKE '").trim_end_matches("%'").to_owned();
                let all: Vec<Value> = f
                    .experiments
                    .iter()
                    .filter(|e| e.1.starts_with(&prefix))
                    .map(|e| json!({"experiment_id": e.0, "name": e.1, "lifecycle_stage": "active"}))
                    .collect();
                Json(json!({"experiments": all}))
            }),
        )
        .route(
            "/api/2.0/mlflow/experiments/delete",
            post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                f.lock().unwrap().seen.push(("experiments/delete".into(), b));
                Json(json!({}))
            }),
        )
        .route(
            "/api/2.0/mlflow/runs/create",
            post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                let mut f = f.lock().unwrap();
                f.seen.push(("runs/create".into(), b.clone()));
                let id = format!("run{:030}", f.runs.len() + 1);
                f.runs.push((id.clone(), b["experiment_id"].as_str().unwrap().to_owned(), b.clone()));
                Json(json!({"run": {"info": {"run_id": id, "experiment_id": b["experiment_id"], "user_id": b["user_id"]}}}))
            }),
        )
        .route(
            "/api/2.0/mlflow/runs/get",
            get(|State(f): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let f = f.lock().unwrap();
                let r = f.runs.iter().find(|r| r.0 == q["run_id"]).unwrap();
                Json(json!({"run": {"info": {"run_id": r.0, "experiment_id": r.1}}}))
            }),
        )
        .route(
            "/api/2.0/mlflow/runs/log-batch",
            post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                f.lock().unwrap().seen.push(("runs/log-batch".into(), b));
                Json(json!({}))
            }),
        )
        .route(
            "/api/2.0/mlflow/runs/search",
            post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                f.lock().unwrap().seen.push(("runs/search".into(), b));
                Json(json!({"runs": []}))
            }),
        )
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, fake)
}

fn calls(f: &Shared, name: &str) -> Vec<Value> {
    f.lock()
        .unwrap()
        .seen
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, b)| b.clone())
        .collect()
}

struct Lab {
    ws: String,
    schema: String,
    base: String,
}

impl Lab {
    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base)
    }
}

async fn lab(s: &Server) -> Lab {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob()]).await;
    let catalog = s.make(&ws, &ana(), "catalog", "", "ai").await;
    let schema = s.make(&ws, &ana(), "schema", &catalog, "main").await;
    Lab {
        base: format!("/api/v1/workspaces/{ws}/mlflow"),
        ws,
        schema,
    }
}

fn mlflow_error(r: &common::Reply, status: u16, code: &str) {
    assert_eq!(r.status, status, "{}", r.text());
    assert_eq!(r.json()["error_code"], code, "{}", r.text());
}

#[allow(clippy::too_many_lines)]
async fn the_gateway_decides_for_every_request(s: Server, fake: Shared) {
    let l = lab(&s).await;
    let create = l.url("api/2.0/mlflow/experiments/create");
    // Made by someone with CREATE_EXPERIMENT (the catalog's owner), named like a table; the upstream name is kept apart per workspace.
    let made = s
        .post(&create, &ana(), json!({"name": "ai.main.churn"}))
        .await;
    assert_eq!(made.status, 200, "{}", made.text());
    let exp = made.json()["experiment_id"].as_str().unwrap().to_owned();
    let upstream = &calls(&fake, "experiments/create")[0];
    assert_eq!(upstream["name"], format!("{}.ai.main.churn", l.ws));
    assert!(upstream.get("artifact_location").is_none());
    // It is a catalog object now, in its schema.
    let listed = s
        .get(
            &format!(
                "/api/v1/workspaces/{}/securables?parent_id={}",
                l.ws, l.schema
            ),
            &ana(),
        )
        .await;
    assert!(
        listed.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "experiment" && i["name"] == "churn")
    );

    // What is not a good experiment is refused before MLflow hears of it.
    for body in [
        json!({"name": "churn"}),
        json!({"name": "ai.main.Churn"}),
        json!({"name": "ai.main.other", "artifact_location": "s3://elsewhere/x"}),
        json!({"name": "nowhere.main.x"}),
    ] {
        let r = s.post(&create, &ana(), body.clone()).await;
        assert!(r.status == 400 || r.status == 404, "{body}: {}", r.text());
        assert!(
            r.json()["error_code"].is_string(),
            "errors are in MLflow's shape: {}",
            r.text()
        );
    }
    mlflow_error(
        &s.post(&create, &ana(), json!({"name": "ai.main.churn"}))
            .await,
        400,
        "RESOURCE_ALREADY_EXISTS",
    );
    assert_eq!(
        calls(&fake, "experiments/create").len(),
        1,
        "nothing refused reached MLflow"
    );

    // A run, and the person it is for: the client's idea of who it is does not count.
    let run = s
        .post(
            &l.url("api/2.0/mlflow/runs/create"),
            &ana(),
            json!({"experiment_id": exp, "user_id": "someone-else"}),
        )
        .await;
    assert_eq!(run.status, 200, "{}", run.text());
    let run_id = run.json()["run"]["info"]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let sent = &calls(&fake, "runs/create")[0];
    assert_ne!(sent["user_id"], "someone-else");
    assert_eq!(sent["user_id"], run.json()["run"]["info"]["user_id"]);

    // Bob has been given nothing: to him none of it is there, by id, by name, in a search, or through a run.
    let by_id = format!(
        "{}?experiment_id={exp}",
        l.url("api/2.0/mlflow/experiments/get")
    );
    let by_name = format!(
        "{}?experiment_name=ai.main.churn",
        l.url("api/2.0/mlflow/experiments/get-by-name")
    );
    let run_get = format!("{}?run_id={run_id}", l.url("api/2.0/mlflow/runs/get"));
    mlflow_error(&s.get(&by_id, &bob()).await, 404, "RESOURCE_DOES_NOT_EXIST");
    mlflow_error(
        &s.get(&by_name, &bob()).await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );
    mlflow_error(
        &s.get(&run_get, &bob()).await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );
    let search = s
        .post(
            &l.url("api/2.0/mlflow/experiments/search"),
            &bob(),
            json!({"max_results": 10}),
        )
        .await;
    assert_eq!(search.json(), json!({"experiments": []}));
    mlflow_error(
        &s.post(
            &l.url("api/2.0/mlflow/runs/create"),
            &bob(),
            json!({"experiment_id": exp}),
        )
        .await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );
    mlflow_error(
        &s.post(
            &l.url("api/2.0/mlflow/runs/log-batch"),
            &bob(),
            json!({"run_id": run_id, "metrics": []}),
        )
        .await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );
    mlflow_error(
        &s.post(
            &l.url("api/2.0/mlflow/runs/search"),
            &bob(),
            json!({"experiment_ids": [exp]}),
        )
        .await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );
    let bobs = s
        .post(&create, &bob(), json!({"name": "ai.main.bobs"}))
        .await;
    assert!([403, 404].contains(&bobs.status), "{}", bobs.text());
    assert!(calls(&fake, "runs/search").is_empty() && calls(&fake, "runs/log-batch").is_empty());

    // A reader sees it and its runs, may not write; the name is the one a person gave.
    s.grant(&l.ws, &ana(), &l.schema, "user:bob", "reader")
        .await;
    let got = s.get(&by_id, &bob()).await;
    assert_eq!(got.status, 200, "{}", got.text());
    assert_eq!(got.json()["experiment"]["name"], "ai.main.churn");
    assert_eq!(
        s.get(&by_name, &bob()).await.json()["experiment"]["experiment_id"],
        exp
    );
    assert_eq!(s.get(&run_get, &bob()).await.status, 200);
    let found = s
        .post(
            &l.url("api/2.0/mlflow/experiments/search"),
            &bob(),
            json!({"max_results": 10}),
        )
        .await;
    assert_eq!(found.json()["experiments"][0]["name"], "ai.main.churn");
    assert!(found.json().get("next_page_token").is_none() && found.json().get("total").is_none());
    assert_eq!(
        s.post(
            &l.url("api/2.0/mlflow/runs/search"),
            &bob(),
            json!({"experiment_ids": [exp]})
        )
        .await
        .status,
        200
    );
    for (path, body) in [
        ("api/2.0/mlflow/runs/create", json!({"experiment_id": exp})),
        (
            "api/2.0/mlflow/runs/log-batch",
            json!({"run_id": run_id, "metrics": []}),
        ),
        (
            "api/2.0/mlflow/experiments/set-experiment-tag",
            json!({"experiment_id": exp, "key": "a", "value": "b"}),
        ),
    ] {
        let r = s.post(&l.url(path), &bob(), body).await;
        mlflow_error(&r, 403, "PERMISSION_DENIED");
    }

    // A contributor may write, and still not delete.
    s.grant(&l.ws, &ana(), &l.schema, "user:bob", "contributor")
        .await;
    assert_eq!(
        s.post(
            &l.url("api/2.0/mlflow/runs/log-batch"),
            &bob(),
            json!({"run_id": run_id, "metrics": []})
        )
        .await
        .status,
        200
    );
    let theirs = s
        .post(
            &l.url("api/2.0/mlflow/runs/create"),
            &bob(),
            json!({"experiment_id": exp}),
        )
        .await;
    assert_eq!(theirs.status, 200, "{}", theirs.text());
    mlflow_error(
        &s.post(
            &l.url("api/2.0/mlflow/experiments/delete"),
            &bob(),
            json!({"experiment_id": exp}),
        )
        .await,
        403,
        "PERMISSION_DENIED",
    );
    assert_eq!(
        s.post(
            &l.url("api/2.0/mlflow/experiments/delete"),
            &ana(),
            json!({"experiment_id": exp})
        )
        .await
        .status,
        200
    );
    assert_eq!(calls(&fake, "experiments/delete").len(), 1);

    // A run id is its own experiment's: a body that names another run is not smuggled through.
    mlflow_error(
        &s.post(
            &l.url("api/2.0/mlflow/runs/log-batch"),
            &bob(),
            json!({"run_id": run_id, "run_uuid": "another"}),
        )
        .await,
        400,
        "INVALID_PARAMETER_VALUE",
    );

    // Anything not on the list is not there: other MLflow areas, other methods, tricks.
    for (method, path) in [
        ("POST", "api/2.0/mlflow/registered-models/create"),
        ("GET", "api/2.0/mlflow-artifacts/artifacts"),
        ("POST", "ajax-api/2.0/mlflow/experiments/create"),
        ("GET", "api/2.0/mlflow/experiments/create"),
        ("POST", "api/2.0/mlflow/experiments/../experiments/create"),
    ] {
        let r = if method == "GET" {
            s.get(&l.url(path), &ana()).await
        } else {
            s.post(&l.url(path), &ana(), json!({})).await
        };
        assert_eq!(r.status, 404, "{method} {path}: {}", r.text());
    }
    // Without a token it is not served at all, and /version is answered.
    assert_eq!(
        s.send("GET", &l.url("version"), None, axum::body::Body::empty())
            .await
            .status,
        401
    );
    assert_eq!(s.get(&l.url("version"), &bob()).await.text(), "3.18.0");
}

async fn experiments_are_not_shared_between_workspaces_or_found_by_a_guessed_id(
    s: Server,
    fake: Shared,
) {
    let l = lab(&s).await;
    let made = s
        .post(
            &l.url("api/2.0/mlflow/experiments/create"),
            &ana(),
            json!({"name": "ai.main.churn"}),
        )
        .await;
    let exp = made.json()["experiment_id"].as_str().unwrap().to_owned();
    // Something MLflow holds that this workspace never made, and another workspace's experiment with the same name.
    fake.lock()
        .unwrap()
        .experiments
        .push(("900".into(), format!("{}.ai.main.hidden", l.ws), true));
    fake.lock()
        .unwrap()
        .experiments
        .push(("901".into(), "other-ws.ai.main.churn".into(), true));
    let search = s
        .post(
            &l.url("api/2.0/mlflow/experiments/search"),
            &ana(),
            json!({"max_results": 100}),
        )
        .await;
    let names: Vec<&str> = search.json()["experiments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect::<Vec<_>>()
        .into_iter()
        .map(|n| Box::leak(n.to_owned().into_boxed_str()) as &str)
        .collect();
    assert_eq!(
        names,
        ["ai.main.churn"],
        "only what the catalog knows, in this workspace"
    );
    // A guessed id that MLflow has but the id map does not is not found, for the owner too.
    mlflow_error(
        &s.get(
            &format!(
                "{}?experiment_id=900",
                l.url("api/2.0/mlflow/experiments/get")
            ),
            &ana(),
        )
        .await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );

    // Another organization's person, in their own workspace, using this experiment's id.
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
    let there = format!("/api/v1/workspaces/{theirs}/mlflow");
    mlflow_error(
        &s.get(
            &format!("{there}/api/2.0/mlflow/experiments/get?experiment_id={exp}"),
            &rita,
        )
        .await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );
    mlflow_error(
        &s.post(
            &format!("{there}/api/2.0/mlflow/runs/create"),
            &rita,
            json!({"experiment_id": exp}),
        )
        .await,
        404,
        "RESOURCE_DOES_NOT_EXIST",
    );
    // And her own workspace's gateway lists nothing of ours; ours does not accept her token.
    let none = s
        .post(
            &format!("{there}/api/2.0/mlflow/experiments/search"),
            &rita,
            json!({"max_results": 10}),
        )
        .await;
    assert_eq!(none.json(), json!({"experiments": []}));
    assert!(
        [403, 404].contains(
            &s.get(&l.url("api/2.0/mlflow/experiments/search"), &rita)
                .await
                .status
        )
    );
}

macro_rules! with_fake {
    ($name:ident) => {
        mod $name {
            use super::*;
            #[tokio::test]
            async fn sqlite() {
                let (url, fake) = start().await;
                super::$name(
                    common::server_with_mlflow(fresh_db(false).await, url).await,
                    fake,
                )
                .await;
            }
            #[tokio::test]
            #[ignore = "needs LOAMZ_TEST_POSTGRES_URL"]
            async fn postgres() {
                let (url, fake) = start().await;
                super::$name(
                    common::server_with_mlflow(fresh_db(true).await, url).await,
                    fake,
                )
                .await;
            }
        }
    };
}

with_fake!(the_gateway_decides_for_every_request);
with_fake!(experiments_are_not_shared_between_workspaces_or_found_by_a_guessed_id);

#[tokio::test]
async fn without_an_upstream_the_gateway_says_so() {
    let s = common::server(fresh_db(false).await).await;
    let l = lab(&s).await;
    let r = s.get(&l.url("version"), &ana()).await;
    mlflow_error(&r, 503, "TEMPORARILY_UNAVAILABLE");
}
