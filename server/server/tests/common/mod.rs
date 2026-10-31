//! A whole server in the test process: a real database, a real folder for stores, real signed tokens, and requests sent
//! straight to the router (no network). The verifier is given the test key in code; there is no other way to give it keys.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::{path::PathBuf, sync::OnceLock};

use axum::{
    Router,
    body::Body,
    http::{Request, header},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use loamz_data_server::{
    MIGRATIONS,
    kernel::{
        new_id,
        settings::{Settings, Storage},
        state::{App, Shared},
    },
    router, shared,
};
use loamz_lake::sql::Db;
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
use serde_json::{Value, json};
use tower::ServiceExt;

pub const ISSUER: &str = "http://keycloak.test/realms/loamz";
pub const ACME: &str = "acme";
pub const RIVAL: &str = "rival";

struct Keys {
    pem: String,
    jwks: String,
    /// A second key the server does not trust, to make forged tokens.
    other_pem: String,
}

fn keys() -> &'static Keys {
    static KEYS: OnceLock<Keys> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let other = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = key.to_public_key();
        let jwks = json!({"keys": [{
            "kid": "k1", "kty": "RSA", "use": "sig", "alg": "RS256",
            "n": URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
        }]});
        Keys {
            pem: key
                .to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
                .unwrap()
                .to_string(),
            other_pem: other
                .to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
                .unwrap()
                .to_string(),
            jwks: jwks.to_string(),
        }
    })
}

/// Who a token says it is.
#[derive(Clone)]
pub struct Who {
    pub sub: String,
    pub orgs: Vec<(String, String)>,
    pub groups: Vec<String>,
    pub roles: Vec<String>,
    pub acting: Option<String>,
}

/// A person with no organization.
pub fn person(sub: &str) -> Who {
    Who {
        sub: sub.into(),
        orgs: vec![],
        groups: vec![],
        roles: vec![],
        acting: None,
    }
}

/// A member of the organization `acme`.
pub fn member(sub: &str) -> Who {
    Who {
        orgs: vec![(ACME.into(), "acme".into())],
        ..person(sub)
    }
}

impl Who {
    pub fn in_group(mut self, g: &str) -> Self {
        self.groups.push(g.into());
        self
    }

    pub fn with_role(mut self, r: &str) -> Self {
        self.roles.push(r.into());
        self
    }

    pub fn also_in(mut self, id: &str, alias: &str) -> Self {
        self.orgs.push((id.into(), alias.into()));
        self
    }

    pub fn acting_as(mut self, agent: &str) -> Self {
        self.acting = Some(agent.into());
        self
    }

    pub fn id(&self) -> String {
        format!("user:{}", self.sub)
    }
}

fn mint_with(who: &Who, pem: &str, audience: &str, issuer: &str) -> String {
    let mut claims = json!({
        "sub": who.sub, "iss": issuer, "aud": audience, "azp": "loamz-data",
        "exp": loamz_data_server::kernel::now_ms() / 1000 + 3600,
        "name": who.sub, "email": format!("{}@example.test", who.sub),
        "groups": who.groups,
        "resource_access": {"loamz-data": {"roles": who.roles}},
    });
    if !who.orgs.is_empty() {
        claims["organization"] = Value::Object(
            who.orgs
                .iter()
                .map(|(id, alias)| (alias.clone(), json!({"id": id})))
                .collect(),
        );
    }
    if let Some(agent) = &who.acting {
        claims["act"] = json!({"sub": agent});
    }
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("k1".into());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap(),
    )
    .unwrap()
}

pub fn token(who: &Who) -> String {
    mint_with(who, &keys().pem, "loamz-data", ISSUER)
}

pub fn forged(who: &Who) -> String {
    mint_with(who, &keys().other_pem, "loamz-data", ISSUER)
}

pub fn wrong_audience(who: &Who) -> String {
    mint_with(who, &keys().pem, "someone-else", ISSUER)
}

pub fn wrong_issuer(who: &Who) -> String {
    mint_with(
        who,
        &keys().pem,
        "loamz-data",
        "http://evil.test/realms/loamz",
    )
}

pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

pub struct Server {
    pub app: Shared,
    pub routes: Vec<(&'static str, String)>,
    router: Router,
    dir: PathBuf,
}

/// A database of its own: a temporary `SQLite` file, or a throwaway `Postgres` database.
pub async fn fresh_db(postgres: bool) -> Db {
    if postgres {
        let base = std::env::var("LOAMZ_TEST_POSTGRES_URL")
            .expect("set LOAMZ_TEST_POSTGRES_URL to run the Postgres tests");
        let admin = Db::connect(&base).await.unwrap();
        let name = format!("t_{}", new_id().replace('-', ""));
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(admin.pool())
            .await
            .unwrap();
        let (head, _) = base.rsplit_once('/').unwrap();
        Db::connect(&format!("{head}/{name}")).await.unwrap()
    } else {
        let file = std::env::temp_dir().join(format!("loamz-data-test-{}.db", new_id()));
        Db::connect(&format!("sqlite:{}?mode=rwc", file.display()))
            .await
            .unwrap()
    }
}

pub async fn server(db: Db) -> Server {
    server_with(db, 1024 * 1024).await
}

pub async fn server_with(db: Db, max_upload: u64) -> Server {
    let dir = std::env::temp_dir().join(format!("loamz-data-stores-{}", new_id()));
    let settings = Settings {
        listen: "127.0.0.1:0".parse().unwrap(),
        database_url: String::new(),
        storage: Storage::Folder(dir.clone()),
        issuer: ISSUER.into(),
        max_upload_bytes: max_upload,
        web_dir: None,
    };
    db.migrate(&MIGRATIONS).await.unwrap();
    let app = shared(App::new(&settings, db).unwrap());
    app.verifier.set_keys(&keys().jwks).unwrap();
    let (door, routes) = router(app.clone());
    Server {
        app,
        routes,
        router: door,
        dir,
    }
}

impl Server {
    pub async fn send(&self, method: &str, path: &str, token: Option<&str>, body: Body) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let res = self
            .router
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status().as_u16();
        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        Reply {
            status,
            content_type,
            bytes,
        }
    }

