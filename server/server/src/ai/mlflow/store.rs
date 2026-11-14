//! Which `MLflow` objects belong to which workspace and securable. `MLflow`'s own ids mean nothing here until they are in this table, so
//! a guessed id from another workspace finds nothing.

use sqlx::{Any, Executor, Row};

use crate::kernel::error::Result;

/// Remembers an `MLflow` object: `kind` is `experiment` (the securable is the experiment's own) or `run` (the securable is its experiment's).
pub async fn insert<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    kind: &str,
    upstream_id: &str,
    securable_id: &str,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO mlflow_objects (workspace_id, kind, upstream_id, securable_id, created_by) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(workspace)
    .bind(kind)
    .bind(upstream_id)
    .bind(securable_id)
    .bind(created_by)
    .execute(e)
    .await?;
    Ok(())
}

/// The securable an `MLflow` object of this workspace belongs to.
pub async fn securable_of<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    kind: &str,
    upstream_id: &str,
) -> Result<Option<String>> {
    let row = sqlx::query(
        "SELECT securable_id FROM mlflow_objects WHERE workspace_id = $1 AND kind = $2 AND upstream_id = $3",
    )
    .bind(workspace)
    .bind(kind)
    .bind(upstream_id)
    .fetch_optional(e)
    .await?;
    Ok(match row {
        Some(r) => Some(r.try_get::<String, _>("securable_id")?),
        None => None,
    })
}

/// `MLflow`'s id for a securable (an experiment), if it has one.
pub async fn upstream_of<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    kind: &str,
    securable_id: &str,
) -> Result<Option<String>> {
    let row = sqlx::query(
        "SELECT upstream_id FROM mlflow_objects WHERE workspace_id = $1 AND kind = $2 AND securable_id = $3",
    )
    .bind(workspace)
    .bind(kind)
    .bind(securable_id)
    .fetch_optional(e)
    .await?;
    Ok(match row {
        Some(r) => Some(r.try_get::<String, _>("upstream_id")?),
        None => None,
    })
}
