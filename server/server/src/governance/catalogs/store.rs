use sqlx::{Any, Executor, Row};

use super::model::{Kind, Securable};
use crate::kernel::error::{Error, Result};

const COLUMNS: &str = "id, org_id, workspace_id, folder, kind, parent_id, name, description, tags, owner, classification, placement, consistency, state, created_ms, metadata";

pub struct CandidateSearch<'a> {
    pub org: &'a str,
    pub workspace: &'a str,
    pub kind: &'a str,
    pub owner: &'a str,
    pub tag_pattern: &'a str,
    pub text_pattern: &'a str,
    pub after_name: &'a str,
    pub after_id: &'a str,
    pub limit: i64,
}

fn row(r: &sqlx::any::AnyRow) -> Result<Securable> {
    let kind: String = r.try_get("kind")?;
    let tags: String = r.try_get("tags")?;
    Ok(Securable {
        id: r.try_get("id")?,
        org_id: r.try_get("org_id")?,
        workspace_id: r.try_get("workspace_id")?,
        folder: r.try_get("folder")?,
        kind: Kind::parse(&kind)
            .ok_or_else(|| Error::Internal(format!("unknown kind {kind} in the database")))?,
        parent_id: r.try_get("parent_id")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        tags: serde_json::from_str(&tags)
            .map_err(|_| Error::Internal("stored securable tags are damaged".into()))?,
        owner: r.try_get("owner")?,
        classification: r.try_get("classification")?,
        placement: r.try_get("placement")?,
        consistency: r.try_get("consistency")?,
        state: r.try_get("state")?,
        created_ms: r.try_get("created_ms")?,
        metadata: r.try_get("metadata")?,
    })
}

pub async fn insert<'e, E: Executor<'e, Database = Any>>(e: E, s: &Securable) -> Result<()> {
    let description_folded = s.description.as_deref().unwrap_or("").to_lowercase();
    let owner_folded = s.owner.to_lowercase();
    sqlx::query(&format!(
        "INSERT INTO securables ({COLUMNS}, description_folded, owner_folded)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)"
    ))
    .bind(&s.id)
    .bind(&s.org_id)
    .bind(&s.workspace_id)
    .bind(&s.folder)
    .bind(s.kind.as_str())
    .bind(&s.parent_id)
    .bind(&s.name)
    .bind(&s.description)
    .bind(
        serde_json::to_string(&s.tags)
            .map_err(|_| Error::Internal("tags could not be saved".into()))?,
    )
    .bind(&s.owner)
    .bind(&s.classification)
    .bind(&s.placement)
    .bind(&s.consistency)
    .bind(&s.state)
    .bind(s.created_ms)
    .bind(&s.metadata)
    .bind(description_folded)
    .bind(owner_folded)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn set_metadata(
    c: &mut sqlx::AnyConnection,
    org: &str,
    id: &str,
    description: Option<&str>,
    tags: &[String],
) -> Result<bool> {
    let description_folded = description.unwrap_or("").to_lowercase();
    let tags = serde_json::to_string(tags)
        .map_err(|_| Error::Internal("tags could not be saved".into()))?;
    let result = sqlx::query(
        "UPDATE securables SET description = $3, tags = $4, description_folded = $5
         WHERE org_id = $1 AND id = $2 AND state = 'active'",
    )
    .bind(org)
    .bind(id)
    .bind(description)
    .bind(tags)
    .bind(description_folded)
    .execute(&mut *c)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// One bounded, stable page of active workspace entries matching the supplied filters.
pub async fn search_candidates<'e, E: Executor<'e, Database = Any>>(
    e: E,
    search: CandidateSearch<'_>,
) -> Result<Vec<Securable>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables
         WHERE org_id = $1 AND workspace_id = $2 AND state = 'active'
           AND ($3 = '' OR kind = $3)
           AND ($4 = '' OR owner = $4)
           AND ($5 = '' OR lower(tags) LIKE lower($5) ESCAPE '\\')
           AND ($6 = '' OR lower(name) LIKE $6 ESCAPE '\\'
             OR description_folded LIKE $6 ESCAPE '\\'
             OR lower(tags) LIKE $6 ESCAPE '\\'
             OR owner_folded LIKE $6 ESCAPE '\\')
           AND ($7 = '' OR name > $7 OR (name = $7 AND id > $8))
         ORDER BY name, id LIMIT $9"
    ))
    .bind(search.org)
    .bind(search.workspace)
    .bind(search.kind)
    .bind(search.owner)
    .bind(search.tag_pattern)
    .bind(search.text_pattern)
    .bind(search.after_name)
    .bind(search.after_id)
    .bind(search.limit)
    .fetch_all(e)
    .await?;
    rows.iter().map(row).collect()
}

