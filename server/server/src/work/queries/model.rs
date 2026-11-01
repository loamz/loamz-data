//! A saved query: a name and the SQL, and the compute it last ran on. Pure rules.

use serde::{Deserialize, Serialize};

pub const MAX_NAME: usize = 120;
pub const MAX_SQL: usize = 200_000;

#[derive(Debug, Clone, Serialize)]
pub struct SavedQuery {
    pub id: String,
    pub org_id: String,
    pub workspace_id: String,
    pub owner: String,
    pub name: String,
    pub sql: String,
    /// The compute it last ran on; empty if none was chosen.
    pub target_id: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Deserialize)]
pub struct Draft {
    pub name: String,
    pub sql: String,
    #[serde(default)]
    pub target_id: String,
}

/// What is wrong with a draft, if anything.
#[must_use]
pub fn problem(d: &Draft) -> Option<&'static str> {
    if d.name.trim().is_empty() || d.name.chars().count() > MAX_NAME {
        return Some("a saved query needs a name of at most 120 characters");
    }
    if d.sql.len() > MAX_SQL {
        return Some("that query is too long to save");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str, sql: &str) -> Draft {
        Draft {
            name: name.into(),
            sql: sql.into(),
            target_id: String::new(),
        }
    }

    #[test]
    fn a_name_is_needed_and_bounded() {
        assert!(problem(&draft("", "SELECT 1")).is_some());
        assert!(problem(&draft("   ", "SELECT 1")).is_some());
        assert!(problem(&draft(&"x".repeat(121), "SELECT 1")).is_some());
        assert!(problem(&draft("daily", "SELECT 1")).is_none());
    }

    #[test]
    fn the_sql_is_bounded() {
        assert!(problem(&draft("big", &"x".repeat(MAX_SQL + 1))).is_some());
    }
}
