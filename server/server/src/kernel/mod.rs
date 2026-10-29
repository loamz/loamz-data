//! What every feature shares: settings, errors, who is calling, the one permission function, and the app state.
//! The kernel knows no feature.

pub mod auth;
pub mod authz;
pub mod error;
pub mod http;
pub mod settings;
pub mod state;
pub mod system;

use std::time::{SystemTime, UNIX_EPOCH};

/// A new random id (a UUID).
#[must_use]
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
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
