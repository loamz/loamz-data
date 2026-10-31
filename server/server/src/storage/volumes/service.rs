//! Files in a volume or a table, reached only through governance: find it, ask IAM, then touch the bytes.

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

/// What holds files: a volume (any files) or a table (CSV or Parquet files that a query reads as rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Volume,
    Table,
}

impl Holder {
    #[must_use]
    pub fn kind(self) -> Kind {
        match self {
            Self::Volume => Kind::Volume,
            Self::Table => Kind::Table,
        }
    }

    fn read(self) -> Privilege {
        match self {
            Self::Volume => Privilege::ReadVolume,
            Self::Table => Privilege::Select,
        }
    }

    fn write(self) -> Privilege {
        match self {
            Self::Volume => Privilege::WriteVolume,
            Self::Table => Privilege::Modify,
        }
    }

    /// The word the address uses for it.
    #[must_use]
    pub fn route(self) -> &'static str {
        match self {
            Self::Volume => "volumes",
            Self::Table => "tables",
        }
    }
}

#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
}

async fn holder(
    app: &App,
    ask: &Ask<'_>,
    of: Holder,
    id: &str,
    access: Access,
) -> Result<(Securable, Target)> {
    let (entry, target) = catalogs::target_in(app, ask.workspace, ask.org, id).await?;
    if entry.kind != of.kind() {
        return Err(Error::NotFound("not found".into()));
    }
    let need = match access {
        Access::Read => of.read(),
        Access::Write => of.write(),
    };
    iam::require(app, ask.identity, ask.org, ask.sides, need, &target).await?;
    Ok((entry, target))
}

/// # Errors
/// Not allowed, bad path, too large.
pub async fn put(
    app: &App,
    ask: &Ask<'_>,
    of: Holder,
    id: &str,
    path: &str,
    body: Body,
) -> Result<u64> {
    let (entry, target) = holder(app, ask, of, id, Access::Write).await?;
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
pub async fn get(
    app: &App,
    ask: &Ask<'_>,
    of: Holder,
    id: &str,
    path: &str,
) -> Result<(Body, u64)> {
    let (entry, target) = holder(app, ask, of, id, Access::Read).await?;
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
pub async fn list(app: &App, ask: &Ask<'_>, of: Holder, id: &str) -> Result<Vec<Entry>> {
    let (entry, _) = holder(app, ask, of, id, Access::Read).await?;
    Ok(app.stores.list(&entry.id).await?)
}

/// # Errors
/// Not allowed, bad path, no such file.
pub async fn delete(app: &App, ask: &Ask<'_>, of: Holder, id: &str, path: &str) -> Result<()> {
    let (entry, target) = holder(app, ask, of, id, Access::Write).await?;
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

/// Removes the bytes of a volume or a table. Used by lifecycle, which has already decided; safe to run again.
///
/// # Errors
/// The disk could not be written.
pub async fn erase_files(app: &App, volume_id: &str) -> Result<()> {
    Ok(app.stores.remove_volume(volume_id).await?)
}

/// Whether any bytes of the volume or table are left (the erase check).
///
/// # Errors
/// The disk could not be read.
pub async fn has_files(app: &App, volume_id: &str) -> Result<bool> {
    Ok(app.stores.exists(volume_id).await?)
}
