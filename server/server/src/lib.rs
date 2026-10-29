//! Loamz Data server. `kernel` is what everything shares; `governance` is the only door; `storage` is what it opens.

pub mod governance;
pub mod kernel;
pub mod storage;

use std::sync::Arc;

use axum::Router;
use kernel::{error::Error, http::Routes, state::Shared};

/// The catalog database's migrations, one set for `SQLite` and `Postgres`.
pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Every route the server has. The tests hold this list to the `OpenAPI` document.
#[must_use]
pub fn routes() -> Routes {
    kernel::system::routes()
        .merge(governance::catalogs::api::routes())
        .merge(governance::iam::api::routes())
        .merge(governance::audit::api::routes())
        .merge(governance::lifecycle::api::routes())
        .merge(storage::volumes::api::routes())
}

/// The router, and the `(method, path)` pairs it serves.
pub fn router(app: Shared) -> (Router, Vec<(&'static str, String)>) {
    let (router, seen) = routes().into_parts();
    // Unknown paths get the same problem document as every other error.
    let router = router
        .fallback(|| async { Error::NotFound("no such route".into()) })
        .with_state(app);
    (router, seen)
}

#[must_use]
pub fn shared(app: kernel::state::App) -> Shared {
    Arc::new(app)
}
