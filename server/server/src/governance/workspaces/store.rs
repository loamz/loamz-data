use sqlx::{Any, Executor, Row};

use super::model::{Member, Role, Workspace};
use crate::kernel::error::{Error, Result};

fn ws(r: &sqlx::any::AnyRow) -> std::result::Result<Workspace, sqlx::Error> {
    Ok(Workspace {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        name: r.try_get("name")?,
        created_ms: r.try_get("created_ms")?,
    })
}

pub async fn insert<'e, E: Executor<'e, Database = Any>>(
    e: E,
    w: &Workspace,
    by: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO workspaces (id, org_id, name, created_by, created_ms) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(&w.id)
    .bind(&w.org_id)
    .bind(&w.name)
    .bind(by)
    .bind(w.created_ms)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn get<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<Option<Workspace>> {
    let r = sqlx::query("SELECT id, org_id, name, created_ms FROM workspaces WHERE id = $1")
        .bind(id)
        .fetch_optional(e)
        .await?;
    Ok(r.as_ref().map(ws).transpose()?)
}

pub async fn in_org<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
) -> Result<Vec<Workspace>> {
    let rows = sqlx::query(
        "SELECT id, org_id, name, created_ms FROM workspaces WHERE org_id = $1 ORDER BY name",
    )
    .bind(org)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(ws)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub async fn name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query("SELECT id FROM workspaces WHERE org_id = $1 AND name = $2")
            .bind(org)
            .bind(name)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

pub async fn rename<'e, E: Executor<'e, Database = Any>>(e: E, id: &str, name: &str) -> Result<()> {
    sqlx::query("UPDATE workspaces SET name = $2 WHERE id = $1")
        .bind(id)
        .bind(name)
        .execute(e)
        .await?;
    Ok(())
}

/// Removes a workspace and everything that hangs on it (members, entitlements, bindings).
pub async fn delete(c: &mut sqlx::AnyConnection, id: &str) -> Result<()> {
    for table in ["workspace_members", "entitlements", "catalog_bindings"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE workspace_id = $1"))
            .bind(id)
            .execute(&mut *c)
            .await?;
    }
    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(id)
        .execute(&mut *c)
        .await?;
    Ok(())
}

pub async fn members<'e, E: Executor<'e, Database = Any>>(e: E, id: &str) -> Result<Vec<Member>> {
    let rows = sqlx::query(
        "SELECT principal, role FROM workspace_members WHERE workspace_id = $1 ORDER BY principal",
    )
    .bind(id)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| {
            let role: String = r.try_get("role")?;
            Ok(Member {
                principal: r.try_get("principal")?,
                role: Role::parse(&role)
                    .ok_or_else(|| Error::Internal(format!("unknown workspace role {role}")))?,
            })
        })
        .collect()
}

pub async fn put_member<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    principal: &str,
    role: Role,
    at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, principal, role, created_ms) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (workspace_id, principal) DO UPDATE SET role = excluded.role",
    )
    .bind(id)
    .bind(principal)
    .bind(role.as_str())
    .bind(at)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn remove_member(c: &mut sqlx::AnyConnection, id: &str, principal: &str) -> Result<u64> {
    sqlx::query("DELETE FROM entitlements WHERE workspace_id = $1 AND principal = $2")
        .bind(id)
        .bind(principal)
        .execute(&mut *c)
        .await?;
    Ok(
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND principal = $2")
            .bind(id)
            .bind(principal)
            .execute(&mut *c)
            .await?
            .rows_affected(),
    )
}

pub async fn entitlements<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query("SELECT principal, entitlement FROM entitlements WHERE workspace_id = $1 ORDER BY principal, entitlement").bind(id).fetch_all(e).await?;
    rows.iter()
        .map(|r| Ok((r.try_get("principal")?, r.try_get("entitlement")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

pub async fn put_entitlement<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    principal: &str,
    entitlement: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO entitlements (workspace_id, principal, entitlement) VALUES ($1,$2,$3) ON CONFLICT (workspace_id, principal, entitlement) DO NOTHING").bind(id).bind(principal).bind(entitlement).execute(e).await?;
    Ok(())
}

pub async fn remove_entitlement<'e, E: Executor<'e, Database = Any>>(
    e: E,
    id: &str,
    principal: &str,
    entitlement: &str,
) -> Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM entitlements WHERE workspace_id = $1 AND principal = $2 AND entitlement = $3",
    )
    .bind(id)
    .bind(principal)
    .bind(entitlement)
    .execute(e)
    .await?
    .rows_affected())
}

/// The workspaces a catalog is bound to (none: it is visible in all).
pub async fn bindings_of_catalog<'e, E: Executor<'e, Database = Any>>(
    e: E,
    catalog: &str,
) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT workspace_id FROM catalog_bindings WHERE catalog_id = $1")
        .bind(catalog)
        .fetch_all(e)
        .await?;
    rows.iter()
        .map(|r| r.try_get("workspace_id"))
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub async fn bound_catalogs<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT catalog_id FROM catalog_bindings WHERE workspace_id = $1")
        .bind(workspace)
        .fetch_all(e)
        .await?;
    rows.iter()
        .map(|r| r.try_get("catalog_id"))
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub async fn bind<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    catalog: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO catalog_bindings (workspace_id, catalog_id) VALUES ($1,$2) ON CONFLICT (workspace_id, catalog_id) DO NOTHING").bind(workspace).bind(catalog).execute(e).await?;
    Ok(())
}

pub async fn unbind<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    catalog: &str,
) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM catalog_bindings WHERE workspace_id = $1 AND catalog_id = $2")
            .bind(workspace)
            .bind(catalog)
            .execute(e)
            .await?
            .rows_affected(),
    )
}

/// Used when a catalog is dropped.
pub async fn forget_catalog<'e, E: Executor<'e, Database = Any>>(
    e: E,
    catalog: &str,
) -> Result<()> {
    sqlx::query("DELETE FROM catalog_bindings WHERE catalog_id = $1")
        .bind(catalog)
        .execute(e)
        .await?;
    Ok(())
}

/// Used when a person is erased: takes them out of every workspace of the organization.
pub async fn forget_principal(
    c: &mut sqlx::AnyConnection,
    org: &str,
    principal: &str,
) -> Result<()> {
    for table in ["workspace_members", "entitlements"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE principal = $1 AND workspace_id IN (SELECT id FROM workspaces WHERE org_id = $2)")).bind(principal).bind(org).execute(&mut *c).await?;
    }
    Ok(())
}

/// How many assignments a principal still has in the organization (the erase check).
pub async fn count_assignments<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    principal: &str,
) -> Result<i64> {
    let r = sqlx::query("SELECT COUNT(*) AS n FROM workspace_members WHERE principal = $1 AND workspace_id IN (SELECT id FROM workspaces WHERE org_id = $2)").bind(principal).bind(org).fetch_one(e).await?;
    Ok(r.try_get("n")?)
}
