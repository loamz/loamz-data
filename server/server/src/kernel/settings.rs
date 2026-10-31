//! Deployment settings, read once at start and checked. Everything that identifies who may do what lives in the identity
//! provider and in the database, never here: these settings say only where things are.

use std::{net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    /// Where the bytes of volumes and tables live (a local folder, or any folder the infrastructure has mounted).
    pub stores_dir: PathBuf,
    /// The Keycloak realm that signs people in, such as `https://auth.loamz.com/realms/loamz`.
    pub issuer: String,
    pub max_upload_bytes: u64,
    /// The built web app, served by this server when set.
    pub web_dir: Option<PathBuf>,
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
            stores_dir: need("LOAMZ_DATA_STORES_DIR")?.into(),
            issuer,
            max_upload_bytes: max_mb.saturating_mul(1024 * 1024),
            web_dir: var("LOAMZ_DATA_WEB_DIR").map(Into::into),
        })
    }
}
