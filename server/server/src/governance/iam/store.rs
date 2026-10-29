use loamz_lake::sql::Db;
use sqlx::Row;

use crate::kernel::{authz::Grant, authz::Privilege, error::Result};

/// Every grant held by any of these principals in this tenant.
pub async fn held_by(db: &Db, tenant: &str, principals: &[&str]) -> Result<Vec<Grant>> {
    let mut out = Vec::new();
    for p in principals {
        let rows = sqlx::query("SELECT principal, securable, privilege FROM grants WHERE tenant = $1 AND principal = $2")
            .bind(tenant)
            .bind(*p)
            .fetch_all(db.pool())
            .await?;
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

pub async fn put(
    db: &Db,
    tenant: &str,
    principal: &str,
    securable: &str,
    privilege: &str,
    by: &str,
    at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO grants (tenant, principal, securable, privilege, granted_by, created_ms) VALUES ($1,$2,$3,$4,$5,$6) \
         ON CONFLICT (tenant, principal, securable, privilege) DO NOTHING",
    )
    .bind(tenant)
    .bind(principal)
    .bind(securable)
    .bind(privilege)
    .bind(by)
    .bind(at)
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn remove(db: &Db, tenant: &str, principal: &str, securable: &str) -> Result<u64> {
    let done =
        sqlx::query("DELETE FROM grants WHERE tenant = $1 AND principal = $2 AND securable = $3")
            .bind(tenant)
            .bind(principal)
            .bind(securable)
            .execute(db.pool())
            .await?;
    Ok(done.rows_affected())
}

/// Everything granted on one securable: `(principal, privilege)`.
pub async fn on(db: &Db, tenant: &str, securable: &str) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query("SELECT principal, privilege FROM grants WHERE tenant = $1 AND securable = $2 ORDER BY principal, privilege")
        .bind(tenant)
        .bind(securable)
        .fetch_all(db.pool())
        .await?;
    rows.iter()
        .map(|r| Ok((r.try_get("principal")?, r.try_get("privilege")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn remove_on(db: &Db, tenant: &str, securable: &str) -> Result<()> {
    sqlx::query("DELETE FROM grants WHERE tenant = $1 AND securable = $2")
        .bind(tenant)
        .bind(securable)
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn remove_for(db: &Db, tenant: &str, principal: &str) -> Result<()> {
    sqlx::query("DELETE FROM grants WHERE tenant = $1 AND principal = $2")
        .bind(tenant)
        .bind(principal)
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn count_for(db: &Db, tenant: &str, principal: &str) -> Result<i64> {
    let r = sqlx::query("SELECT COUNT(*) AS n FROM grants WHERE tenant = $1 AND principal = $2")
        .bind(tenant)
        .bind(principal)
        .fetch_one(db.pool())
        .await?;
    Ok(r.try_get("n")?)
}
