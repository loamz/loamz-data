use std::sync::Arc;

use loamz_lake::{grants::Issuer, objects::Store, sql::Db};
use tokio::sync::Mutex;

use super::{auth::Verifier, settings::Settings};

pub struct App {
    pub db: Db,
    /// The bytes of volumes and tables: one prefix per securable, named by its id.
    pub stores: Store,
    pub verifier: Verifier,
    /// Issues short-lived credentials for some folders of the bucket, for machines that read data themselves.
    pub issuer: Option<Issuer>,
    pub max_upload: u64,
    /// The bearer token serverless endpoints accept, read from a mounted file. None: serverless compute is off.
    pub serverless_token: Option<String>,
    /// One writer per organization's audit chain at a time within this process. A change and its audit entry are written in
    /// one database transaction; the chain's primary key refuses a second server writing the same entry.
    pub write: Mutex<()>,
}

pub type Shared = Arc<App>;

impl App {
    /// # Errors
    /// The storage cannot be opened.
    pub fn new(settings: &Settings, db: Db) -> Result<Self, String> {
        Ok(Self {
            db,
            stores: settings.storage.open()?,
            verifier: Verifier::new(&settings.issuer)
                .with_agent_clients(settings.agent_clients.clone()),
            issuer: settings.storage.issuer()?,
            max_upload: settings.max_upload_bytes,
            serverless_token: settings.serverless_token()?,
            write: Mutex::new(()),
        })
    }
}
