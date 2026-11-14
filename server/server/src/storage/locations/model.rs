//! Credentials and locations, and the rule that locations never overlap. Pure.

/// Public settings and the private vault pointer for a storage credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credential {
    pub id: String,
    pub org_id: String,
    pub workspace_id: String,
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    pub region: String,
    pub secret_path: String,
    pub secret_version: i64,
    pub generation: i64,
    pub created_by: String,
    pub created_ms: i64,
}

/// An approved part of a store: a bucket, or a prefix in it, reached with one credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub id: String,
    pub org_id: String,
    pub workspace_id: String,
    pub name: String,
    pub credential_id: String,
    pub bucket: String,
    pub prefix: String,
    pub created_by: String,
    pub created_ms: i64,
}

/// A bucket name as S3 allows it: 3 to 63 lower-case letters, digits, dots and dashes, starting and ending with a letter or digit.
#[must_use]
pub fn valid_bucket(name: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    (3..=63).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && edge(name.chars().next())
        && edge(name.chars().last())
}

/// A prefix as the model keeps it: no leading or trailing slash, plain segments, no `.` or `..`. Empty means the whole bucket.
///
/// # Errors
/// A segment is empty or not plain.
pub fn normalize_prefix(prefix: &str) -> Result<String, &'static str> {
    let trimmed = prefix.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    for part in trimmed.split('/') {
        let plain = !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !plain {
            return Err(
                "a prefix is plain folder names: letters, digits, dashes, underscores and dots",
            );
        }
    }
    Ok(trimmed.to_owned())
}

/// Whether two prefixes of the same bucket overlap: they are the same, or one is a folder inside the other (the whole bucket
/// contains everything). `/data` and `/data2` do not overlap.
#[must_use]
pub fn overlaps(a: &str, b: &str) -> bool {
    a.is_empty()
        || b.is_empty()
        || a == b
        || a.starts_with(&format!("{b}/"))
        || b.starts_with(&format!("{a}/"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_named_as_s3_does() {
        for ok in ["loamz-data", "a.b-c", "abc", "b22"] {
            assert!(valid_bucket(ok), "{ok}");
        }
        for bad in ["ab", "-abc", "abc-", "Abc", "a_b", "a b", &"a".repeat(64)] {
            assert!(!valid_bucket(bad), "{bad:?}");
        }
    }

    #[test]
    fn prefixes_are_plain_folders() {
        assert_eq!(normalize_prefix("").unwrap(), "");
        assert_eq!(normalize_prefix("/").unwrap(), "");
        assert_eq!(normalize_prefix("/team-a/raw/").unwrap(), "team-a/raw");
        for bad in [
            "a//b", "a/../b", "./a", "a b", "a?x", "a/*", "a/%2e%2e", "café",
        ] {
            assert!(normalize_prefix(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn locations_overlap_when_one_is_inside_the_other() {
        assert!(overlaps("", "a"), "the whole bucket holds everything");
        assert!(overlaps("a", "a"));
        assert!(overlaps("a", "a/b"));
        assert!(overlaps("a/b", "a"));
        assert!(!overlaps("a", "ab"), "a folder name is not a text prefix");
        assert!(!overlaps("a/b", "a/c"));
    }
}
