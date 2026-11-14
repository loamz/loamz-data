//! Credentials and locations of a workspace. A workspace's admins manage them. A credential names its secrets by reference into the
//! vault, and the vault must have them; a location is a bucket or a prefix that overlaps no other location of the installation.

use super::{
    model::{Credential, Location, normalize_prefix, overlaps, valid_bucket},
    store,
};
use crate::{
    governance::{
        audit::{model::Who, service::Uow},
        workspaces::{
            model::valid_name,
            service::{deny, open},
        },
    },
    kernel::{
        auth::Identity,
        error::{Error, Result},
        new_id, now_ms,
        places::{self, LocationSource, Opened},
        secrets::SecretStore,
        settings::valid_issuer,
        state::App,
    },
};

/// The workspace's admins manage storage; a service principal never does.
async fn manage(
    app: &App,
    identity: &Identity,
    workspace: &str,
    action: &'static str,
) -> Result<String> {
    let (w, standing) = open(app, identity, workspace).await?;
    if !standing.is_admin() || identity.confined_to.is_some() {
        return Err(deny(app, identity, &w, action).await);
    }
    Ok(w.org_id)
}

fn is_direct_person(identity: &Identity) -> bool {
    identity.kind == crate::kernel::auth::Kind::Person
        && identity.actor.is_none()
        && identity.scope.is_none()
        && identity.confined_to.is_none()
}

async fn manage_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    action: &'static str,
) -> Result<String> {
    let (w, standing) = open(app, identity, workspace).await?;
    if !standing.is_admin() || !is_direct_person(identity) {
        let _ = deny(app, identity, &w, action).await;
        return Err(Error::NotFound("not found".into()));
    }
    Ok(w.org_id)
}

/// Check management access before a secret-bearing body is interpreted.
pub(super) async fn authorize_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
) -> Result<()> {
    manage_credential(app, identity, workspace, "manage a storage credential")
        .await
        .map(|_| ())
}

/// Whether a credential and bucket are the ones this server itself writes to. A folder store (tests only) accepts any.
fn is_own_store(app: &App, endpoint: &str, bucket: &str) -> bool {
    app.places
        .own()
        .bucket_place()
        .is_none_or(|(own_endpoint, own_bucket)| {
            own_endpoint.trim_end_matches('/') == endpoint.trim_end_matches('/')
                && own_bucket == bucket
        })
}

/// What a person gives to make a credential. The secret is never formatted or returned.
pub struct NewCredential {
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    pub region: String,
    pub secret: serde_json::Value,
}

fn valid_secret(provider: &str, secret: &serde_json::Value) -> bool {
    let Some(fields) = secret.as_object() else {
        return false;
    };
    let required: &[&str] = match provider {
        "rustfs" | "minio" => &["kind", "access_key_id", "secret_access_key"],
        "r2" => &["kind", "access_key_id", "secret_access_key", "api_token"],
        _ => return false,
    };
    fields.len() == required.len()
        && required.iter().all(|key| {
            fields
                .get(*key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|v| !v.is_empty() && v.len() <= 8192)
        })
        && fields.get("kind").and_then(serde_json::Value::as_str)
            == Some(if provider == "r2" { "r2" } else { "sts" })
}

fn secret_in_metadata(secret: &serde_json::Value, values: &[&str]) -> bool {
    secret.as_object().is_some_and(|fields| {
        fields
            .iter()
            .filter(|(name, _)| name.as_str() != "kind")
            .filter_map(|(_, value)| value.as_str())
            .any(|secret| values.iter().any(|value| value.contains(secret)))
    })
}

