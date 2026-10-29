//! What a catalog entry is, and the rules for a good one. Pure.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Catalog,
    Schema,
    Table,
    Volume,
    View,
}

impl Kind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Schema => "schema",
            Self::Table => "table",
            Self::Volume => "volume",
            Self::View => "view",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "catalog" => Self::Catalog,
            "schema" => Self::Schema,
            "table" => Self::Table,
            "volume" => Self::Volume,
            "view" => Self::View,
            _ => return None,
        })
    }

    /// What this must sit inside. A catalog sits at the tenant's root, which is no entry at all.
    #[must_use]
    pub fn parent(self) -> Option<Kind> {
        match self {
            Self::Catalog => None,
            Self::Schema => Some(Self::Catalog),
            Self::Table | Self::Volume | Self::View => Some(Self::Schema),
        }
    }
}

pub const CLASSIFICATIONS: [&str; 3] = ["normal", "personal", "sealed"];
/// Where bytes may live. Only the server's own storage exists today; others are refused, not quietly replaced.
pub const PLACEMENTS: [&str; 1] = ["local"];
pub const CONSISTENCIES: [&str; 2] = ["transaction", "snapshot"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Securable {
    pub id: String,
    pub tenant: String,
    pub kind: Kind,
    pub parent_id: String,
    pub name: String,
    pub owner: String,
    pub classification: String,
    pub placement: String,
    pub consistency: String,
    /// `active`, or `dropping` while the drop task is still running.
    pub state: String,
    pub created_ms: i64,
}

/// What a caller asks for. Empty choices get the defaults.
#[derive(Clone, Debug)]
pub struct Draft {
    pub kind: Kind,
    pub parent_id: String,
    pub name: String,
    pub classification: String,
    pub placement: String,
    pub consistency: String,
}

/// Names are lower case words with digits and underscores, so they are safe in any URL, file name or SQL identifier.
#[must_use]
pub fn valid_name(n: &str) -> bool {
    let mut chars = n.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && n.len() <= 63
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Fills in defaults and refuses what cannot be honoured.
///
/// # Errors
/// A message saying what is wrong.
pub fn check(mut d: Draft) -> Result<Draft, String> {
    if !valid_name(&d.name) {
        return Err("name must be lower case letters, digits and underscores, starting with a letter (63 at most)".into());
    }
    if d.classification.is_empty() {
        d.classification = "normal".into();
    }
    if d.placement.is_empty() {
        d.placement = "local".into();
    }
    if d.consistency.is_empty() {
        d.consistency = if d.kind == Kind::Volume {
            "snapshot"
        } else {
            "transaction"
        }
        .into();
    }
    if !CLASSIFICATIONS.contains(&d.classification.as_str()) {
        return Err(format!(
            "classification must be one of {}",
            CLASSIFICATIONS.join(", ")
        ));
    }
    if !PLACEMENTS.contains(&d.placement.as_str()) {
        return Err(format!(
            "placement must be one of {}",
            PLACEMENTS.join(", ")
        ));
    }
    if !CONSISTENCIES.contains(&d.consistency.as_str()) {
        return Err(format!(
            "consistency must be one of {}",
            CONSISTENCIES.join(", ")
        ));
    }
    // A request this placement cannot keep is refused, never quietly weakened.
    if d.kind == Kind::Volume && d.consistency == "transaction" {
        return Err("a volume holds files and can only promise snapshot consistency".into());
    }
    if d.kind == Kind::Catalog && !d.parent_id.is_empty() {
        return Err("a catalog has no parent".into());
    }
    if d.kind != Kind::Catalog && d.parent_id.is_empty() {
        return Err("parent_id is required".into());
    }
    Ok(d)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn draft(kind: Kind) -> Draft {
        Draft {
            kind,
            parent_id: if kind == Kind::Catalog {
                String::new()
            } else {
                "p".into()
            },
            name: "sales".into(),
            classification: String::new(),
            placement: String::new(),
            consistency: String::new(),
        }
    }

    #[test]
    fn defaults_are_filled_and_follow_the_kind() {
        let t = check(draft(Kind::Table)).unwrap();
        assert_eq!(
            (
                t.classification.as_str(),
                t.placement.as_str(),
                t.consistency.as_str()
            ),
            ("normal", "local", "transaction")
        );
        assert_eq!(check(draft(Kind::Volume)).unwrap().consistency, "snapshot");
    }

    #[test]
    fn a_request_the_storage_cannot_keep_is_refused() {
        let mut v = draft(Kind::Volume);
        v.consistency = "transaction".into();
        assert!(check(v).is_err());
        let mut t = draft(Kind::Table);
        t.placement = "s3".into();
        assert!(check(t).is_err());
    }

    #[test]
    fn names_are_plain_words() {
        for bad in [
            "",
            "Sales",
            "1a",
            "a-b",
            "a b",
            "../x",
            "a/b",
            &"a".repeat(64),
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(valid_name("sales_2026"));
    }

    #[test]
    fn parents_follow_the_hierarchy() {
        assert_eq!(Kind::Volume.parent(), Some(Kind::Schema));
        assert_eq!(Kind::Schema.parent(), Some(Kind::Catalog));
        assert_eq!(Kind::Catalog.parent(), None);
        let mut c = draft(Kind::Catalog);
        c.parent_id = "x".into();
        assert!(check(c).is_err());
        let mut s = draft(Kind::Schema);
        s.parent_id.clear();
        assert!(check(s).is_err());
    }
}