/// How many catalogs the workspace owns.
pub async fn count_catalogs<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    workspace: &str,
) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM securables WHERE org_id = $1 AND workspace_id = $2 AND kind = 'catalog'",
    )
    .bind(org)
    .bind(workspace)
    .fetch_one(e)
    .await?)
}

pub async fn get<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
) -> Result<Option<Securable>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND id = $2"
    ))
    .bind(org)
    .bind(id)
    .fetch_optional(e)
    .await?;
    r.as_ref().map(row).transpose()
}

pub async fn name_taken<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    workspace: &str,
    parent: &str,
    name: &str,
) -> Result<bool> {
    Ok(
        sqlx::query(
            "SELECT id FROM securables WHERE org_id = $1 AND workspace_id = $2 AND parent_id = $3 AND name = $4",
        )
            .bind(org)
            .bind(workspace)
            .bind(parent)
            .bind(name)
            .fetch_optional(e)
            .await?
            .is_some(),
    )
}

pub async fn children<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    parent: &str,
) -> Result<Vec<Securable>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND parent_id = $2 ORDER BY name"
    ))
    .bind(org)
    .bind(parent)
    .fetch_all(e)
    .await?;
    rows.iter().map(row).collect()
}

/// Whether one of these securables sits below a container, without loading its children.
pub async fn has_descendant(
    c: &mut sqlx::AnyConnection,
    org: &str,
    parent: &str,
    interesting: &[&str],
    depth: usize,
) -> Result<bool> {
    let depth = i64::try_from(depth)
        .map_err(|_| Error::Internal("catalog hierarchy depth is out of range".into()))?;
    for ids in interesting.chunks(500) {
        let placeholders = (4..4 + ids.len())
            .map(|n| format!("${n}"))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "WITH RECURSIVE descendants(id, depth) AS (
               SELECT id, 0 FROM securables WHERE org_id = $1 AND id = $2
               UNION ALL
               SELECT child.id, descendants.depth + 1
               FROM securables child JOIN descendants ON child.parent_id = descendants.id
               WHERE child.org_id = $1 AND child.state = 'active' AND descendants.depth < $3
             )
             SELECT CASE WHEN EXISTS (SELECT 1 FROM descendants WHERE id IN ({placeholders}))
               THEN CAST(1 AS BIGINT) ELSE CAST(0 AS BIGINT) END"
        );
        let mut query = sqlx::query_scalar::<_, i64>(&query)
            .bind(org)
            .bind(parent)
            .bind(depth);
        for id in ids {
            query = query.bind(*id);
        }
        if query.fetch_one(&mut *c).await? != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn owned_by<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    owner: &str,
) -> Result<Vec<Securable>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND owner = $2 ORDER BY created_ms"
    ))
    .bind(org)
    .bind(owner)
    .fetch_all(e)
    .await?;
    rows.iter().map(row).collect()
}

pub async fn set_state<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
    state: &str,
) -> Result<()> {
    sqlx::query("UPDATE securables SET state = $3 WHERE org_id = $1 AND id = $2")
        .bind(org)
        .bind(id)
        .bind(state)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn set_owner<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
    owner: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE securables SET owner = $3, owner_folded = $4 WHERE org_id = $1 AND id = $2",
    )
    .bind(org)
    .bind(id)
    .bind(owner)
    .bind(owner.to_lowercase())
    .execute(e)
    .await?;
    Ok(())
}