/// Creates a credential with a server-generated vault path.
///
/// # Errors
/// The caller, settings, vault or database cannot complete the operation.
pub async fn create_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    new: NewCredential,
) -> Result<Credential> {
    let org = manage_credential(app, identity, workspace, "create a storage credential").await?;
    if new.provider != "rustfs" {
        return Err(Error::Unsupported(
            "this storage provider is not yet available".into(),
        ));
    }
    if !valid_name(&new.name)
        || !valid_issuer(&new.endpoint)
        || !valid_secret(&new.provider, &new.secret)
    {
        return Err(Error::Invalid("invalid credential settings".into()));
    }
    if secret_in_metadata(
        &new.secret,
        &[&new.name, &new.provider, &new.endpoint, &new.region],
    ) {
        return Err(Error::Invalid("invalid credential settings".into()));
    }
    loamz_lake::endpoint::PinnedEndpoint::resolve(
        &new.endpoint,
        app.storage_allow_private,
        app.storage_allow_http_dev,
    )
    .await
    .map_err(|e| {
        if e == "endpoint resolution failed" {
            Error::Unavailable("endpoint is unavailable".into())
        } else {
            Error::Invalid("the endpoint is not allowed".into())
        }
    })?;
    let vault = app
        .vault
        .as_deref()
        .ok_or_else(|| Error::Unavailable("vault unavailable".into()))?;
    let id = new_id();
    let path = format!("customer-storage/{id}");
    let mut uow = Uow::begin(app).await?;
    if !crate::governance::workspaces::service::exists_in(&mut uow.tx, workspace).await? {
        return Err(Error::NotFound("not found".into()));
    }
    if store::credential_name_taken(&mut *uow.tx, workspace, &new.name).await? {
        return Err(Error::Conflict("credential name already exists".into()));
    }
    let version = vault
        .write_version(&path, &new.secret)
        .await
        .map_err(|_| Error::Unavailable("vault unavailable".into()))?;
    let c = Credential {
        id,
        org_id: org.clone(),
        workspace_id: workspace.to_owned(),
        name: new.name,
        endpoint: new.endpoint.trim_end_matches('/').to_owned(),
        region: if new.region.is_empty() {
            "us-east-1".into()
        } else {
            new.region
        },
        provider: new.provider,
        secret_path: path,
        secret_version: version,
        generation: 1,
        created_by: format!("user:{}", identity.subject),
        created_ms: now_ms(),
    };
    store::insert_credential(&mut *uow.tx, &c).await?;
    uow.audit(
        &org,
        &Who::of(identity),
        "storage.credential.create",
        &format!("credential {} generation {}", c.id, c.generation),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(c)
}

/// # Errors
/// Not an admin of the workspace.
pub async fn credentials(
    app: &App,
    identity: &Identity,
    workspace: &str,
) -> Result<Vec<Credential>> {
    manage_credential(app, identity, workspace, "list storage credentials").await?;
    store::credentials(app.db.pool(), workspace).await
}

/// Reads non-secret metadata for an administrator.
///
/// # Errors
/// The caller cannot manage the credential or it does not exist.
pub async fn credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
) -> Result<Credential> {
    manage_credential(app, identity, workspace, "read a storage credential").await?;
    store::get_credential(app.db.pool(), workspace, id)
        .await?
        .ok_or_else(|| Error::NotFound("not found".into()))
}

/// Replaces the complete secret and atomically switches its pinned version and generation.
///
/// # Errors
/// The caller, vault or database cannot complete the rotation, or the generation is stale.
pub async fn rotate_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
    expected: i64,
    secret: serde_json::Value,
) -> Result<()> {
    let org = manage_credential(app, identity, workspace, "rotate a storage credential").await?;
    let vault = app
        .vault
        .as_deref()
        .ok_or_else(|| Error::Unavailable("vault unavailable".into()))?;
    let mut uow = Uow::begin(app).await?;
    let c = store::get_credential(&mut *uow.tx, workspace, id)
        .await?
        .ok_or_else(|| Error::NotFound("not found".into()))?;
    if c.generation != expected {
        return Err(Error::Precondition);
    }
    if !valid_secret(&c.provider, &secret)
        || secret_in_metadata(&secret, &[&c.name, &c.provider, &c.endpoint, &c.region])
    {
        return Err(Error::Invalid("invalid credential settings".into()));
    }
    let version = vault
        .write_version(&c.secret_path, &secret)
        .await
        .map_err(|_| Error::Unavailable("vault unavailable".into()))?;
    let rows = sqlx::query("UPDATE storage_credentials SET secret_version = $1, generation = generation + 1 WHERE workspace_id = $2 AND id = $3 AND generation = $4 AND secret_version = $5")
        .bind(version).bind(workspace).bind(id).bind(c.generation).bind(c.secret_version).execute(&mut *uow.tx).await?.rows_affected();
    if rows != 1 {
        return Err(Error::Precondition);
    }
    uow.audit(
        &org,
        &Who::of(identity),
        "storage.credential.rotate",
        &format!("credential {id} generation {}", c.generation + 1),
        "ok",
    )
    .await?;
    uow.commit().await?;
    let mut start = 1_i64;
    while start < version {
        let end = start.saturating_add(128).min(version);
        let old: Vec<i64> = (start..end).collect();
        vault
            .destroy_versions(&c.secret_path, &old)
            .await
            .map_err(|_| {
                Error::Unavailable("vault cleanup unavailable; check the current generation".into())
            })?;
        start = end;
    }
    Ok(())
}