    pub async fn call(&self, method: &str, path: &str, who: &Who, body: Option<Value>) -> Reply {
        self.send(
            method,
            path,
            Some(&token(who)),
            body.map_or_else(Body::empty, |b| Body::from(b.to_string())),
        )
        .await
    }

    pub async fn get(&self, path: &str, who: &Who) -> Reply {
        self.call("GET", path, who, None).await
    }

    pub async fn post(&self, path: &str, who: &Who, body: Value) -> Reply {
        self.call("POST", path, who, Some(body)).await
    }

    pub async fn put(&self, path: &str, who: &Who, body: Value) -> Reply {
        self.call("PUT", path, who, Some(body)).await
    }

    pub async fn delete(&self, path: &str, who: &Who) -> Reply {
        self.call("DELETE", path, who, None).await
    }

    pub async fn upload(
        &self,
        ws: &str,
        volume: &str,
        path: &str,
        who: &Who,
        bytes: &[u8],
    ) -> Reply {
        self.send(
            "PUT",
            &format!("/api/v1/workspaces/{ws}/volumes/{volume}/files/{path}"),
            Some(&token(who)),
            Body::from(bytes.to_vec()),
        )
        .await
    }

    pub async fn download(&self, ws: &str, volume: &str, path: &str, who: &Who) -> Reply {
        self.get(
            &format!("/api/v1/workspaces/{ws}/volumes/{volume}/files/{path}"),
            who,
        )
        .await
    }

    /// Creates the Data account of `acme` (by `ana`, who holds the Keycloak role) and a workspace in it; returns the workspace id.
    pub async fn acme(&self) -> String {
        let r = self
            .post(&format!("/api/v1/orgs/{ACME}/account"), &ana(), json!({}))
            .await;
        assert_eq!(r.status, 201, "creating the account: {}", r.text());
        self.workspace(&ana(), ACME, "Main").await
    }

    pub async fn workspace(&self, who: &Who, org: &str, name: &str) -> String {
        let r = self
            .post(
                &format!("/api/v1/orgs/{org}/workspaces"),
                who,
                json!({"name": name}),
            )
            .await;
        assert_eq!(r.status, 201, "creating workspace {name}: {}", r.text());
        r.json()["id"].as_str().unwrap().to_owned()
    }

    /// Assigns people (as principals) to a workspace as users.
    pub async fn assign(&self, ws: &str, who: &[&Who]) {
        for w in who {
            let r = self
                .put(
                    &format!("/api/v1/workspaces/{ws}/members"),
                    &ana(),
                    json!({"principal": w.id(), "role": "user"}),
                )
                .await;
            assert_eq!(r.status, 204, "assigning {}: {}", w.sub, r.text());
        }
    }

    /// Creates something and returns its id, failing the test if it is refused.
    pub async fn make(&self, ws: &str, who: &Who, kind: &str, parent: &str, name: &str) -> String {
        let r = self
            .post(
                &format!("/api/v1/workspaces/{ws}/securables"),
                who,
                json!({"kind": kind, "parent_id": parent, "name": name}),
            )
            .await;
        assert_eq!(r.status, 201, "creating {kind} {name}: {}", r.text());
        r.json()["id"].as_str().unwrap().to_owned()
    }

    pub async fn grant(&self, ws: &str, who: &Who, on: &str, principal: &str, role: &str) {
        let r = self
            .put(
                &format!("/api/v1/workspaces/{ws}/securables/{on}/grants"),
                who,
                json!({"principal": principal, "role": role}),
            )
            .await;
        assert_eq!(
            r.status,
            204,
            "granting {role} to {principal}: {}",
            r.text()
        );
    }

    /// Runs queued tasks until none is left due.
    pub async fn work(&self) -> usize {
        let mut n = 0;
        while loamz_data_server::governance::lifecycle::service::run_once(
            &self.app,
            loamz_data_server::kernel::now_ms(),
        )
        .await
        .unwrap()
        {
            n += 1;
        }
        n
    }

    pub fn store_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The organization's account admin: holds the Keycloak role that lets them create the account.
pub fn ana() -> Who {
    member("ana").with_role("account-admin")
}
pub fn bob() -> Who {
    member("bob")
}
pub fn cy() -> Who {
    member("cy")
}
pub fn zed() -> Who {
    member("zed")
}
