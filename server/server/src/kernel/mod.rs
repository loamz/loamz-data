//! What every feature shares: settings, errors, who is calling, the one permission function, and the app state.
//! The kernel knows no feature.

pub mod auth;
pub mod authz;
pub mod cache;
pub mod directory;
pub mod error;
pub mod http;
pub mod secrets;
pub mod settings;
pub mod state;
pub mod system;

use std::time::{SystemTime, UNIX_EPOCH};

/// A new random id (a UUID).
#[must_use]
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A new random secret: what it is for, then 244 random bits. Shown once; only its hash is kept.
#[must_use]
pub fn new_secret(prefix: &str) -> String {
    format!(
        "{prefix}{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// The hash a secret or token is kept as.
#[must_use]
pub fn hash_secret(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    Sha256::digest(secret.as_bytes())
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Milliseconds since 1970.
#[must_use]
pub fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(i64::MAX)
}
