//! Short-lived storage credentials for a machine that must read (or write) some of the bucket: valid for minutes, limited to the
//! folders asked for, read-only unless writing was asked for. The bucket's own key never leaves the server.
//!
//! [`Issuer`] is the contract; each kind of store has its own way to keep it:
//! - [`R2Issuer`]: Cloudflare R2's own temporary-credentials API.
//! - [`StsIssuer`]: STS `AssumeRole` with a session policy, which `RustFS`, `MinIO`-compatible stores and AWS S3 all speak.

mod policy;
mod r2;
pub(crate) mod sigv4;
mod sts;

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::files::{FileError, check_volume};

pub use policy::session_policy;
pub use r2::R2Issuer;
pub use sts::StsIssuer;

/// What a machine is given: an S3 key pair and session token, good until they expire.
#[derive(Clone, Deserialize)]
pub struct Credentials {
    #[serde(rename = "accessKeyId")]
    pub key_id: String,
    #[serde(rename = "secretAccessKey")]
    pub secret: String,
    #[serde(rename = "sessionToken")]
    pub session_token: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials([REDACTED])")
    }
}

/// Gives out credentials limited to some volume folders.
#[async_trait]
pub trait Issuer: Send + Sync {
    /// Credentials for the given volume folders (each `<id>/`), for about `ttl`.
    ///
    /// # Errors
    /// A folder name is not allowed, or the provider refused.
    async fn issue(
        &self,
        volumes: &[&str],
        write: bool,
        ttl: Duration,
    ) -> Result<Credentials, FileError>;
}

/// The folders (`<id>/`) a request names; each must be an allowed volume id, and there must be at least one.
fn folders(volumes: &[&str]) -> Result<Vec<String>, FileError> {
    let mut out = Vec::new();
    for v in volumes {
        check_volume(v)?;
        out.push(format!("{v}/"));
    }
    if out.is_empty() {
        return Err(FileError::BadPath("no folders"));
    }
    Ok(out)
}

fn provider_error(message: impl Into<String>) -> FileError {
    FileError::Io(std::io::Error::other(message.into()))
}
