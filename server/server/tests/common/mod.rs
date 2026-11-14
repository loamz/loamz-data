//! A whole server in the test process: a real database, a real folder for stores, real signed tokens, and requests sent
//! straight to the router (no network). The verifier is given the test key in code; there is no other way to give it keys.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use async_trait::async_trait;
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
        directory::{Directory, NewPerson, Person},
        error::Result as KernelResult,
        new_id,
        secrets::{SecretRef, SecretStore},
        settings::{Secrets, Settings, Storage},
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
    pub client: Option<String>,
}

/// A person with no organization.
pub fn person(sub: &str) -> Who {
    Who {
        sub: sub.into(),
        orgs: vec![],
        groups: vec![],
        roles: vec![],
        acting: None,
        client: None,
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

    /// The person's token was issued to this client (an exchange for an agent), not to the web app.
    pub fn via_client(mut self, client: &str) -> Self {
        self.client = Some(client.into());
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
        "sub": who.sub, "iss": issuer, "aud": audience, "azp": who.client.as_deref().unwrap_or("loamz-data"),
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
    pub cache_control: String,
    pub etag: String,
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

/// An issuer that counts how often it was asked, for the credential cache test.
#[derive(Default)]
pub struct CountingIssuer(pub std::sync::atomic::AtomicUsize);

#[async_trait]
impl loamz_lake::grants::Issuer for CountingIssuer {
    async fn issue(
        &self,
        _volumes: &[&str],
        _write: bool,
        _ttl: std::time::Duration,
    ) -> Result<loamz_lake::grants::Credentials, loamz_lake::files::FileError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(loamz_lake::grants::Credentials {
            key_id: "k".into(),
            secret: "s".into(),
            session_token: "t".into(),
        })
    }
}

/// A vault that holds a few secrets, so a test can name them without a real `OpenBao`.
#[derive(Default)]
pub struct FakeVault {
    versions: Mutex<HashMap<String, Vec<Option<Value>>>>,
    pub fail_write: std::sync::atomic::AtomicBool,
    pub fail_destroy: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl SecretStore for FakeVault {
    async fn write_version(&self, path: &str, data: &Value) -> Result<i64, String> {
        if self.fail_write.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("CANARY_UPSTREAM_SECRET".into());
        }
        let mut all = self.versions.lock().unwrap();
        let entries = all.entry(path.to_owned()).or_default();
        entries.push(Some(data.clone()));
        Ok(i64::try_from(entries.len()).unwrap())
    }
    async fn read_version(&self, path: &str, version: i64) -> Result<Value, String> {
        let index = version
            .checked_sub(1)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("missing")?;
        self.versions
            .lock()
            .unwrap()
            .get(path)
            .and_then(|v| v.get(index))
            .and_then(Clone::clone)
            .ok_or_else(|| "missing".into())
    }
    async fn destroy_versions(&self, path: &str, versions: &[i64]) -> Result<(), String> {
        if self.fail_destroy.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("CANARY_UPSTREAM_SECRET".into());
        }
        let mut all = self.versions.lock().unwrap();
        if let Some(entries) = all.get_mut(path) {
            for version in versions {
                if let Some(index) = version.checked_sub(1).and_then(|v| usize::try_from(v).ok())
                    && let Some(entry) = entries.get_mut(index)
                {
                    *entry = None;
                }
            }
        }
        Ok(())
    }
    async fn destroy_path(&self, path: &str) -> Result<(), String> {
        if self.fail_destroy.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("CANARY_UPSTREAM_SECRET".into());
        }
        self.versions.lock().unwrap().remove(path);
        Ok(())
    }
    async fn get(&self, r: &SecretRef) -> Result<String, String> {
        if r.key == "hook" {
            return Ok("https://example.com/hook".into());
        }
        if r.key == "insecure" {
            return Ok("http://example.com/hook".into());
        }
        if r.key == "private" {
            return Ok("https://127.0.0.1/hook".into());
        }
        match r.path.as_str() {
            "loamz-data/bucket" | "loamz-data/other" => Ok("a-secret".into()),
            _ => Err(format!("OpenBao has no secret {}", r.path)),
        }
    }
}

/// A directory of people kept in memory, so a test can see what was asked of it.
#[derive(Default)]
pub struct FakeDirectory {
    pub organizations: Mutex<Vec<String>>,
    pub people: Mutex<Vec<String>>,
    pub members: Mutex<Vec<(String, String)>>,
    pub groups: Mutex<HashMap<String, Vec<String>>>,
}

impl FakeDirectory {
    pub fn set_groups(&self, subject: &str, groups: &[&str]) {
        self.groups.lock().unwrap().insert(
            subject.to_owned(),
            groups.iter().map(|group| (*group).to_owned()).collect(),
        );
    }
}

#[async_trait]
impl Directory for FakeDirectory {
    async fn create_organization(&self, alias: &str) -> KernelResult<()> {
        let mut all = self.organizations.lock().unwrap();
        if !all.iter().any(|a| a == alias) {
            all.push(alias.into());
        }
        Ok(())
    }

    async fn ensure_person(&self, person: &NewPerson, _password: &str) -> KernelResult<Person> {
        let mut all = self.people.lock().unwrap();
        let created = !all.contains(&person.email);
        if created {
            all.push(person.email.clone());
        }
        Ok(Person {
            subject: format!("kc-{}", person.email),
            created,
        })
    }

