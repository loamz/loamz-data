use std::sync::Arc;

use loamz_lake::{grants::Issuer, objects::Store, sql::Db};
use tokio::sync::Mutex;

use super::{
    auth::Verifier,
    directory::Directory,
    secrets::SecretStore,
    settings::{Secrets, Settings},
};

pub struct App {
    pub db: Db,
    /// The bytes of volumes and tables: one prefix per securable, named by its id.
    pub stores: Store,
    pub verifier: Verifier,
    /// Issues short-lived credentials for some folders of the bucket, for machines that read data themselves.
    pub issuer: Option<Arc<dyn Issuer>>,
    /// Where the secrets are, for the settings people make while it runs (a storage credential names its secrets). None: no vault.
    pub vault: Option<Arc<dyn SecretStore>>,
    /// The directory of people and organizations, where the installation's admin creates them. None: it is not connected.
    pub directory: Option<Arc<dyn Directory>>,
    /// Table metadata files, which never change once written: read from the bucket once.
    pub metadata_cache: std::sync::Mutex<super::cache::BoundedCache>,
    /// Wakes the serverless worker the moment a statement is queued, instead of when it next looks.
    pub queued: tokio::sync::Notify,
    /// One pooled connection to the serverless endpoints, so a statement does not pay for a new TLS handshake each time.
    pub serverless_http: reqwest::Client,
    pub max_upload: u64,
    pub max_active_runs: i64,
    /// The bearer token serverless endpoints accept, read from a mounted file. None: serverless compute is off.
    pub serverless_token: Option<String>,
    /// Where the serverless SQL endpoint is (https); with the token, it makes serverless compute creatable.
    pub serverless_endpoint: Option<String>,
    /// One writer per organization's audit chain at a time within this process. A change and its audit entry are written in
    /// one database transaction; the chain's primary key refuses a second server writing the same entry.
    pub write: Mutex<()>,
}

pub type Shared = Arc<App>;

impl App {
    /// # Errors
    /// The storage cannot be opened.
    pub fn new(settings: &Settings, secrets: &Secrets, db: Db) -> Result<Self, String> {
        Ok(Self {
            db,
            stores: settings.storage.open(secrets)?,
            verifier: Verifier::new(&settings.issuer)
                .with_agent_clients(settings.agent_clients.clone()),
            issuer: settings.storage.issuer(secrets)?,
            vault: None,
            directory: settings.directory(secrets)?,
            metadata_cache: std::sync::Mutex::new(super::cache::BoundedCache::new(64 << 20)),
            queued: tokio::sync::Notify::new(),
            serverless_http: reqwest::Client::builder()
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .build()
                .unwrap_or_default(),
            max_upload: settings.max_upload_bytes,
            max_active_runs: settings.max_active_runs,
            serverless_token: secrets.serverless_token.clone(),
            serverless_endpoint: settings.serverless_endpoint.clone(),
            write: Mutex::new(()),
        })
    }
}
