use loamz_lake::sql::Db;
use sqlx::Row;

use super::model::{Kind, Securable};
use crate::kernel::error::{Error, Result};

const COLUMNS: &str = "id, tenant, kind, parent_id, name, owner, classification, placement, consistency, state, created_ms";

fn row(r: &sqlx::any::AnyRow) -> Result<Securable> {
    let kind: String = r.try_get("kind")?;
    Ok(Securable {
        id: r.try_get("id")?,
        tenant: r.try_get("tenant")?,
        kind: Kind::parse(&kind)
            .ok_or_else(|| Error::Internal(format!("unknown kind {kind} in the database")))?,
        parent_id: r.try_get("parent_id")?,
        name: r.try_get("name")?,
        owner: r.try_get("owner")?,
        classification: r.try_get("classification")?,
        placement: r.try_get("placement")?,
        consistency: r.try_get("consistency")?,
        state: r.try_get("state")?,
        created_ms: r.try_get("created_ms")?,
    })
}

pub async fn insert(db: &Db, s: &Securable) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO securables ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"
    ))
    .bind(&s.id)
    .bind(&s.tenant)
    .bind(s.kind.as_str())
    .bind(&s.parent_id)
    .bind(&s.name)
    .bind(&s.owner)
    .bind(&s.classification)
    .bind(&s.placement)
    .bind(&s.consistency)
    .bind(&s.state)
    .bind(s.created_ms)
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn get(db: &Db, tenant: &str, id: &str) -> Result<Option<Securable>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE tenant = $1 AND id = $2"
    ))
    .bind(tenant)
    .bind(id)
    .fetch_optional(db.pool())
    .await?;
    r.as_ref().map(row).transpose()
}

pub async fn name_taken(db: &Db, tenant: &str, parent: &str, name: &str) -> Result<bool> {
    let r =
        sqlx::query("SELECT id FROM securables WHERE tenant = $1 AND parent_id = $2 AND name = $3")
            .bind(tenant)
            .bind(parent)
            .bind(name)
            .fetch_optional(db.pool())
            .await?;
    Ok(r.is_some())
}

pub async fn children(db: &Db, tenant: &str, parent: &str) -> Result<Vec<Securable>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE tenant = $1 AND parent_id = $2 ORDER BY name"
    ))
    .bind(tenant)
    .bind(parent)
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(row).collect()
}

pub async fn owned_by(db: &Db, tenant: &str, owner: &str) -> Result<Vec<Securable>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE tenant = $1 AND owner = $2 ORDER BY created_ms"
    ))
    .bind(tenant)
    .bind(owner)
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(row).collect()
}

pub async fn set_state(db: &Db, tenant: &str, id: &str, state: &str) -> Result<()> {
    sqlx::query("UPDATE securables SET state = $3 WHERE tenant = $1 AND id = $2")
        .bind(tenant)
        .bind(id)
        .bind(state)
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn delete(db: &Db, tenant: &str, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM securables WHERE tenant = $1 AND id = $2")
        .bind(tenant)
        .bind(id)
        .execute(db.pool())
        .await?;
    Ok(())
}
