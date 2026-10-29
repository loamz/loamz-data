//! Files in a volume, reached only through governance: find the volume, ask IAM, then touch the bytes.

use axum::body::Body;
use futures_util::StreamExt;
use loamz_lake::files::Entry;
use tokio_util::io::ReaderStream;

use crate::{
    governance::{
        audit::{model::Who, service as audit},
        catalogs::{
            model::{Kind, Securable},
            service as catalogs,
        },
        iam::service as iam,
    },
    kernel::{
        auth::Caller,
        authz::{Privilege, Target},
        error::{Error, Result},
        state::App,
    },
};

async fn volume(app: &App, caller: &Caller, id: &str) -> Result<(Securable, Target)> {
    let (entry, target) = catalogs::target(app, &caller.tenant, id).await?;
    if entry.kind != Kind::Volume {
        return Err(Error::NotFound("not found".into()));
    }
    Ok((entry, target))
}

/// # Errors
/// Not allowed, bad path, too large.
pub async fn put(app: &App, caller: &Caller, id: &str, path: &str, body: Body) -> Result<u64> {
    let (entry, target) = volume(app, caller, id).await?;
    iam::require(app, caller, Privilege::Write, &target).await?;
    let mut upload = app
        .volumes
        .begin_put(&entry.id, path, app.max_upload)
        .await?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| Error::Invalid(format!("the upload was interrupted: {e}")))?;
        upload.write(&chunk).await?;
    }
    let size = upload.commit().await?;
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        "file.write",
        &format!("{}/{path}", target.label),
        "ok",
    )
    .await?;
    Ok(size)
}

/// # Errors
/// Not allowed, bad path, no such file.
pub async fn get(app: &App, caller: &Caller, id: &str, path: &str) -> Result<(Body, u64)> {
    let (entry, target) = volume(app, caller, id).await?;
    iam::require(app, caller, Privilege::Read, &target).await?;
    let (file, size) = app.volumes.open(&entry.id, path).await?;
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        "file.read",
        &format!("{}/{path}", target.label),
        "ok",
    )
    .await?;
    Ok((Body::from_stream(ReaderStream::new(file)), size))
}

/// # Errors
/// Not allowed.
pub async fn list(app: &App, caller: &Caller, id: &str) -> Result<Vec<Entry>> {
    let (entry, target) = volume(app, caller, id).await?;
    iam::require(app, caller, Privilege::Read, &target).await?;
    Ok(app.volumes.list(&entry.id).await?)
}

/// # Errors
/// Not allowed, bad path, no such file.
pub async fn delete(app: &App, caller: &Caller, id: &str, path: &str) -> Result<()> {
    let (entry, target) = volume(app, caller, id).await?;
    iam::require(app, caller, Privilege::Write, &target).await?;
    app.volumes.delete(&entry.id, path).await?;
    audit::record(
        app,
        &caller.tenant,
        &Who::of(caller),
        "file.delete",
        &format!("{}/{path}", target.label),
        "ok",
    )
    .await
}

/// Removes a volume's bytes. Used by lifecycle, which has already decided; safe to run again.
///
/// # Errors
/// The disk could not be written.
pub async fn erase_files(app: &App, volume_id: &str) -> Result<()> {
    Ok(app.volumes.remove_volume(volume_id).await?)
}

/// Whether any bytes of the volume are left (the erase check).
///
/// # Errors
/// The disk could not be read.
pub async fn has_files(app: &App, volume_id: &str) -> Result<bool> {
    Ok(app.volumes.exists(volume_id).await?)
}
