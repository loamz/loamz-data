use sqlx::{Any, Executor, Row};

use super::model::Org;
use crate::kernel::error::Result;

fn org(r: &sqlx::any::AnyRow) -> std::result::Result<Org, sqlx::Error> {
    Ok(Org {
        id: r.try_get("id")?,
        kind: r.try_get("kind")?,
        alias: r.try_get("alias")?,
        created_ms: r.try_get("created_ms")?,
    })
}

pub async fn get<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<Option<Org>> {
    let r = sqlx::query("SELECT id, kind, alias, created_ms FROM orgs WHERE id = $1")
        .bind(id)
        .fetch_optional(e)
        .await?;
    Ok(r.as_ref().map(org).transpose()?)
}

pub async fn insert<'e, E: Executor<'e, Database = Any>>(
    e: E,
    o: &Org,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO orgs (id, kind, alias, created_by, created_ms) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(&o.id)
    .bind(&o.kind)
    .bind(&o.alias)
    .bind(created_by)
    .bind(o.created_ms)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn admins<'e, E: Executor<'e, Database = Any>>(e: E, org: &str) -> Result<Vec<String>> {
    let rows =
        sqlx::query("SELECT principal FROM account_admins WHERE org_id = $1 ORDER BY principal")
            .bind(org)
            .fetch_all(e)
            .await?;
    rows.iter()
        .map(|r| r.try_get("principal"))
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub async fn add_admin<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    principal: &str,
    by: &str,
    at: i64,
) -> Result<()> {
    sqlx::query("INSERT INTO account_admins (org_id, principal, added_by, created_ms) VALUES ($1,$2,$3,$4) ON CONFLICT (org_id, principal) DO NOTHING")
        .bind(org)
        .bind(principal)
        .bind(by)
        .bind(at)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn remove_admin<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    principal: &str,
) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM account_admins WHERE org_id = $1 AND principal = $2")
            .bind(org)
            .bind(principal)
            .execute(e)
            .await?
            .rows_affected(),
    )
}
