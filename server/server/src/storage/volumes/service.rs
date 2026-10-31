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
        auth::Identity,
        authz::{Privilege, Side, Target},
        error::{Error, Result},
        state::App,
    },
};

/// Who is asking, where from, and as what.
pub struct Ask<'a> {
    pub identity: &'a Identity,
    pub workspace: &'a str,
    pub org: &'a str,
    pub sides: &'a [Side],
}

async fn volume(
    app: &App,
    ask: &Ask<'_>,
    id: &str,
    need: Privilege,
) -> Result<(Securable, Target)> {
    let (entry, target) = catalogs::target_in(app, ask.workspace, ask.org, id).await?;
    if entry.kind != Kind::Volume {
        return Err(Error::NotFound("not found".into()));
    }
    iam::require(app, ask.identity, ask.org, ask.sides, need, &target).await?;
    Ok((entry, target))
}

/// # Errors
/// Not allowed, bad path, too large.
pub async fn put(app: &App, ask: &Ask<'_>, id: &str, path: &str, body: Body) -> Result<u64> {
    let (entry, target) = volume(app, ask, id, Privilege::WriteVolume).await?;
    let mut upload = app
        .stores
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
        ask.org,
        &Who::of(ask.identity),
        "file.write",
        &format!("{}/{path}", target.label),
        "ok",
    )
    .await?;
    Ok(size)
}

/// # Errors
/// Not allowed, bad path, no such file.
pub async fn get(app: &App, ask: &Ask<'_>, id: &str, path: &str) -> Result<(Body, u64)> {
    let (entry, target) = volume(app, ask, id, Privilege::ReadVolume).await?;
    let (file, size) = app.stores.open(&entry.id, path).await?;
    audit::record(
        app,
        ask.org,
        &Who::of(ask.identity),
        "file.read",
        &format!("{}/{path}", target.label),
        "ok",
    )
    .await?;
    Ok((Body::from_stream(ReaderStream::new(file)), size))
}

/// # Errors
/// Not allowed.
pub async fn list(app: &App, ask: &Ask<'_>, id: &str) -> Result<Vec<Entry>> {
    let (entry, _) = volume(app, ask, id, Privilege::ReadVolume).await?;
    Ok(app.stores.list(&entry.id).await?)
}

/// # Errors
/// Not allowed, bad path, no such file.
pub async fn delete(app: &App, ask: &Ask<'_>, id: &str, path: &str) -> Result<()> {
    let (entry, target) = volume(app, ask, id, Privilege::WriteVolume).await?;
    app.stores.delete(&entry.id, path).await?;
    audit::record(
        app,
        ask.org,
        &Who::of(ask.identity),
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
    Ok(app.stores.remove_volume(volume_id).await?)
}

/// Whether any bytes of the volume are left (the erase check).
///
/// # Errors
/// The disk could not be read.
pub async fn has_files(app: &App, volume_id: &str) -> Result<bool> {
    Ok(app.stores.exists(volume_id).await?)
}
