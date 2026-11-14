//! Files in a volume or a table, reached only through governance: find it, ask IAM, then touch the bytes.

use axum::body::Body;
use futures_util::StreamExt;
use loamz_lake::files::Entry;

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

fn file_error(error: loamz_lake::files::FileError) -> Error {
    match error {
        loamz_lake::files::FileError::BadPath(_) => Error::NotFound("not found".into()),
        other => other.into(),
    }
}

/// Who is asking, where from, and as what.
pub struct Ask<'a> {
    pub identity: &'a Identity,
    pub workspace: &'a str,
    pub org: &'a str,
    pub sides: &'a [Side],
}

#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
}

async fn holder(app: &App, ask: &Ask<'_>, id: &str, access: Access) -> Result<(Securable, Target)> {
    let (entry, target) = catalogs::target_in(app, ask.workspace, ask.org, id).await?;
    if entry.kind != Kind::Volume {
        return Err(Error::NotFound("not found".into()));
    }
    let need = match access {
        Access::Read => Privilege::ReadVolume,
        Access::Write => Privilege::WriteVolume,
    };
    iam::require(app, ask.identity, ask.org, ask.sides, need, &target).await?;
    Ok((entry, target))
}

async fn run_output_path(
    app: &App,
    ask: &Ask<'_>,
    target: &Target,
    volume: &str,
    path: &str,
    action: &str,
) -> Result<()> {
    let Some(scope) = &ask.identity.scope else {
        return Ok(());
    };
    if scope.writes.iter().any(|id| id == volume)
        && !path.starts_with(&format!("runs/{}/", scope.run))
    {
        audit::record(
            app,
            ask.org,
            &Who::of(ask.identity),
            action,
            &format!("{}/runs/{}", target.label, scope.run),
            "denied",
        )
        .await?;
        return Err(Error::NotFound("not found".into()));
    }
    Ok(())
}

/// # Errors
/// Not allowed, a hidden path, or too large.
pub async fn put(app: &App, ask: &Ask<'_>, id: &str, path: &str, body: Body) -> Result<u64> {
    let (entry, target) = holder(app, ask, id, Access::Write).await?;
    run_output_path(app, ask, &target, id, path, "file.write").await?;
    let mut upload = app
        .places
        .begin_put(&entry.folder, path, app.max_upload)
        .await
        .map_err(file_error)?;
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
/// Not allowed, a hidden path, or no such file.
pub async fn get(app: &App, ask: &Ask<'_>, id: &str, path: &str) -> Result<(Body, u64)> {
    let (entry, target) = holder(app, ask, id, Access::Read).await?;
    let (file, size) = app
        .places
        .open(&entry.folder, path)
        .await
        .map_err(file_error)?;
    audit::record(
        app,
        ask.org,
        &Who::of(ask.identity),
        "file.read",
        &format!("{}/{path}", target.label),
        "ok",
    )
    .await?;
    Ok((Body::from_stream(file), size))
}

/// # Errors
/// Not allowed.
pub async fn list(app: &App, ask: &Ask<'_>, id: &str) -> Result<Vec<Entry>> {
    let (entry, _) = holder(app, ask, id, Access::Read).await?;
    Ok(app.places.list(&entry.folder).await?)
}

/// # Errors
/// Not allowed, a hidden path, or no such file.
pub async fn delete(app: &App, ask: &Ask<'_>, id: &str, path: &str) -> Result<()> {
    let (entry, target) = holder(app, ask, id, Access::Write).await?;
    run_output_path(app, ask, &target, id, path, "file.delete").await?;
    app.places
        .delete(&entry.folder, path)
        .await
        .map_err(file_error)?;
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

/// Removes the bytes of a volume or a table's folder. Used by lifecycle, which has already decided; safe to run again.
///
/// # Errors
/// The disk could not be written.
pub async fn erase_files(app: &App, folder: &str) -> Result<()> {
    Ok(app.places.remove_volume(folder).await?)
}

/// Whether any bytes of the volume or table folder are left (the erase check).
///
/// # Errors
/// The disk could not be read.
pub async fn has_files(app: &App, folder: &str) -> Result<bool> {
    Ok(app.places.exists(folder).await?)
}
