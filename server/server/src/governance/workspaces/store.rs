use sqlx::{Any, Executor, Row};

use super::model::{Member, Role, ServicePrincipal, Workspace};
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

/// A workspace with storage settings must retain its custody and location records.
pub async fn has_storage(c: &mut sqlx::AnyConnection, workspace: &str) -> Result<bool> {
    Ok(sqlx::query("SELECT id FROM storage_credentials WHERE workspace_id = $1 UNION ALL SELECT id FROM storage_locations WHERE workspace_id = $1 LIMIT 1")
        .bind(workspace).fetch_optional(&mut *c).await?.is_some())
}

/// Removes a workspace and everything that hangs on it (members, entitlements, saved queries).
pub async fn delete(c: &mut sqlx::AnyConnection, id: &str) -> Result<()> {
    for table in [
        "workspace_members",
        "entitlements",
        "saved_queries",
        "service_principals",
        "workspace_quotas",
    ] {
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

const SP_COLUMNS: &str = "id, org_id, workspace_id, name, created_by, created_ms";

fn service_principal(r: &sqlx::any::AnyRow) -> Result<ServicePrincipal> {
    Ok(ServicePrincipal {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        created_by: r.try_get("created_by")?,
        created_ms: r.try_get("created_ms")?,
    })
}

pub async fn insert_service_principal<'e, E: Executor<'e, Database = Any>>(
    e: E,
    sp: &ServicePrincipal,
    secret_hash: &str,
) -> Result<()> {
    sqlx::query(&format!(
        "INSERT INTO service_principals ({SP_COLUMNS}, secret_hash) VALUES ($1,$2,$3,$4,$5,$6,$7)"
    ))
    .bind(&sp.id)
    .bind(&sp.org_id)
    .bind(&sp.workspace_id)
    .bind(&sp.name)
    .bind(&sp.created_by)
    .bind(sp.created_ms)
    .bind(secret_hash)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn service_principals<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
) -> Result<Vec<ServicePrincipal>> {
    let rows = sqlx::query(&format!(
        "SELECT {SP_COLUMNS} FROM service_principals WHERE workspace_id = $1 ORDER BY name"
    ))
    .bind(workspace)
    .fetch_all(e)
    .await?;
    rows.iter().map(service_principal).collect()
}

/// The service principal whose token has this hash.
pub async fn service_principal_by_secret<'e, E: Executor<'e, Database = Any>>(
    e: E,
    secret_hash: &str,
) -> Result<Option<ServicePrincipal>> {
    let r = sqlx::query(&format!(
        "SELECT {SP_COLUMNS} FROM service_principals WHERE secret_hash = $1"
    ))
    .bind(secret_hash)
    .fetch_optional(e)
    .await?;
    r.as_ref().map(service_principal).transpose()
}

pub async fn service_principal_name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query("SELECT id FROM service_principals WHERE workspace_id = $1 AND name = $2")
            .bind(workspace)
            .bind(name)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

pub async fn set_service_principal_secret<'e, E: Executor<'e, Database = Any>>(
    e: E,
    workspace: &str,
    id: &str,
    secret_hash: &str,
) -> Result<u64> {
    Ok(sqlx::query(
        "UPDATE service_principals SET secret_hash = $3 WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace)
    .bind(id)
    .bind(secret_hash)
    .execute(e)
    .await?
    .rows_affected())
}

pub async fn delete_service_principal(
    c: &mut sqlx::AnyConnection,
    workspace: &str,
    id: &str,
) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM service_principals WHERE workspace_id = $1 AND id = $2")
            .bind(workspace)
            .bind(id)
            .execute(&mut *c)
            .await?
            .rows_affected(),
    )
}
