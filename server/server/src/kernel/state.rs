use std::sync::Arc;

use loamz_lake::{files::Volumes, sql::Db};
use tokio::sync::Mutex;

use super::{auth::Verifier, settings::Settings};

pub struct App {
    pub db: Db,
    /// The bytes of volumes and tables: one folder per securable, named by its id.
    pub stores: Volumes,
    pub verifier: Verifier,
    pub max_upload: u64,
    /// One writer per organization's audit chain at a time within this process. A change and its audit entry are written in
    /// one database transaction; the chain's primary key refuses a second server writing the same entry.
    pub write: Mutex<()>,
}

pub type Shared = Arc<App>;

impl App {
    #[must_use]
    pub fn new(settings: &Settings, db: Db) -> Self {
        Self {
            db,
            stores: Volumes::new(&settings.stores_dir),
            verifier: Verifier::new(&settings.issuer),
            max_upload: settings.max_upload_bytes,
            write: Mutex::new(()),
        }
    }
}
