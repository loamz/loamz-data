//! Settings come from the environment (`LOAMZ_DATA_*`). Nothing is guessed about identity: the issuer and the keys
//! must be given, because a wrong default there would let the wrong people in.

use std::{collections::HashSet, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug)]
pub enum KeySource {
    Url(String),
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    pub volumes_dir: PathBuf,
    pub issuer: String,
    pub audience: String,
    pub keys: KeySource,
    /// People who own an organization's whole catalog space (a personal account always owns its own).
    pub admins: HashSet<String>,
    pub max_upload_bytes: u64,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

impl Settings {
    /// # Errors
    /// A required setting is missing or malformed.
    pub fn from_env() -> Result<Self, String> {
        let need = |name: &str| var(name).ok_or_else(|| format!("{name} is not set"));
        let keys = match (var("LOAMZ_DATA_JWKS_URL"), var("LOAMZ_DATA_JWKS_FILE")) {
            (Some(url), None) => KeySource::Url(url),
            (None, Some(file)) => KeySource::File(file.into()),
            (None, None) => {
                return Err(
                    "set LOAMZ_DATA_JWKS_URL (or LOAMZ_DATA_JWKS_FILE for development)".into(),
                );
            }
            (Some(_), Some(_)) => {
                return Err("set only one of LOAMZ_DATA_JWKS_URL and LOAMZ_DATA_JWKS_FILE".into());
            }
        };
        Ok(Self {
            listen: var("LOAMZ_DATA_LISTEN")
                .unwrap_or_else(|| "127.0.0.1:8020".into())
                .parse()
                .map_err(|_| "LOAMZ_DATA_LISTEN is not an address like 127.0.0.1:8020")?,
            database_url: var("LOAMZ_DATA_DATABASE_URL")
                .unwrap_or_else(|| "sqlite:loamz-data.db?mode=rwc".into()),
            volumes_dir: var("LOAMZ_DATA_VOLUMES_DIR")
                .unwrap_or_else(|| "volumes".into())
                .into(),
            issuer: need("LOAMZ_DATA_OIDC_ISSUER")?,
            audience: var("LOAMZ_DATA_OIDC_AUDIENCE").unwrap_or_else(|| "loamz-data".into()),
            keys,
            admins: var("LOAMZ_DATA_ADMINS")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            max_upload_bytes: var("LOAMZ_DATA_MAX_UPLOAD_MB")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1024)
                .saturating_mul(1024 * 1024),
        })
    }
}
