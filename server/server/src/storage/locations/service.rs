//! Credentials and locations of a workspace. A workspace's admins manage them. A credential names its secrets by reference into the
//! vault, and the vault must have them; a location is a bucket or a prefix that overlaps no other location of the installation.

use super::{
    model::{Credential, Location, Vending, normalize_prefix, overlaps, valid_bucket},
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
        secrets::SecretRef,
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

/// What a person gives to make a credential.
pub struct NewCredential {
    pub name: String,
    pub endpoint: String,
    pub region: String,
    pub key_id_ref: String,
    pub secret_ref: String,
    pub vending: Vending,
}

/// The vault must have the secret a reference names: say so now, not at the first read.
async fn check_secret(app: &App, reference: &str) -> Result<()> {
    let r: SecretRef = reference.parse().map_err(Error::Invalid)?;
    let vault = app.vault.as_deref().ok_or_else(|| {
        Error::Unavailable("this installation has no vault to keep storage secrets in".into())
    })?;
    vault
        .get(&r)
        .await
        .map(|_| ())
        .map_err(|e| Error::Invalid(format!("{reference}: {e}")))
}

/// # Errors
/// Not an admin of the workspace, a bad or taken name, a bad endpoint, or a secret the vault does not have.
pub async fn create_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    new: NewCredential,
) -> Result<Credential> {
    let org = manage(app, identity, workspace, "create a storage credential").await?;
    if !valid_name(&new.name) {
        return Err(Error::Invalid(
            "the name is words, digits, spaces, dashes or underscores, up to 64".into(),
        ));
    }
    if !valid_issuer(&new.endpoint) {
        return Err(Error::Invalid(
            "the endpoint must be an https URL, or an http URL on this computer".into(),
        ));
    }
    check_secret(app, &new.key_id_ref).await?;
    check_secret(app, &new.secret_ref).await?;
    if let Vending::R2 { token_ref } = &new.vending {
        check_secret(app, token_ref).await?;
    }
    let c = Credential {
        id: new_id(),
        org_id: org.clone(),
        workspace_id: workspace.to_owned(),
        name: new.name,
        endpoint: new.endpoint.trim_end_matches('/').to_owned(),
        region: if new.region.is_empty() {
            "us-east-1".into()
        } else {
            new.region
        },
        key_id_ref: new.key_id_ref,
        secret_ref: new.secret_ref,
        vending: new.vending,
        created_by: format!("user:{}", identity.subject),
        created_ms: now_ms(),
    };
    let mut uow = Uow::begin(app).await?;
    if store::credential_name_taken(&mut *uow.tx, workspace, &c.name).await? {
        return Err(Error::Conflict(format!("{} already exists here", c.name)));
    }
    store::insert_credential(&mut *uow.tx, &c).await?;
    uow.audit(
        &org,
        &Who::of(identity),
        "storage.credential.create",
        &format!("credential {}", c.id),
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
    manage(app, identity, workspace, "list storage credentials").await?;
    store::credentials(app.db.pool(), workspace).await
}

/// # Errors
/// Not an admin, no such credential, or a location still uses it.
pub async fn delete_credential(
    app: &App,
    identity: &Identity,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let org = manage(app, identity, workspace, "delete a storage credential").await?;
    if store::locations_using(app.db.pool(), id).await? > 0 {
        return Err(Error::Conflict(
            "a location still uses this credential: delete it first".into(),
        ));
    }
    let mut uow = Uow::begin(app).await?;
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
    uow.commit().await
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
    let mut uow = Uow::begin(app).await?;
    if store::location_name_taken(&mut *uow.tx, workspace, &l.name).await? {
        return Err(Error::Conflict(format!("{} already exists here", l.name)));
    }
    let taken = store::prefixes_in_bucket(&mut *uow.tx, &credential.endpoint, &l.bucket).await?;
    if taken.iter().any(|t| overlaps(t, &l.prefix)) {
        return Err(Error::Conflict(
            "this overlaps another location: locations never share a path".into(),
        ));
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