pub async fn delete<'e, E: Executor<'e, Database = Any>>(e: E, org: &str, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM securables WHERE org_id = $1 AND id = $2")
        .bind(org)
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn child_by_name<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    workspace: &str,
    parent: &str,
    name: &str,
) -> Result<Option<Securable>> {
    let r = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM securables WHERE org_id = $1 AND workspace_id = $2 AND parent_id = $3 AND name = $4"
    ))
    .bind(org)
    .bind(workspace)
    .bind(parent)
    .bind(name)
    .fetch_optional(e)
    .await?;
    r.as_ref().map(row).transpose()
}

/// Moves a table to its next metadata file, only if it still is at `from` (a commit that lost a race gets false).
pub async fn advance<'e, E: Executor<'e, Database = Any>>(
    e: E,
    org: &str,
    id: &str,
    from: &str,
    to: &str,
) -> Result<bool> {
    let r = sqlx::query(
        "UPDATE securables SET metadata = $4 WHERE org_id = $1 AND id = $2 AND metadata = $3 AND state = 'active'",
    )
    .bind(org)
    .bind(id)
    .bind(from)
    .bind(to)
    .execute(e)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// The compute ids a catalog says may read it (none: any compute of its workspace).
pub async fn compute_allowed<'e, E: Executor<'e, Database = Any>>(
    e: E,
    catalog: &str,
) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT target_id FROM catalog_compute WHERE catalog_id = $1 ORDER BY target_id",
    )
    .bind(catalog)
    .fetch_all(e)
    .await?;
    rows.iter()
        .map(|r| r.try_get("target_id").map_err(Into::into))
        .collect()
}

/// Replaces the list of compute that may read the catalog.
pub async fn set_compute_allowed(
    c: &mut sqlx::AnyConnection,
    catalog: &str,
    targets: &[String],
) -> Result<()> {
    sqlx::query("DELETE FROM catalog_compute WHERE catalog_id = $1")
        .bind(catalog)
        .execute(&mut *c)
        .await?;
    for t in targets {
        sqlx::query("INSERT INTO catalog_compute (catalog_id, target_id) VALUES ($1,$2)")
            .bind(catalog)
            .bind(t)
            .execute(&mut *c)
            .await?;
    }
    Ok(())
}

/// A compute target is gone: no catalog names it any more.
pub async fn forget_compute(c: &mut sqlx::AnyConnection, target: &str) -> Result<()> {
    sqlx::query("DELETE FROM catalog_compute WHERE target_id = $1")
        .bind(target)
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// A catalog is gone: so is its list.
pub async fn forget_catalog_compute(c: &mut sqlx::AnyConnection, catalog: &str) -> Result<()> {
    sqlx::query("DELETE FROM catalog_compute WHERE catalog_id = $1")
        .bind(catalog)
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// The rules of a table, if it has any.
pub async fn get_policy<'e, E: Executor<'e, Database = Any>>(
    e: E,
    table: &str,
) -> Result<Option<super::policy::Policy>> {
    let row = sqlx::query("SELECT row_filter, masks FROM table_policies WHERE table_id = $1")
        .bind(table)
        .fetch_optional(e)
        .await?;
    row.map(|r| {
        let masks: String = r.try_get("masks")?;
        Ok(super::policy::Policy {
            row_filter: r.try_get("row_filter")?,
            masks: serde_json::from_str(&masks).map_err(|_| {
                crate::kernel::error::Error::Internal("stored masks are damaged".into())
            })?,
        })
    })
    .transpose()
}

/// Saves the rules of a table; none at all removes the row.
pub async fn put_policy(
    c: &mut sqlx::AnyConnection,
    org: &str,
    table: &str,
    p: &super::policy::Policy,
    by: &str,
    now_ms: i64,
) -> Result<()> {
    sqlx::query("DELETE FROM table_policies WHERE table_id = $1")
        .bind(table)
        .execute(&mut *c)
        .await?;
    if p.is_empty() {
        return Ok(());
    }
    let masks = serde_json::to_string(&p.masks)
        .map_err(|_| crate::kernel::error::Error::Internal("masks could not be saved".into()))?;
    sqlx::query(
        "INSERT INTO table_policies (table_id, org_id, row_filter, masks, updated_by, updated_ms) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(table)
    .bind(org)
    .bind(p.row_filter.trim())
    .bind(masks)
    .bind(by)
    .bind(now_ms)
    .execute(&mut *c)
    .await?;
    Ok(())
}
