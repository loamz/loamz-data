//! The token exchange the server does for the browser: only two kinds of request pass, always as our client, and the
//! identity provider's own answer comes back. The identity provider here is a small recorder, so the test can see what was sent.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};

use axum::{Router, extract::Form, http::StatusCode, routing::post};
use common::{fresh_db, server};
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Vec<(String, String)>>>>;

/// A token endpoint that answers `status`/`body` and remembers every form it was sent.
async fn upstream(status: u16, body: &'static str) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let record = seen.clone();
    let app = Router::new().route(
        "/token",
        post(move |Form(f): Form<Vec<(String, String)>>| {
            record.lock().unwrap().push(f);
            async move { (StatusCode::from_u16(status).unwrap(), body) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

async fn ask(s: &common::Server, body: Value) -> common::Reply {
    s.send(
        "POST",
        "/api/v1/auth/token",
        None,
        axum::body::Body::from(body.to_string()),
    )
    .await
}

fn field<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[tokio::test]
async fn a_code_is_exchanged_as_our_client_and_nothing_else_is_added() {
    let (url, seen) = upstream(
        200,
        r#"{"access_token":"a","refresh_token":"r","expires_in":300}"#,
    )
    .await;
    let s = server(fresh_db(false).await).await;
    s.app.verifier.set_token_endpoint(&url);
    // The caller tries to name another client and add its own fields: neither gets through.
    let r = ask(&s, json!({"grant_type": "authorization_code", "code": "c", "redirect_uri": "http://x/relay", "code_verifier": "v", "client_id": "loamz-chat", "scope": "admin"})).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["access_token"], "a");
    let sent = seen.lock().unwrap();
    let form = &sent[0];
    assert_eq!(field(form, "client_id"), Some("loamz-data"));
    assert_eq!(field(form, "grant_type"), Some("authorization_code"));
    assert_eq!(
        (
            field(form, "code"),
            field(form, "redirect_uri"),
            field(form, "code_verifier")
        ),
        (Some("c"), Some("http://x/relay"), Some("v"))
    );
    assert_eq!(form.len(), 5, "only what sign-in needs: {form:?}");
}

#[tokio::test]
async fn a_session_is_renewed_the_same_way() {
    let (url, seen) = upstream(200, r#"{"access_token":"b"}"#).await;
    let s = server(fresh_db(false).await).await;
    s.app.verifier.set_token_endpoint(&url);
    assert_eq!(
        ask(
            &s,
            json!({"grant_type": "refresh_token", "refresh_token": "rt"})
        )
        .await
        .status,
        200
    );
    let sent = seen.lock().unwrap();
    assert_eq!(
        (
            field(&sent[0], "grant_type"),
            field(&sent[0], "refresh_token"),
            field(&sent[0], "client_id")
        ),
        (Some("refresh_token"), Some("rt"), Some("loamz-data"))
    );
}

#[tokio::test]
async fn nothing_else_can_be_asked_for() {
    let (url, seen) = upstream(200, "{}").await;
    let s = server(fresh_db(false).await).await;
    s.app.verifier.set_token_endpoint(&url);
    for body in [
        json!({"grant_type": "password", "code": "x"}),
        json!({"grant_type": "client_credentials"}),
        json!({"grant_type": "authorization_code", "code": "c", "redirect_uri": "http://x"}),
        json!({"grant_type": "authorization_code", "code": "c", "code_verifier": "v"}),
        json!({"grant_type": "refresh_token"}),
        json!({"grant_type": "refresh_token", "refresh_token": "x".repeat(5000)}),
        json!({}),
    ] {
        let r = ask(&s, body.clone()).await;
        assert_eq!(r.status, 400, "{body}");
        assert_eq!(r.content_type, "application/problem+json");
    }
    assert!(
        seen.lock().unwrap().is_empty(),
        "none of those reached the identity provider"
    );
}

#[tokio::test]
async fn a_refusal_comes_back_as_it_was_and_a_dead_provider_says_so() {
    let (url, _) = upstream(400, r#"{"error":"invalid_grant"}"#).await;
    let s = server(fresh_db(false).await).await;
    s.app.verifier.set_token_endpoint(&url);
    let r = ask(
        &s,
        json!({"grant_type": "refresh_token", "refresh_token": "old"}),
    )
    .await;
    assert_eq!(
        (r.status, r.json()["error"].clone()),
        (400, json!("invalid_grant"))
    );

    let (url, _) = upstream(500, "boom").await;
    s.app.verifier.set_token_endpoint(&url);
    assert_eq!(
        ask(
            &s,
            json!({"grant_type": "refresh_token", "refresh_token": "x"})
        )
        .await
        .status,
        502,
        "a provider fault is not passed on as ours"
    );

    s.app
        .verifier
        .set_token_endpoint("http://127.0.0.1:1/token");
    let r = ask(
        &s,
        json!({"grant_type": "refresh_token", "refresh_token": "x"}),
    )
    .await;
    assert_eq!(r.status, 503);
    assert!(
        !r.text().contains("127.0.0.1"),
        "the address is not shown to the caller"
    );
}