fn location_error(e: &loamz_lake::location::LocationError) -> Error {
    match e {
        loamz_lake::location::LocationError::Rejected => {
            Error::Invalid("the store rejected the credential".into())
        }
        loamz_lake::location::LocationError::Unavailable => {
            Error::Unavailable("the store is unavailable".into())
        }
        loamz_lake::location::LocationError::Unsupported => {
            Error::Unsupported("the provider or endpoint is unsupported".into())
        }
    }
}

/// Builds the store of a location from its credential at the version the database pins. The secret is read, used and dropped here.
async fn build_handle(
    app: &App,
    c: &Credential,
    l: &Location,
) -> Result<loamz_lake::location::LocationHandle> {
    let vault = app
        .vault
        .as_ref()
        .ok_or_else(|| Error::Unavailable("vault unavailable".into()))?;
    open_with(
        &**vault,
        app.storage_allow_private,
        app.storage_allow_http_dev,
        c,
        l,
    )
    .await
}

async fn open_with(
    vault: &dyn SecretStore,
    allow_private: bool,
    allow_http: bool,
    c: &Credential,
    l: &Location,
) -> Result<loamz_lake::location::LocationHandle> {
    let mut data = vault
        .read_version(&c.secret_path, c.secret_version)
        .await
        .map_err(|_| Error::Unavailable("vault unavailable".into()))?;
    let mut field = |name: &str| match data.get_mut(name) {
        Some(serde_json::Value::String(value)) => Ok(std::mem::take(value)),
        _ => Err(Error::Unavailable("vault credential is invalid".into())),
    };
    let key_id = field("access_key_id")?;
    let key_secret = field("secret_access_key")?;
    let token = if c.provider == "r2" {
        Some(field("api_token")?)
    } else {
        None
    };
    let secret = loamz_lake::location::LocationSecret::new(key_id, key_secret, token);
    drop(data);
    let config = loamz_lake::location::LocationCredential {
        provider: c.provider.clone(),
        endpoint: c.endpoint.clone(),
        region: c.region.clone(),
        bucket: l.bucket.clone(),
        prefix: l.prefix.clone(),
        generation: c.generation,
        allow_private,
        allow_http,
    };
    loamz_lake::location::build_location(&config, &secret)
        .await
        .map_err(|e| location_error(&e))
}

/// Where the kernel finds a location's store: it reads the location and its credential, and builds the store.
struct Source {
    db: loamz_lake::sql::Db,
    vault: std::sync::Arc<dyn SecretStore>,
    allow_private: bool,
    allow_http: bool,
}

#[async_trait::async_trait]
impl LocationSource for Source {
    async fn generation(&self, location: &str) -> std::result::Result<Option<i64>, String> {
        let Some(l) = store::location_any(self.db.pool(), location)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        Ok(
            store::get_credential(self.db.pool(), &l.workspace_id, &l.credential_id)
                .await
                .map_err(|e| e.to_string())?
                .map(|c| c.generation),
        )
    }

    async fn open(&self, location: &str) -> std::result::Result<Opened, String> {
        let l = store::location_any(self.db.pool(), location)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no such location")?;
        let c = store::get_credential(self.db.pool(), &l.workspace_id, &l.credential_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no such credential")?;
        let handle = open_with(&*self.vault, self.allow_private, self.allow_http, &c, &l)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Opened {
            handle,
            prefix: l.prefix,
        })
    }
}

/// Lets the kernel reach the storage locations of customers. Done once at start, when the vault is known.
pub fn attach(app: &App) {
    if let Some(vault) = &app.vault {
        app.places.attach(std::sync::Arc::new(Source {
            db: app.db.clone(),
            vault: vault.clone(),
            allow_private: app.storage_allow_private,
            allow_http: app.storage_allow_http_dev,
        }));
    }
}

