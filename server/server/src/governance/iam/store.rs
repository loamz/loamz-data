use sqlx::{Any, Executor, Row};

use crate::kernel::{authz::Grant, authz::Privilege, error::Result};

/// Every grant in this organization held by any of these principals.
pub async fn held_by(
    c: &mut sqlx::AnyConnection,
    org: &str,
    principals: &[String],
) -> Result<Vec<Grant>> {
    let mut out = Vec::new();
    for p in principals {
        let rows = sqlx::query("SELECT principal, securable, privilege FROM grants WHERE org_id = $1 AND principal = $2").bind(org).bind(p).fetch_all(&mut *c).await?;
        for r in rows {
            let privilege: String = r.try_get("privilege")?;
            if let Some(privilege) = Privilege::parse(&privilege) {
                out.push(Grant {
                    principal: r.try_get("principal")?,
                    securable: r.try_get("securable")?,
                    privilege,
                });
            }
        }
    }
    Ok(out)
}

pub async fn put<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    principal: &str,
    securable: &str,
    privilege: &str,
    by: &str,
    at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO grants (org_id, principal, securable, privilege, granted_by, created_ms) VALUES ($1,$2,$3,$4,$5,$6) \
         ON CONFLICT (org_id, principal, securable, privilege) DO NOTHING",
    )
    .bind(org)
    .bind(principal)
    .bind(securable)
    .bind(privilege)
    .bind(by)
    .bind(at)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn remove<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    principal: &str,
    securable: &str,
) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM grants WHERE org_id = $1 AND principal = $2 AND securable = $3")
            .bind(org)
            .bind(principal)
            .bind(securable)
            .execute(e)
            .await?
            .rows_affected(),
    )
}

/// Everything granted directly on one securable: `(principal, privilege)`.
pub async fn on<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    securable: &str,
) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query("SELECT principal, privilege FROM grants WHERE org_id = $1 AND securable = $2 ORDER BY principal, privilege").bind(org).bind(securable).fetch_all(e).await?;
    rows.iter()
        .map(|r| Ok((r.try_get("principal")?, r.try_get("privilege")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn remove_on<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    securable: &str,
) -> Result<()> {
    sqlx::query("DELETE FROM grants WHERE org_id = $1 AND securable = $2")
        .bind(org)
        .bind(securable)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn remove_for<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    principal: &str,
) -> Result<()> {
    sqlx::query("DELETE FROM grants WHERE org_id = $1 AND principal = $2")
        .bind(org)
        .bind(principal)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn count_for<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    principal: &str,
) -> Result<i64> {
    let r = sqlx::query("SELECT COUNT(*) AS n FROM grants WHERE org_id = $1 AND principal = $2")
        .bind(org)
        .bind(principal)
        .fetch_one(e)
        .await?;
    Ok(r.try_get("n")?)
}
