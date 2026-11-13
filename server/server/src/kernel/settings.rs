//! Deployment settings, read once at start and checked. Everything that identifies who may do what lives in the identity
//! provider and in the database, never here: these settings say only where things are.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use loamz_lake::{
    grants::{Issuer, R2Issuer, StsIssuer},
    objects::{Bucket, Store},
};

use super::{
    directory::{Directory, Keycloak},
    secrets::{Login, OpenBao, SecretRef, SecretStore},
};

#[derive(Clone, Debug)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    /// Where the bytes of volumes and tables live.
    pub storage: Storage,
    /// The Keycloak realm that signs people in, such as `https://auth.loamz.com/realms/loamz`.
    pub issuer: String,
    pub max_upload_bytes: u64,
    /// Maximum queued and running SQL statements and jobs per organization.
    pub max_active_runs: i64,
    /// The built web app, served by this server when set.
    pub web_dir: Option<PathBuf>,
    /// The token serverless compute endpoints expect, by reference.
    pub serverless_token: Option<SecretRef>,
    /// The https address of the deployed serverless SQL endpoint (Modal), so compute can be created without typing it.
    pub serverless_endpoint: Option<String>,
    /// Keycloak clients that act for people (the agents server): a person's token issued to one is that agent acting for them.
    pub agent_clients: Vec<String>,
    /// The Keycloak service-account client the server uses to create organizations and people, and its secret by reference.
    /// Without the reference, the directory is not connected: only people already in Keycloak can sign in.
    pub directory_client: String,
    pub directory_secret: Option<SecretRef>,
    /// Where the secrets are: `OpenBao`, and how this server logs in to it. Needed when any setting names a secret.
    pub openbao: Option<OpenBaoSettings>,
}

/// Where `OpenBao` is and how the server logs in. The login is the one secret that is a mounted file, not a reference.
#[derive(Clone, Debug)]
pub struct OpenBaoSettings {
    pub addr: String,
    pub mount: String,
    pub login: LoginSettings,
}

#[derive(Clone, Debug)]
pub enum LoginSettings {
    /// A file holding a token (development).
    Token { file: PathBuf },
    /// An `AppRole`: its id, and a file holding its secret id (a systemd credential, say).
    AppRole {
        role_id: String,
        secret_id_file: PathBuf,
    },
}

/// The secrets the settings name, read once from the vault at start and kept in memory. There is no `Debug`: it must not be
/// printed by accident.
#[derive(Default)]
pub struct Secrets {
    pub bucket_key_id: Option<String>,
    pub bucket_secret: Option<String>,
    /// R2's management token.
    pub bucket_issuer_token: Option<String>,
    pub serverless_token: Option<String>,
    pub directory_secret: Option<String>,
}

impl Settings {
    /// The vault, if the settings say where it is.
    ///
    /// # Errors
    /// The login file cannot be read.
    pub fn vault(&self) -> Result<Option<Arc<dyn SecretStore>>, String> {
        let Some(b) = &self.openbao else {
            return Ok(None);
        };
        let read = |p: &PathBuf| {
            std::fs::read_to_string(p)
                .map(|v| v.trim().to_owned())
                .map_err(|e| format!("cannot read {}: {e}", p.display()))
        };
        let login = match &b.login {
            LoginSettings::Token { file } => Login::Token(read(file)?),
            LoginSettings::AppRole {
                role_id,
                secret_id_file,
            } => Login::AppRole {
                role_id: role_id.clone(),
                secret_id: read(secret_id_file)?,
            },
        };
        Ok(Some(Arc::new(OpenBao::new(&b.addr, &b.mount, login)?)))
    }

