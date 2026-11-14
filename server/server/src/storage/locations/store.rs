use sqlx::{Any, Executor, Row};

use super::model::{Credential, Location};
use crate::kernel::error::Result;

const CREDENTIAL: &str = "id, org_id, workspace_id, name, endpoint, region, provider, secret_path, secret_version, generation, created_by, created_ms";
const LOCATION: &str =
    "id, org_id, workspace_id, name, credential_id, bucket, prefix, created_by, created_ms";

fn credential(r: &sqlx::any::AnyRow) -> Result<Credential> {
    Ok(Credential {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        endpoint: r.try_get("endpoint")?,
        region: r.try_get("region")?,
        provider: r.try_get("provider")?,
        secret_path: r.try_get("secret_path")?,
        secret_version: r.try_get("secret_version")?,
        generation: r.try_get("generation")?,
        created_by: r.try_get("created_by")?,
        created_ms: r.try_get("created_ms")?,
    })
}

fn location(r: &sqlx::any::AnyRow) -> Result<Location> {
    Ok(Location {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        credential_id: r.try_get("credential_id")?,
        bucket: r.try_get("bucket")?,
        prefix: r.try_get("prefix")?,
        created_by: r.try_get("created_by")?,
        created_ms: r.try_get("created_ms")?,
    })
}

pub async fn insert_credential<'e, E: Executor<'e, Database = Any>>(
    e: E,
    c: &Credential,
) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO storage_credentials ({CREDENTIAL}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"
    ))
    .bind(&c.id)
    .bind(&c.org_id)
    .bind(&c.workspace_id)
    .bind(&c.name)
    .bind(&c.endpoint)
    .bind(&c.region)
    .bind(&c.provider)
    .bind(&c.secret_path)
    .bind(c.secret_version)
    .bind(c.generation)
    .bind(&c.created_by)
    .bind(c.created_ms)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn credentials<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<Vec<Credential>> {
    let rows = sqlx::query(&format!(
        "SELECT {CREDENTIAL} FROM storage_credentials WHERE workspace_id = $1 ORDER BY name"
    ))
    .bind(workspace)
    .fetch_all(e)
    .await?;
    rows.iter().map(credential).collect()
}

pub async fn get_credential<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    id: &str,
) -> Result<Option<Credential>> {
    let r = sqlx::query(&format!(
        "SELECT {CREDENTIAL} FROM storage_credentials WHERE workspace_id = $1 AND id = $2"
    ))
    .bind(workspace)
    .bind(id)
    .fetch_optional(e)
    .await?;
    r.as_ref().map(credential).transpose()
}

pub async fn credential_name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query("SELECT id FROM storage_credentials WHERE workspace_id = $1 AND name = $2")
            .bind(workspace)
            .bind(name)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

pub async fn delete_credential(
    c: &mut sqlx::AnyConnection,
    workspace: &str,
    id: &str,
) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM storage_credentials WHERE workspace_id = $1 AND id = $2")
            .bind(workspace)
            .bind(id)
            .execute(&mut *c)
            .await?
            .rows_affected(),
    )
}

pub async fn insert_location<'e, E: Executor<'e, Database = Any>>(
    e: E,
    l: &Location,
) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO storage_locations ({LOCATION}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"
    ))
    .bind(&l.id)
    .bind(&l.org_id)
    .bind(&l.workspace_id)
    .bind(&l.name)
    .bind(&l.credential_id)
    .bind(&l.bucket)
    .bind(&l.prefix)
    .bind(&l.created_by)
    .bind(l.created_ms)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn locations<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<Vec<Location>> {
    let rows = sqlx::query(&format!(
        "SELECT {LOCATION} FROM storage_locations WHERE workspace_id = $1 ORDER BY name"
    ))
    .bind(workspace)
    .fetch_all(e)
    .await?;
    rows.iter().map(location).collect()
}

pub async fn location_name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query("SELECT id FROM storage_locations WHERE workspace_id = $1 AND name = $2")
            .bind(workspace)
            .bind(name)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

/// The prefixes already taken in one bucket of one store (across every workspace of the installation: the bytes are shared).
pub async fn prefixes_in_bucket<'e, E: Executor<'e, Database = Any>>(
    e: E,
    endpoint: &str,
    bucket: &str,
) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query(
        "SELECT l.workspace_id, l.prefix FROM storage_locations l JOIN storage_credentials c ON c.id = l.credential_id \
         WHERE c.endpoint = $1 AND l.bucket = $2",
    )
    .bind(endpoint)
    .bind(bucket)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| Ok((r.try_get("workspace_id")?, r.try_get("prefix")?)))
        .collect()
}

pub async fn locations_using<'e, E: Executor<'e, Database = Any>>(
    e: E,
    credential: &str,
) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM storage_locations WHERE credential_id = $1",
    )
    .bind(credential)
    .fetch_one(e)
    .await?)
}

pub async fn delete_location(
    c: &mut sqlx::AnyConnection,
    workspace: &str,
    id: &str,
) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM storage_locations WHERE workspace_id = $1 AND id = $2")
            .bind(workspace)
            .bind(id)
            .execute(&mut *c)
            .await?
            .rows_affected(),
    )
}

/// Whether `id` is some workspace other than `except`: its folder in the server's own bucket is that workspace's default storage.
pub async fn other_workspace<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    except: &str,
) -> Result<bool> {
    Ok(
        sqlx::query("SELECT id FROM workspaces WHERE id = $1 AND id <> $2")
            .bind(id)
            .bind(except)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

/// Whether a catalog of the workspace keeps its bytes under this prefix.
pub async fn catalogs_under<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    prefix: &str,
) -> Result<bool> {
    let like = format!(
        "{}/%",
        prefix
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    Ok(sqlx::query(
        "SELECT id FROM securables WHERE workspace_id = $1 AND kind = 'catalog' AND folder LIKE $2 ESCAPE '\\'",
    )
    .bind(workspace)
    .bind(like)
    .fetch_optional(e)
    .await?
    .is_some())
}

/// A location by its id alone: for the server's own use when it reaches the bytes of a folder that is on one.
pub async fn location_any<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
) -> Result<Option<Location>> {
    let row = sqlx::query(&format!(
        "SELECT {LOCATION} FROM storage_locations WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(e)
    .await?;
    row.as_ref().map(location).transpose()
}

/// A location of the workspace, by name.
pub async fn location_by_name<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    name: &str,
) -> Result<Option<Location>> {
    let row = sqlx::query(&format!(
        "SELECT {LOCATION} FROM storage_locations WHERE workspace_id = $1 AND name = $2"
    ))
    .bind(workspace)
    .bind(name)
    .fetch_optional(e)
    .await?;
    row.as_ref().map(location).transpose()
}

/// A location of the workspace, by id.
pub async fn location_by_id<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    id: &str,
) -> Result<Option<Location>> {
    let row = sqlx::query(&format!(
        "SELECT {LOCATION} FROM storage_locations WHERE workspace_id = $1 AND id = $2"
    ))
    .bind(workspace)
    .bind(id)
    .fetch_optional(e)
    .await?;
    row.as_ref().map(location).transpose()
}