/// Checks a location using its current pinned vault version without returning credential material.
///
/// # Errors
/// The caller, location, vault, endpoint or provider cannot complete the check.
pub async fn check_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
    location_id: &str,
) -> Result<(i64, &'static str)> {
    let c = credential(app, identity, workspace, id).await?;
    let l = store::location_by_id(app.db.pool(), workspace, location_id)
        .await?
        .filter(|l| l.credential_id == c.id)
        .ok_or_else(|| Error::NotFound("not found".into()))?;
    let generation = c.generation;
    let outcome: Result<()> = async {
        let handle = build_handle(app, &c, &l).await?;
        loamz_lake::location::check(&handle)
            .await
            .map_err(|e| location_error(&e))
    }
    .await;
    match outcome {
        Ok(()) => Ok((generation, "usable")),
        Err(Error::Invalid(_)) => Ok((generation, "denied")),
        Err(Error::Unavailable(_)) => Ok((generation, "unavailable")),
        Err(error) => Err(error),
    }
}

/// # Errors
/// Not an admin, no such credential, or a location still uses it.
pub async fn delete_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let org = manage_credential(app, identity, workspace, "delete a storage credential").await?;
    let vault = app
        .vault
        .as_deref()
        .ok_or_else(|| Error::Unavailable("vault unavailable".into()))?;
    let mut uow = Uow::begin(app).await?;
    let credential = store::get_credential(&mut *uow.tx, workspace, id)
        .await?
        .ok_or_else(|| Error::NotFound("not found".into()))?;
    if store::locations_using(&mut *uow.tx, &credential.id).await? > 0 {
        return Err(Error::Conflict(
            "a location still uses this credential: delete it first".into(),
        ));
    }
    if store::delete_credential(&mut uow.tx, workspace, id).await? == 0 {
        return Err(Error::NotFound("not found".into()));
    }
    uow.audit(
        &org,
        &Who::of(identity),
        "storage.credential.delete",
        &format!("credential {id}"),
        "ok",
    )
    .await?;
    uow.commit().await?;
    vault
        .destroy_path(&credential.secret_path)
        .await
        .map_err(|_| {
            Error::Unavailable(
                "credential deleted, but vault cleanup failed; contact an administrator".into(),
            )
        })
}

/// What a person gives to make a location.
pub struct NewLocation {
    pub name: String,
    pub credential_id: String,
    pub bucket: String,
    pub prefix: String,
}

/// Makes a location: a bucket or a prefix of it, reached with one of the workspace's credentials. It may overlap no other location of
/// the installation in the same store and bucket.
///
/// # Errors
/// Not an admin, a bad name, bucket or prefix, no such credential, or an overlap.
pub async fn create_location(
    app: &App,
    identity: &Identity,
    workspace: &str,
    new: NewLocation,
) -> Result<Location> {
    let org = manage(app, identity, workspace, "create a storage location").await?;
    if !valid_name(&new.name) {
        return Err(Error::Invalid(
            "the name is words, digits, spaces, dashes or underscores, up to 64".into(),
        ));
    }
    if !valid_bucket(&new.bucket) {
        return Err(Error::Invalid("the bucket name is not valid".into()));
    }
    let prefix = normalize_prefix(&new.prefix).map_err(|e| Error::Invalid(e.into()))?;
    let credential = store::get_credential(app.db.pool(), workspace, &new.credential_id)
        .await?
        .ok_or_else(|| Error::NotFound("no such credential in this workspace".into()))?;
    let l = Location {
        id: new_id(),
        org_id: org.clone(),
        workspace_id: workspace.to_owned(),
        name: new.name,
        credential_id: credential.id.clone(),
        bucket: new.bucket,
        prefix,
        created_by: format!("user:{}", identity.subject),
        created_ms: now_ms(),
    };
    // The server's own bucket also holds every workspace's default storage, one folder per workspace id: a location may not take
    // the whole bucket, or sit in or over someone else's.
    if app.places.own().bucket_place().is_some()
        && is_own_store(app, &credential.endpoint, &l.bucket)
        && (l.prefix.is_empty()
            || store::other_workspace(
                app.db.pool(),
                l.prefix.split('/').next().unwrap_or_default(),
                workspace,
            )
            .await?)
    {
        return Err(if l.prefix.is_empty() {
            Error::Invalid(
                "this installation's own bucket is shared: give the location a folder".into(),
            )
        } else {
            Error::NotFound("not found".into())
        });
    }
    let mut uow = Uow::begin(app).await?;
    if store::get_credential(&mut *uow.tx, workspace, &l.credential_id)
        .await?
        .is_none()
    {
        return Err(Error::NotFound("not found".into()));
    }
    if store::location_name_taken(&mut *uow.tx, workspace, &l.name).await? {
        return Err(Error::Conflict(format!("{} already exists here", l.name)));
    }
    let taken = store::prefixes_in_bucket(&mut *uow.tx, &credential.endpoint, &l.bucket).await?;
    if let Some((owner, _)) = taken.iter().find(|(_, prefix)| overlaps(prefix, &l.prefix)) {
        return Err(if owner == workspace {
            Error::Conflict("this overlaps another location: locations never share a path".into())
        } else {
            Error::NotFound("not found".into())
        });
    }
    store::insert_location(&mut *uow.tx, &l).await?;
    uow.audit(
        &org,
        &Who::of(identity),
        "storage.location.create",
        &format!("location {}", l.id),
        "ok",
    )
    .await?;
    uow.commit().await?;
    Ok(l)
}