    /// Reads every secret the settings name, once.
    ///
    /// # Errors
    /// A secret is named but there is no vault, or the vault does not have it.
    pub async fn resolve(&self, vault: Option<&dyn SecretStore>) -> Result<Secrets, String> {
        let mut out = Secrets::default();
        if let Storage::Bucket {
            key_id,
            secret,
            vending,
            ..
        } = &self.storage
        {
            out.bucket_key_id = Some(fetch(vault, key_id).await?);
            out.bucket_secret = Some(fetch(vault, secret).await?);
            if let Some(Vending::R2 { token }) = vending {
                out.bucket_issuer_token = Some(fetch(vault, token).await?);
            }
        }
        if let Some(r) = &self.serverless_token {
            let t = fetch(vault, r).await?;
            if t.len() < 32 {
                return Err("the serverless token must be at least 32 characters".into());
            }
            out.serverless_token = Some(t);
        }
        if let Some(r) = &self.directory_secret {
            out.directory_secret = Some(fetch(vault, r).await?);
        }
        Ok(out)
    }

    /// The directory of people, if its secret is given.
    ///
    /// # Errors
    /// The issuer is not a Keycloak realm.
    pub fn directory(&self, secrets: &Secrets) -> Result<Option<Arc<dyn Directory>>, String> {
        let Some(secret) = &secrets.directory_secret else {
            return Ok(None);
        };
        Ok(Some(Arc::new(Keycloak::new(
            &self.issuer,
            &self.directory_client,
            secret.trim(),
        )?)))
    }
}

/// Where data bytes live: a bucket (Cloudflare R2 or any S3, so remote machines can query it) or a folder.
#[derive(Clone, Debug)]
pub enum Storage {
    Folder(PathBuf),
    /// The two secrets are references into the vault, never values.
    Bucket {
        endpoint: String,
        name: String,
        key_id: SecretRef,
        secret: SecretRef,
        /// How other machines are given short-lived access; without it, no machine but this server reads the bucket.
        vending: Option<Vending>,
    },
}

/// How a bucket gives out short-lived credentials limited to some folders.
#[derive(Clone, Debug)]
pub enum Vending {
    /// Cloudflare R2's own API, with a management token kept in the vault.
    R2 { token: SecretRef },
    /// STS `AssumeRole` with a session policy (`RustFS`, S3). `endpoint` is where STS answers: for `RustFS`, its S3 address.
    Sts {
        endpoint: String,
        region: String,
        role_arn: Option<String>,
    },
}

impl Storage {
    /// The issuer of short-lived folder credentials, if this storage can have one.
    ///
    /// # Errors
    /// The settings do not fit the store.
    pub fn issuer(&self, secrets: &Secrets) -> Result<Option<Arc<dyn Issuer>>, String> {
        let Self::Bucket {
            endpoint,
            name,
            vending: Some(vending),
            ..
        } = self
        else {
            return Ok(None);
        };
        let missing = || "a secret the bucket needs was not read".to_owned();
        let key_id = secrets.bucket_key_id.as_deref().ok_or_else(missing)?;
        Ok(Some(match vending {
            Vending::R2 { .. } => Arc::new(
                R2Issuer::new(
                    endpoint,
                    name,
                    key_id,
                    secrets.bucket_issuer_token.as_deref().ok_or_else(missing)?,
                )
                .map_err(|e| e.to_string())?,
            ),
            Vending::Sts {
                endpoint,
                region,
                role_arn,
            } => Arc::new(
                StsIssuer::new(
                    endpoint,
                    region,
                    name,
                    key_id,
                    secrets.bucket_secret.as_deref().ok_or_else(missing)?,
                    role_arn.as_deref(),
                )
                .map_err(|e| e.to_string())?,
            ),
        }))
    }

    /// # Errors
    /// The bucket settings are not usable.
    pub fn open(&self, secrets: &Secrets) -> Result<Store, String> {
        match self {
            Self::Folder(dir) => Ok(Store::folder(dir)),
            Self::Bucket { endpoint, name, .. } => {
                let missing = || "a secret the bucket needs was not read".to_owned();
                Store::bucket(&Bucket {
                    endpoint: endpoint.clone(),
                    name: name.clone(),
                    key_id: secrets.bucket_key_id.clone().ok_or_else(missing)?,
                    secret: secrets.bucket_secret.clone().ok_or_else(missing)?,
                })
                .map_err(|e| e.to_string())
            }
        }
    }
}

