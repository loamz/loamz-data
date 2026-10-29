//! A whole server in the test process: a real database, a real folder for volumes, real signed tokens, and requests
//! sent straight to the router (no network).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::{collections::HashSet, path::PathBuf, sync::OnceLock};

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
        settings::{KeySource, Settings},
        state::{App, Shared},
    },
    router, shared,
};
use loamz_lake::sql::Db;
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
use serde_json::{Value, json};
use tower::ServiceExt;

pub const ISSUER: &str = "http://keycloak.test/realms/loamz";
pub const AUDIENCE: &str = "loamz-data";

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
pub struct Who<'a> {
    pub sub: &'a str,
    pub org: Option<&'a str>,
    pub acting: Option<&'a str>,
}

pub fn person(sub: &str) -> Who<'_> {
    Who {
        sub,
        org: None,
        acting: None,
    }
}

pub fn member<'a>(sub: &'a str, org: &'a str) -> Who<'a> {
    Who {
        sub,
        org: Some(org),
        acting: None,
    }
}

fn mint_with(who: &Who, pem: &str, audience: &str) -> String {
    let mut claims = json!({
        "sub": who.sub, "iss": ISSUER, "aud": audience,
        "exp": loamz_data_server::kernel::now_ms() / 1000 + 3600,
    });
    if let Some(org) = who.org {
        claims["organization"] = json!({ org: {} });
    }
    if let Some(agent) = who.acting {
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
    mint_with(who, &keys().pem, AUDIENCE)
}

pub fn forged(who: &Who) -> String {
    mint_with(who, &keys().other_pem, AUDIENCE)
}

pub fn wrong_audience(who: &Who) -> String {
    mint_with(who, &keys().pem, "someone-else")
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
    let dir = std::env::temp_dir().join(format!("loamz-data-volumes-{}", new_id()));
    let jwks = std::env::temp_dir().join(format!("loamz-data-jwks-{}.json", new_id()));
    std::fs::write(&jwks, &keys().jwks).unwrap();
    let settings = Settings {
        listen: "127.0.0.1:0".parse().unwrap(),
        database_url: String::new(),
        volumes_dir: dir.clone(),
        issuer: ISSUER.into(),
        audience: AUDIENCE.into(),
        keys: KeySource::File(jwks),
        // "ana" owns the organization "acme"'s whole catalog space.
        admins: HashSet::from(["ana".to_owned()]),
        max_upload_bytes: max_upload,
    };
    db.migrate(&MIGRATIONS).await.unwrap();
    let app = shared(App::new(&settings, db));
    app.verifier.refresh().await.unwrap();
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

    pub async fn call(
        &self,
        method: &str,
        path: &str,
        who: &Who<'_>,
        body: Option<Value>,
    ) -> Reply {
        self.send(
            method,
            path,
            Some(&token(who)),
            body.map_or_else(Body::empty, |b| Body::from(b.to_string())),
        )
        .await
    }

    pub async fn get(&self, path: &str, who: &Who<'_>) -> Reply {
        self.call("GET", path, who, None).await
    }

    pub async fn post(&self, path: &str, who: &Who<'_>, body: Value) -> Reply {
        self.call("POST", path, who, Some(body)).await
    }

    pub async fn delete(&self, path: &str, who: &Who<'_>) -> Reply {
        self.call("DELETE", path, who, None).await
    }

    pub async fn upload(&self, volume: &str, path: &str, who: &Who<'_>, bytes: &[u8]) -> Reply {
        self.send(
            "PUT",
            &format!("/api/v1/volumes/{volume}/files/{path}"),
            Some(&token(who)),
            Body::from(bytes.to_vec()),
        )
        .await
    }

    pub async fn download(&self, volume: &str, path: &str, who: &Who<'_>) -> Reply {
        self.get(&format!("/api/v1/volumes/{volume}/files/{path}"), who)
            .await
    }

    /// Creates something and returns its id, failing the test if it is refused.
    pub async fn make(&self, who: &Who<'_>, kind: &str, parent: &str, name: &str) -> String {
        let r = self
            .post(
                "/api/v1/securables",
                who,
                json!({"kind": kind, "parent_id": parent, "name": name}),
            )
            .await;
        assert_eq!(
            r.status,
            201,
            "creating {kind} {name}: {}",
            String::from_utf8_lossy(&r.bytes)
        );
        r.json()["id"].as_str().unwrap().to_owned()
    }

    pub async fn grant(&self, who: &Who<'_>, on: &str, principal: &str, role: &str) {
        let r = self
            .call(
                "PUT",
                &format!("/api/v1/securables/{on}/grants"),
                who,
                Some(json!({"principal": principal, "role": role})),
            )
            .await;
        assert_eq!(
            r.status,
            204,
            "granting {role} to {principal}: {}",
            String::from_utf8_lossy(&r.bytes)
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

    pub fn volume_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