    async fn add_member(&self, alias: &str, subject: &str) -> KernelResult<()> {
        let mut all = self.members.lock().unwrap();
        let pair = (alias.to_owned(), subject.to_owned());
        if !all.contains(&pair) {
            all.push(pair);
        }
        Ok(())
    }

    async fn groups(&self, subject: &str) -> KernelResult<Vec<String>> {
        Ok(self
            .groups
            .lock()
            .unwrap()
            .get(subject)
            .cloned()
            .unwrap_or_default())
    }
}

pub struct Server {
    pub app: Shared,
    pub directory: Arc<FakeDirectory>,
    pub vault: Arc<FakeVault>,
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
    server_with_limits(db, max_upload, 32).await
}

pub async fn server_with_limits(db: Db, max_upload: u64, max_active_runs: i64) -> Server {
    build(db, max_upload, max_active_runs, true).await
}

/// A server whose directory of people is not connected.
/// A server that asks this issuer for storage credentials.
pub async fn server_with_issuer(db: Db, issuer: Arc<dyn loamz_lake::grants::Issuer>) -> Server {
    build_with(db, 1024 * 1024, 32, true, Some(issuer)).await
}

pub async fn server_without_directory(db: Db) -> Server {
    build(db, 1024 * 1024, 32, false).await
}

async fn build(db: Db, max_upload: u64, max_active_runs: i64, directory: bool) -> Server {
    build_with(db, max_upload, max_active_runs, directory, None).await
}

async fn build_with(
    db: Db,
    max_upload: u64,
    max_active_runs: i64,
    directory: bool,
    issuer: Option<Arc<dyn loamz_lake::grants::Issuer>>,
) -> Server {
    let dir = std::env::temp_dir().join(format!("loamz-data-stores-{}", new_id()));
    let settings = Settings {
        listen: "127.0.0.1:0".parse().unwrap(),
        database_url: String::new(),
        storage: Storage::Folder(dir.clone()),
        issuer: ISSUER.into(),
        storage_allow_private: true,
        storage_tls_proxy: None,
        storage_allow_http_dev: true,
        max_upload_bytes: max_upload,
        max_active_runs,
        web_dir: None,
        serverless_token: None,
        serverless_endpoint: Some("https://sql.example.test/run".into()),
        agent_clients: vec!["loamz-agents".into()],
        directory_client: "loamz-data-directory".into(),
        directory_secret: None,
        metrics_token: None,
        openbao: None,
    };
    db.migrate(&MIGRATIONS).await.unwrap();
    let fake = Arc::new(FakeDirectory::default());
    let mut app = App::new(&settings, &Secrets::default(), db).unwrap();
    let vault = Arc::new(FakeVault::default());
    app.vault = Some(vault.clone());
    app.issuer = issuer;
    app.metrics_token = Some("m".repeat(40));
    if directory {
        app.directory = Some(fake.clone());
    }
    let app = shared(app);
    app.verifier.set_keys(&keys().jwks).unwrap();
    let (door, routes) = router(app.clone());
    Server {
        app,
        directory: fake,
        vault,
        routes,
        router: door,
        dir,
    }
}

impl Server {
    pub async fn send(&self, method: &str, path: &str, token: Option<&str>, body: Body) -> Reply {
        self.send_with_header(method, path, token, body, None).await
    }

    pub async fn send_with_header(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Body,
        extra: Option<(&str, &str)>,
    ) -> Reply {
        let mut req =
            Request::builder()
                .method(method)
                .uri(path)
                .extension(axum::extract::ConnectInfo(
                    "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
                ));
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some((name, value)) = extra {
            req = req.header(name, value);
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
        let cache_control = res
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let etag = res
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        Reply {
            status,
            content_type,
            cache_control,
            etag,
            bytes,
        }
    }

    /// A request with a bearer token as given (a service principal's, say), not one minted from a [`Who`].
    pub async fn bearer(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Value>,
    ) -> Reply {
        self.send(
            method,
            path,
            Some(token),
            body.map_or_else(Body::empty, |b| Body::from(b.to_string())),
        )
        .await
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
        if kind == "table" {
            return self.table(ws, who, parent, name, "").await;
        }
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

    /// An Iceberg table with one column, made the way a screen makes one.
    pub async fn table(
        &self,
        ws: &str,
        who: &Who,
        schema: &str,
        name: &str,
        class: &str,
    ) -> String {
        let r = self
            .post(
                &format!("/api/v1/workspaces/{ws}/tables"),
                who,
                json!({"parent_id": schema, "name": name, "classification": class, "columns": [{"name": "id", "type": "long"}]}),
            )
            .await;
        assert_eq!(r.status, 201, "creating table {name}: {}", r.text());
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

    /// The folder of a table or volume in the test store: `<workspace>/<catalog>/<id>`, or just `<id>` for an old one. A folder that
    /// is gone is a path that does not exist.
    pub fn store_dir(&self, id: &str) -> PathBuf {
        fn find(dir: &std::path::Path, id: &str, depth: usize) -> Option<PathBuf> {
            for entry in std::fs::read_dir(dir).ok()?.flatten() {
                let path = entry.path();
                if path.file_name().is_some_and(|n| n == id) {
                    return Some(path);
                }
                if depth > 0
                    && path.is_dir()
                    && let Some(found) = find(&path, id, depth - 1)
                {
                    return Some(found);
                }
            }
            None
        }
        find(&self.dir, id, 2).unwrap_or_else(|| self.dir.join(id))
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