/// Reads one secret from the vault.
async fn fetch(vault: Option<&dyn SecretStore>, r: &SecretRef) -> Result<String, String> {
    match vault {
        Some(v) => v.get(r).await,
        None => Err(format!(
            "{r} is a secret but there is no vault: set LOAMZ_DATA_OPENBAO_ADDR"
        )),
    }
}

/// A setting that names a secret, if it is set.
fn reference(name: &str) -> Result<Option<SecretRef>, String> {
    var(name)
        .map(|v| v.parse().map_err(|e| format!("{name}: {e}")))
        .transpose()
}

/// How the bucket gives out short-lived credentials: R2's token, or an STS endpoint. At most one.
fn vending() -> Result<Option<Vending>, String> {
    match (
        reference("LOAMZ_DATA_BUCKET_ISSUER_TOKEN_REF")?,
        var("LOAMZ_DATA_BUCKET_STS_ENDPOINT"),
    ) {
        (Some(token), None) => Ok(Some(Vending::R2 { token })),
        (None, Some(endpoint)) => Ok(Some(Vending::Sts {
            endpoint,
            region: var("LOAMZ_DATA_BUCKET_STS_REGION").unwrap_or_else(|| "us-east-1".into()),
            role_arn: var("LOAMZ_DATA_BUCKET_STS_ROLE_ARN"),
        })),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(
            "set one of LOAMZ_DATA_BUCKET_ISSUER_TOKEN_REF (R2) or LOAMZ_DATA_BUCKET_STS_ENDPOINT (STS), not both".into(),
        ),
    }
}

