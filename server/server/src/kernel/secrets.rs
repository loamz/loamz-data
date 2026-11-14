//! Secrets. Every credential the server holds lives in `OpenBao`, one place that runs apart from the server; settings name a secret
//! by reference (`path#key`), never by value. The server reads each one once at start and keeps it in memory only.
//!
//! [`SecretStore`] is the contract. The one implementation is [`OpenBao`], over its HTTP API (`KV` version 2). The server's own login
//! to `OpenBao` is the one secret that cannot be in `OpenBao`: a token (development) or an `AppRole` (`role_id` and a `secret_id` file,
//! such as a systemd credential).

use std::{
    str::FromStr,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Mutex;

/// Where a secret is: a path in the vault and a key in it, written `loamz-data/bucket#secret`. Never the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretRef {
    pub path: String,
    pub key: String,
}

impl FromStr for SecretRef {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let plain = |t: &str| {
            !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
                && !t.split('/').any(|p| p.is_empty() || p == "." || p == "..")
        };
        match s.split_once('#') {
            Some((path, key)) if plain(path) && plain(key) && !key.contains('/') => Ok(Self {
                path: path.to_owned(),
                key: key.to_owned(),
            }),
            _ => Err(format!(
                "`{s}` is not a secret reference like loamz-data/bucket#secret"
            )),
        }
    }
}

impl std::fmt::Display for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.path, self.key)
    }
}

#[async_trait]
pub trait SecretStore: Send + Sync {
    /// The value a reference names.
    ///
    /// # Errors
    /// The store could not be reached, refused, or has no such secret. The message names the reference, never a value.
    async fn get(&self, r: &SecretRef) -> Result<String, String>;

    /// Write one version of a server-generated credential path.
    async fn write_version(&self, _path: &str, _data: &Value) -> Result<i64, String> {
        Err("versioned secrets unavailable".into())
    }

    /// Read exactly the version pinned in the database.
    async fn read_version(&self, _path: &str, _version: i64) -> Result<Value, String> {
        Err("versioned secrets unavailable".into())
    }

    /// Permanently destroy specific obsolete versions.
    async fn destroy_versions(&self, _path: &str, _versions: &[i64]) -> Result<(), String> {
        Err("versioned secrets unavailable".into())
    }

    /// Permanently delete the generated path and all of its versions.
    async fn destroy_path(&self, _path: &str) -> Result<(), String> {
        Err("versioned secrets unavailable".into())
    }
}

fn versioned_data(body: &Value, expected: i64) -> Result<Value, String> {
    if body["data"]["metadata"]["version"].as_i64() != Some(expected) {
        return Err("invalid vault response".into());
    }
    body.get("data")
        .and_then(|v| v.get("data"))
        .cloned()
        .ok_or_else(|| "invalid vault response".into())
}

fn vault_token_header(token: &str) -> Result<reqwest::header::HeaderValue, String> {
    let mut header =
        reqwest::header::HeaderValue::from_str(token).map_err(|_| "invalid vault token")?;
    header.set_sensitive(true);
    Ok(header)
}

/// How the server proves who it is to `OpenBao`.
#[derive(Clone)]
pub enum Login {
    /// A token, used as it is (development: the dev server's root token).
    Token(String),
    /// An `AppRole`: logs in for a short-lived token, and again when it runs out.
    AppRole { role_id: String, secret_id: String },
}

pub struct OpenBao {
    http: reqwest::Client,
    addr: String,
    mount: String,
    login: Login,
    token: Mutex<Option<(String, Instant)>>,
}

impl OpenBao {
    /// `addr` is `OpenBao`'s address (https, or http on this computer); `mount` is where its `KV` version 2 engine is (`secret`).
    ///
    /// # Errors
    /// The HTTP client could not be made.
    pub fn new(addr: &str, mount: &str, login: Login) -> Result<Self, String> {
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| e.to_string())?,
            addr: addr.trim_end_matches('/').to_owned(),
            mount: mount.trim_matches('/').to_owned(),
            login,
            token: Mutex::new(None),
        })
    }

    async fn bearer(&self) -> Result<String, String> {
        let (role_id, secret_id) = match &self.login {
            Login::Token(t) => return Ok(t.clone()),
            Login::AppRole { role_id, secret_id } => (role_id, secret_id),
        };
        let mut held = self.token.lock().await;
        if let Some((t, until)) = &*held
            && Instant::now() < *until
        {
            return Ok(t.clone());
        }
        let reply = self
            .http
            .post(format!("{}/v1/auth/approle/login", self.addr))
            .json(&json!({"role_id": role_id, "secret_id": secret_id}))
            .send()
            .await
            .map_err(|e| format!("OpenBao is not reachable: {e}"))?;
        if !reply.status().is_success() {
            return Err(format!(
                "OpenBao refused the AppRole login ({})",
                reply.status().as_u16()
            ));
        }
        let body: Value = reply.json().await.map_err(|e| e.to_string())?;
        let token = body["auth"]["client_token"]
            .as_str()
            .ok_or("OpenBao's login answer has no token")?
            .to_owned();
        let lease = body["auth"]["lease_duration"].as_u64().unwrap_or(60);
        *held = Some((
            token.clone(),
            Instant::now() + Duration::from_secs(lease.saturating_sub(10)),
        ));
        Ok(token)
    }
}

