//! Deployment settings, read once at start and checked. Everything that identifies who may do what lives in the identity
//! provider and in the database, never here: these settings say only where things are.

use std::{net::SocketAddr, path::PathBuf};

use loamz_lake::{
    grants::Issuer,
    objects::{Bucket, Store},
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
    /// The built web app, served by this server when set.
    pub web_dir: Option<PathBuf>,
    /// A mounted file holding the token serverless compute endpoints expect.
    pub serverless_token_file: Option<PathBuf>,
    /// The https address of the deployed serverless SQL endpoint (Modal), so compute can be created without typing it.
    pub serverless_endpoint: Option<String>,
    /// Keycloak clients that act for people (the agents server): a person's token issued to one is that agent acting for them.
    pub agent_clients: Vec<String>,
}

impl Settings {
    /// # Errors
    /// The file cannot be read or is empty.
    pub fn serverless_token(&self) -> Result<Option<String>, String> {
        let Some(p) = &self.serverless_token_file else {
            return Ok(None);
        };
        let t =
            std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        let t = t.trim().to_string();
        if t.len() < 32 {
            return Err("the serverless token must be at least 32 characters".into());
        }
        Ok(Some(t))
    }
}

/// Where data bytes live: a bucket (Cloudflare R2 or any S3, so remote machines can query it) or a folder.
#[derive(Clone, Debug)]
pub enum Storage {
    Folder(PathBuf),
    /// The two secrets are paths of mounted files, never values.
    Bucket {
        endpoint: String,
        name: String,
        key_id_file: PathBuf,
        secret_file: PathBuf,
        /// A Cloudflare token that can issue short-lived credentials limited to some folders; without it, no machine but this server reads the bucket.
        issuer_token_file: Option<PathBuf>,
    },
}

impl Storage {
    /// The issuer of short-lived folder credentials, if this storage can have one.
    ///
    /// # Errors
    /// A file cannot be read, or the bucket is not an R2 address.
    pub fn issuer(&self) -> Result<Option<Issuer>, String> {
        let Self::Bucket {
            endpoint,
            name,
            key_id_file,
            issuer_token_file: Some(token),
            ..
        } = self
        else {
            return Ok(None);
        };
        let read = |p: &PathBuf| {
            std::fs::read_to_string(p)
                .map(|v| v.trim().to_string())
                .map_err(|e| format!("cannot read {}: {e}", p.display()))
        };
        Issuer::new(endpoint, name, &read(key_id_file)?, &read(token)?)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    /// # Errors
    /// The secret files cannot be read, or the bucket settings are not usable.
    pub fn open(&self) -> Result<Store, String> {
        match self {
            Self::Folder(dir) => Ok(Store::folder(dir)),
            Self::Bucket {
                endpoint,
                name,
                key_id_file,
                secret_file,
                ..
            } => {
                let read = |p: &PathBuf| {
                    std::fs::read_to_string(p)
                        .map(|v| v.trim().to_string())
                        .map_err(|e| format!("cannot read {}: {e}", p.display()))
                };
                Store::bucket(&Bucket {
                    endpoint: endpoint.clone(),
                    name: name.clone(),
                    key_id: read(key_id_file)?,
                    secret: read(secret_file)?,
                })
                .map_err(|e| e.to_string())
            }
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

impl Settings {
    /// # Errors
    /// A required setting is missing or malformed.
    pub fn from_env() -> Result<Self, String> {
        let need = |name: &str| var(name).ok_or_else(|| format!("{name} is not set"));
        let issuer = need("LOAMZ_DATA_OIDC_ISSUER")?;
        if !(issuer.starts_with("https://")
            || issuer.starts_with("http://127.0.0.1")
            || issuer.starts_with("http://localhost"))
        {
            return Err("LOAMZ_DATA_OIDC_ISSUER must be an https address".into());
        }
        let max_mb = match var("LOAMZ_DATA_MAX_UPLOAD_MB") {
            Some(v) => v
                .parse::<u64>()
                .map_err(|_| "LOAMZ_DATA_MAX_UPLOAD_MB is not a number")?,
            None => 1024,
        };
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
                    key_id_file: need("LOAMZ_DATA_BUCKET_KEY_ID_FILE")?.into(),
                    secret_file: need("LOAMZ_DATA_BUCKET_SECRET_FILE")?.into(),
                    issuer_token_file: var("LOAMZ_DATA_BUCKET_ISSUER_TOKEN_FILE").map(Into::into),
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
            web_dir: var("LOAMZ_DATA_WEB_DIR").map(Into::into),
            serverless_token_file: var("LOAMZ_DATA_SERVERLESS_TOKEN_FILE").map(Into::into),
            serverless_endpoint: var("LOAMZ_DATA_SERVERLESS_ENDPOINT"),
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