/// # Errors
/// Not an admin of the workspace.
pub async fn locations(app: &App, identity: &Identity, workspace: &str) -> Result<Vec<Location>> {
    manage(app, identity, workspace, "list storage locations").await?;
    store::locations(app.db.pool(), workspace).await
}

/// # Errors
/// Not an admin, or no such location.
pub async fn delete_location(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let org = manage(app, identity, workspace, "delete a storage location").await?;
    if let Some(l) = store::location_by_id(app.db.pool(), workspace, id).await?
        && store::catalogs_under(app.db.pool(), workspace, &places::root_of(&l.id)).await?
    {
        return Err(Error::Conflict(
            "a catalog keeps its data in this location: remove the catalog first".into(),
        ));
    }
    let mut uow = Uow::begin(app).await?;
    if store::delete_location(&mut uow.tx, workspace, id).await? == 0 {
        return Err(Error::NotFound("not found".into()));
    }
    uow.audit(
        &org,
        &Who::of(identity),
        "storage.location.delete",
        &format!("location {id}"),
        "ok",
    )
    .await?;
    uow.commit().await
}

/// What a new catalog's folders begin with when it is put on a registered location: the location's marker. The catalog's bytes are
/// then read and written through the location's own credential, in the location's own bucket and folder.
///
/// # Errors
/// Not an admin of the workspace, or no such location.
pub async fn managed_root(
    app: &App,
    identity: &Identity,
    workspace: &str,
    name: &str,
) -> Result<String> {
    manage(
        app,
        identity,
        workspace,
        "put a catalog on a storage location",
    )
    .await?;
    let l = store::location_by_name(app.db.pool(), workspace, name)
        .await?
        .ok_or_else(|| Error::NotFound("no such storage location in this workspace".into()))?;
    Ok(places::root_of(&l.id))
}

#[cfg(test)]
mod custody_identity_tests {
    use super::*;
    use crate::kernel::auth::{Kind, Scope};

    #[test]
    fn only_a_direct_person_can_manage_custody() {
        let mut identity = Identity {
            subject: "person".into(),
            kind: Kind::Person,
            client: "console".into(),
            name: String::new(),
            email: String::new(),
            groups: vec![],
            orgs: vec![],
            roles: vec![],
            actor: None,
            scope: None,
            confined_to: None,
        };
        assert!(is_direct_person(&identity));
        identity.kind = Kind::Service;
        assert!(!is_direct_person(&identity));
        identity.kind = Kind::Person;
        identity.actor = Some("agent".into());
        assert!(!is_direct_person(&identity));
        identity.actor = None;
        identity.scope = Some(Scope {
            run: "run".into(),
            reads: vec![],
            writes: vec![],
        });
        assert!(!is_direct_person(&identity));
    }
}
