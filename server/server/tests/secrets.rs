//! Secrets are named by reference and read from the vault once. A fake vault covers the rules; `openbao_live` (ignored) runs the
//! real client against OpenBao from `deploy/dev/compose.yml`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::doc_markdown,
    clippy::too_many_lines
)]

use std::collections::HashMap;

use async_trait::async_trait;
use loamz_data_server::kernel::{
    secrets::{Login, OpenBao, SecretRef, SecretStore},
    settings::{Settings, Storage, Vending},
};
use serde_json::json;

struct Fake(HashMap<String, String>);

#[async_trait]
impl SecretStore for Fake {
    async fn get(&self, r: &SecretRef) -> Result<String, String> {
        self.0
            .get(&r.to_string())
            .cloned()
            .ok_or_else(|| format!("no secret {r}"))
    }
}

fn settings(storage: Storage) -> Settings {
    Settings {
        listen: "127.0.0.1:0".parse().unwrap(),
        database_url: String::new(),
        storage,
        issuer: "http://keycloak.test/realms/loamz".into(),
        max_upload_bytes: 1,
        max_active_runs: 1,
        web_dir: None,
        serverless_token: Some("loamz-data/serverless#token".parse().unwrap()),
        serverless_endpoint: None,
        agent_clients: vec![],
        directory_client: "loamz-data-directory".into(),
        directory_secret: Some("loamz-data/directory#secret".parse().unwrap()),
        metrics_token: None,
        openbao: None,
    }
}

fn bucket() -> Storage {
    Storage::Bucket {
        endpoint: "http://127.0.0.1:9000".into(),
        name: "loamz-data".into(),
        key_id: "loamz-data/bucket#key_id".parse().unwrap(),
        secret: "loamz-data/bucket#secret".parse().unwrap(),
        vending: Some(Vending::R2 {
            token: "loamz-data/bucket#issuer_token".parse().unwrap(),
        }),
    }
}

fn vault(serverless: &str) -> Fake {
    Fake(
        [
            ("loamz-data/bucket#key_id", "KEY"),
            ("loamz-data/bucket#secret", "SECRET"),
            ("loamz-data/bucket#issuer_token", "TOKEN"),
            ("loamz-data/serverless#token", serverless),
            ("loamz-data/directory#secret", "DIR"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
    )
}

#[tokio::test]
async fn every_named_secret_is_read_from_the_vault() {
    let long = "s".repeat(40);
    let got = settings(bucket())
        .resolve(Some(&vault(&long)))
        .await
        .unwrap();
    assert_eq!(got.bucket_key_id.as_deref(), Some("KEY"));
    assert_eq!(got.bucket_secret.as_deref(), Some("SECRET"));
    assert_eq!(got.bucket_issuer_token.as_deref(), Some("TOKEN"));
    assert_eq!(got.serverless_token.as_deref(), Some(long.as_str()));
    assert_eq!(got.directory_secret.as_deref(), Some("DIR"));
}

#[tokio::test]
async fn a_secret_with_no_vault_is_an_error_that_names_the_reference_only() {
    let Err(e) = settings(bucket()).resolve(None).await else {
        panic!("a named secret needs a vault");
    };
    assert!(
        e.contains("loamz-data/bucket#key_id") && e.contains("no vault"),
        "{e}"
    );
}

#[tokio::test]
async fn a_short_serverless_token_is_refused_without_printing_it() {
    let Err(e) = settings(bucket()).resolve(Some(&vault("short"))).await else {
        panic!("too short");
    };
    assert!(e.contains("32 characters") && !e.contains("short\""), "{e}");
}

#[tokio::test]
#[ignore = "needs LOAMZ_TEST_OPENBAO_ADDR (OpenBao from deploy/dev/compose.yml, root token loamz-dev-only)"]
async fn openbao_live() {
    let addr = std::env::var("LOAMZ_TEST_OPENBAO_ADDR").expect("set LOAMZ_TEST_OPENBAO_ADDR");
    let root = "loamz-dev-only";
    let http = reqwest::Client::new();
    let call = |method: reqwest::Method, path: &str, body: serde_json::Value| {
        let (http, url) = (http.clone(), format!("{addr}/v1/{path}"));
        async move {
            let r = http
                .request(method, url)
                .header("X-Vault-Token", root)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert!(
                r.status().is_success() || r.status().as_u16() == 400,
                "{}",
                r.status()
            );
            r.text().await.unwrap()
        }
    };
    let name = format!("test-{}", std::process::id());
    call(
        reqwest::Method::POST,
        &format!("secret/data/{name}"),
        json!({"data": {"key": "the-value"}}),
    )
    .await;

    // The dev token reads it; a missing secret or key is an error that names them, not a value.
    let by_token = OpenBao::new(&addr, "secret", Login::Token(root.into())).unwrap();
    let r: SecretRef = format!("{name}#key").parse().unwrap();
    assert_eq!(by_token.get(&r).await.unwrap(), "the-value");
    let missing: SecretRef = format!("{name}#nope").parse().unwrap();
    assert!(
        by_token
            .get(&missing)
            .await
            .unwrap_err()
            .contains("no key nope")
    );
    let absent: SecretRef = "no/such/secret#key".parse().unwrap();
    assert!(
        by_token
            .get(&absent)
            .await
            .unwrap_err()
            .contains("no secret")
    );

    // An AppRole that may read only its own path: the production login.
    call(
        reqwest::Method::POST,
        "sys/auth/approle",
        json!({"type": "approle"}),
    )
    .await;
    call(
        reqwest::Method::PUT,
        &format!("sys/policies/acl/{name}"),
        json!({"policy": format!("path \"secret/data/{name}\" {{ capabilities = [\"read\"] }}")}),
    )
    .await;
    call(
        reqwest::Method::POST,
        &format!("auth/approle/role/{name}"),
        json!({"token_policies": [name], "token_ttl": "1h"}),
    )
    .await;
    let role_id: serde_json::Value = serde_json::from_str(
        &call(
            reqwest::Method::GET,
            &format!("auth/approle/role/{name}/role-id"),
            json!({}),
        )
        .await,
    )
    .unwrap();
    let secret_id: serde_json::Value = serde_json::from_str(
        &call(
            reqwest::Method::POST,
            &format!("auth/approle/role/{name}/secret-id"),
            json!({}),
        )
        .await,
    )
    .unwrap();
    let by_role = OpenBao::new(
        &addr,
        "secret",
        Login::AppRole {
            role_id: role_id["data"]["role_id"].as_str().unwrap().into(),
            secret_id: secret_id["data"]["secret_id"].as_str().unwrap().into(),
        },
    )
    .unwrap();
    assert_eq!(by_role.get(&r).await.unwrap(), "the-value");
    let other: SecretRef = "loamz-data/bucket#secret".parse().unwrap();
    assert!(
        by_role
            .get(&other)
            .await
            .unwrap_err()
            .contains("does not let"),
        "deny by default: the role reads only its own path"
    );
}