#[async_trait]
impl SecretStore for OpenBao {
    async fn write_version(&self, path: &str, data: &Value) -> Result<i64, String> {
        let response = self
            .http
            .post(format!("{}/v1/{}/data/{}", self.addr, self.mount, path))
            .header(
                "X-Vault-Token",
                vault_token_header(&self.bearer().await.map_err(|_| "vault unavailable")?)?,
            )
            .json(&json!({"data": data}))
            .send()
            .await
            .map_err(|_| "vault unavailable")?;
        if !response.status().is_success() {
            return Err("vault write failed".into());
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| "invalid vault response")?;
        body["data"]["version"]
            .as_i64()
            .filter(|v| *v > 0)
            .ok_or_else(|| "invalid vault response".into())
    }

    async fn read_version(&self, path: &str, version: i64) -> Result<Value, String> {
        let response = self
            .http
            .get(format!("{}/v1/{}/data/{}", self.addr, self.mount, path))
            .query(&[("version", version)])
            .header(
                "X-Vault-Token",
                vault_token_header(&self.bearer().await.map_err(|_| "vault unavailable")?)?,
            )
            .send()
            .await
            .map_err(|_| "vault unavailable")?;
        if !response.status().is_success() {
            return Err("vault read failed".into());
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| "invalid vault response")?;
        versioned_data(&body, version)
    }

    async fn destroy_versions(&self, path: &str, versions: &[i64]) -> Result<(), String> {
        if versions.is_empty() {
            return Ok(());
        }
        let response = self
            .http
            .post(format!("{}/v1/{}/destroy/{}", self.addr, self.mount, path))
            .header(
                "X-Vault-Token",
                vault_token_header(&self.bearer().await.map_err(|_| "vault unavailable")?)?,
            )
            .json(&json!({"versions": versions}))
            .send()
            .await
            .map_err(|_| "vault unavailable")?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err("vault destroy failed".into())
        }
    }

    async fn destroy_path(&self, path: &str) -> Result<(), String> {
        let response = self
            .http
            .delete(format!("{}/v1/{}/metadata/{}", self.addr, self.mount, path))
            .header(
                "X-Vault-Token",
                vault_token_header(&self.bearer().await.map_err(|_| "vault unavailable")?)?,
            )
            .send()
            .await
            .map_err(|_| "vault unavailable")?;
        if response.status().is_success() || response.status().as_u16() == 404 {
            Ok(())
        } else {
            Err("vault delete failed".into())
        }
    }

    async fn get(&self, r: &SecretRef) -> Result<String, String> {
        let reply = self
            .http
            .get(format!("{}/v1/{}/data/{}", self.addr, self.mount, r.path))
            .header("X-Vault-Token", vault_token_header(&self.bearer().await?)?)
            .send()
            .await
            .map_err(|e| format!("OpenBao is not reachable: {e}"))?;
        match reply.status().as_u16() {
            200 => {}
            404 => return Err(format!("OpenBao has no secret {}", r.path)),
            403 => return Err(format!("OpenBao does not let this server read {}", r.path)),
            s => return Err(format!("OpenBao answered {s} for {}", r.path)),
        }
        let body: Value = reply.json().await.map_err(|e| e.to_string())?;
        body["data"]["data"][r.key.as_str()]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("the secret {} has no key {}", r.path, r.key))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn versioned_reads_reject_a_mismatched_vault_version() {
        let body = json!({"data":{"data":{"key":"canary"}, "metadata":{"version":2}}});
        assert!(versioned_data(&body, 1).is_err());
        assert_eq!(versioned_data(&body, 2).unwrap()["key"], "canary");
    }

    #[test]
    fn vault_token_header_is_sensitive() {
        assert!(vault_token_header("canary-token").unwrap().is_sensitive());
    }

    #[test]
    fn a_reference_is_a_path_and_a_key() {
        let r: SecretRef = "loamz-data/bucket#secret".parse().unwrap();
        assert_eq!(
            (r.path.as_str(), r.key.as_str()),
            ("loamz-data/bucket", "secret")
        );
        assert_eq!(r.to_string(), "loamz-data/bucket#secret");
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            "", "nokey", "#key", "path#", "a/../b#k", "/a#k", "a//b#k", "a#k/x", "a b#k", "a?x=1#k",
        ] {
            assert!(bad.parse::<SecretRef>().is_err(), "{bad:?}");
        }
    }
}
