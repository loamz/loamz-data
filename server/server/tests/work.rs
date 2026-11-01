//! What people save in a workspace, through the router.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Server, ana, bob, fresh_db, server};
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

fn names(r: &common::Reply) -> Vec<String> {
    r.json()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q: &Value| q["name"].as_str().unwrap().to_owned())
        .collect()
}

async fn saved_queries_are_private_to_their_writer_and_open_to_admins(s: Server) {
    let ws = s.acme().await;
    s.assign(&ws, &[&bob()]).await;
    let url = format!("/api/v1/workspaces/{ws}/queries");
    let mine = s
        .post(
            &url,
            &ana(),
            json!({"name": "daily sales", "sql": "SELECT 1"}),
        )
        .await;
    assert_eq!(mine.status, 201);
    let id = mine.json()["id"].as_str().unwrap().to_owned();
    s.post(&url, &bob(), json!({"name": "bob's", "sql": "SELECT 2"}))
        .await;

    assert_eq!(
        names(&s.get(&url, &bob()).await),
        vec!["bob's"],
        "a person sees their own"
    );
    assert_eq!(
        names(&s.get(&url, &ana()).await).len(),
        2,
        "an admin sees all"
    );
    assert_eq!(s.get(&format!("{url}/{id}"), &bob()).await.status, 404);
    assert_eq!(
        s.put(
            &format!("{url}/{id}"),
            &bob(),
            json!({"name": "x", "sql": "SELECT 3"})
        )
        .await
        .status,
        404,
        "someone else's query cannot be changed"
    );
    assert_eq!(s.delete(&format!("{url}/{id}"), &bob()).await.status, 404);

    let changed = s
        .put(
            &format!("{url}/{id}"),
            &ana(),
            json!({"name": "daily", "sql": "SELECT 4", "target_id": "t1"}),
        )
        .await;
    assert_eq!(changed.status, 200);
    assert_eq!(changed.json()["sql"], "SELECT 4");
    assert_eq!(
        s.post(&url, &ana(), json!({"name": " ", "sql": "SELECT 1"}))
            .await
            .status,
        400
    );
    assert_eq!(s.delete(&format!("{url}/{id}"), &ana()).await.status, 204);
    assert_eq!(names(&s.get(&url, &ana()).await), vec!["bob's"]);
}
both!(saved_queries_are_private_to_their_writer_and_open_to_admins);

async fn a_stranger_cannot_reach_a_workspaces_queries(s: Server) {
    let ws = s.acme().await;
    let url = format!("/api/v1/workspaces/{ws}/queries");
    assert_eq!(s.get(&url, &bob()).await.status, 404, "not a member");
}
both!(a_stranger_cannot_reach_a_workspaces_queries);