/// Where `OpenBao` is, and how the server logs in: a token file, or an `AppRole`.
fn openbao() -> Result<Option<OpenBaoSettings>, String> {
    let Some(addr) = var("LOAMZ_DATA_OPENBAO_ADDR") else {
        return Ok(None);
    };
    if !valid_issuer(&addr) {
        return Err(
            "LOAMZ_DATA_OPENBAO_ADDR must be an https URL, or an http URL on localhost".into(),
        );
    }
    let login = match (
        var("LOAMZ_DATA_OPENBAO_TOKEN_FILE"),
        var("LOAMZ_DATA_OPENBAO_ROLE_ID"),
        var("LOAMZ_DATA_OPENBAO_SECRET_ID_FILE"),
    ) {
        (Some(file), None, None) => LoginSettings::Token { file: file.into() },
        (None, Some(role_id), Some(file)) => LoginSettings::AppRole {
            role_id,
            secret_id_file: file.into(),
        },
        _ => {
            return Err("set LOAMZ_DATA_OPENBAO_TOKEN_FILE, or LOAMZ_DATA_OPENBAO_ROLE_ID with LOAMZ_DATA_OPENBAO_SECRET_ID_FILE".into());
        }
    };
    Ok(Some(OpenBaoSettings {
        addr,
        mount: var("LOAMZ_DATA_OPENBAO_MOUNT").unwrap_or_else(|| "secret".into()),
        login,
    }))
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// HTTP is allowed only for an exact local development host, never a look-alike hostname.
fn valid_issuer(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    matches!(
        (url.scheme(), url.host_str()),
        ("https", Some(_)) | ("http", Some("localhost" | "127.0.0.1" | "[::1]"))
    )
}

impl Settings {
    /// # Errors
    /// A required setting is missing or malformed.
    pub fn from_env() -> Result<Self, String> {
        let need = |name: &str| var(name).ok_or_else(|| format!("{name} is not set"));
        let issuer = need("LOAMZ_DATA_OIDC_ISSUER")?;
        if !valid_issuer(&issuer) {
            return Err(
                "LOAMZ_DATA_OIDC_ISSUER must be an https URL, or an http URL on localhost".into(),
            );
        }
        let max_mb = match var("LOAMZ_DATA_MAX_UPLOAD_MB") {
            Some(v) => v
                .parse::<u64>()
                .map_err(|_| "LOAMZ_DATA_MAX_UPLOAD_MB is not a number")?,
            None => 1024,
        };
        let max_active_runs = var("LOAMZ_DATA_MAX_ACTIVE_RUNS")
            .map_or(Ok(32), |v| v.parse::<i64>())
            .map_err(|_| "LOAMZ_DATA_MAX_ACTIVE_RUNS must be a positive integer")?;
        if max_active_runs < 1 {
            return Err("LOAMZ_DATA_MAX_ACTIVE_RUNS must be a positive integer".into());
        }
        Ok(Self {
            listen: var("LOAMZ_DATA_LISTEN")
                .unwrap_or_else(|| "127.0.0.1:8020".into())
                .parse()
                .map_err(|_| "LOAMZ_DATA_LISTEN is not an address like 127.0.0.1:8020")?,
            database_url: need("LOAMZ_DATA_DATABASE_URL")?,
            storage: match (var("LOAMZ_DATA_STORES_DIR"), var("LOAMZ_DATA_BUCKET")) {
                (Some(dir), None) => Storage::Folder(dir.into()),
                (None, Some(name)) => Storage::Bucket {
                    endpoint: need("LOAMZ_DATA_BUCKET_ENDPOINT")?,
                    name,
                    key_id: need("LOAMZ_DATA_BUCKET_KEY_ID_REF")?
                        .parse()
                        .map_err(|e| format!("LOAMZ_DATA_BUCKET_KEY_ID_REF: {e}"))?,
                    secret: need("LOAMZ_DATA_BUCKET_SECRET_REF")?
                        .parse()
                        .map_err(|e| format!("LOAMZ_DATA_BUCKET_SECRET_REF: {e}"))?,
                    vending: vending()?,
                },
                _ => {
                    return Err(
                        "set exactly one of LOAMZ_DATA_STORES_DIR (a folder) or LOAMZ_DATA_BUCKET (a bucket)"
                            .into(),
                    );
                }
            },
            issuer,
            max_upload_bytes: max_mb.saturating_mul(1024 * 1024),
            max_active_runs,
            web_dir: var("LOAMZ_DATA_WEB_DIR").map(Into::into),
            serverless_token: reference("LOAMZ_DATA_SERVERLESS_TOKEN_REF")?,
            serverless_endpoint: var("LOAMZ_DATA_SERVERLESS_ENDPOINT"),
            directory_client: var("LOAMZ_DATA_DIRECTORY_CLIENT")
                .unwrap_or_else(|| "loamz-data-directory".into()),
            directory_secret: reference("LOAMZ_DATA_DIRECTORY_SECRET_REF")?,
            openbao: openbao()?,
            agent_clients: var("LOAMZ_DATA_AGENT_CLIENTS")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::valid_issuer;

    #[test]
    fn http_issuers_are_restricted_to_exact_local_hosts() {
        for value in [
            "https://identity.example/realms/loamz",
            "http://localhost:8082/realms/loamz",
            "http://127.0.0.1:8082/realms/loamz",
            "http://[::1]:8082/realms/loamz",
        ] {
            assert!(valid_issuer(value), "{value}");
        }
        for value in [
            "http://localhost.evil.example/realm",
            "http://127.0.0.1.evil.example/realm",
            "http://localhost@evil.example/realm",
            "https://user@example.com/realm",
            "https://example.com/realm?x=1",
            "https://example.com/realm#x",
            "https://",
        ] {
            assert!(!valid_issuer(value), "{value}");
        }
    }
}
