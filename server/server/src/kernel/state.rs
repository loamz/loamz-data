use std::{collections::HashSet, sync::Arc};

use loamz_lake::{files::Volumes, sql::Db};
use tokio::sync::Mutex;

use super::{
    auth::{Caller, Verifier},
    settings::Settings,
};

pub struct App {
    pub db: Db,
    pub volumes: Volumes,
    pub verifier: Verifier,
    pub admins: HashSet<String>,
    pub max_upload: u64,
    /// One writer at a time. The server is a single process and the audit trail is a chain, so writes take turns; the
    /// database's own constraints still refuse a duplicate if a second server is ever started by mistake.
    pub write: Mutex<()>,
}

pub type Shared = Arc<App>;

impl App {
    #[must_use]
    pub fn new(settings: &Settings, db: Db) -> Self {
        Self {
            db,
            volumes: Volumes::new(&settings.volumes_dir),
            verifier: Verifier::new(&settings.issuer, &settings.audience, settings.keys.clone()),
            admins: settings.admins.clone(),
            max_upload: settings.max_upload_bytes,
            write: Mutex::new(()),
        }
    }

    /// Whether this person owns the tenant's root: a personal account always does; an organization's owners are
    /// named in the settings. An agent acting for them does not inherit this: it needs its own grants.
    #[must_use]
    pub fn owns_root(&self, caller: &Caller) -> bool {
        self.admins.contains(&caller.subject) || caller.tenant == format!("u-{}", caller.subject)
    }
}
