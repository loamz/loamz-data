//! What happens to data over time. Today: drop something, and erase a person. Both are tasks that must finish.

pub const MAX_ATTEMPTS: i64 = 5;
/// How long a worker owns a task before another may take it (the worker may have died).
pub const LEASE_MS: i64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// `subject` is the id of the catalog entry to drop, with everything inside it.
    Drop,
    /// `subject` is the person whose data, grants and files are erased.
    Erase,
}

impl TaskKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Erase => "erase",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "drop" => Some(Self::Drop),
            "erase" => Some(Self::Erase),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub tenant: String,
    pub kind: String,
    pub subject: String,
    pub requested_by: String,
    pub on_behalf_of: String,
    /// `pending`, `done` or `failed`.
    pub state: String,
    pub attempts: i64,
    pub lease_until: i64,
    pub created_ms: i64,
    pub last_error: String,
}

/// When to look again after a failure: a little longer each time.
#[must_use]
pub fn retry_after_ms(attempts: i64) -> i64 {
    1_000 * attempts.clamp(1, 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_but_not_forever() {
        assert!(retry_after_ms(1) < retry_after_ms(3));
        assert_eq!(retry_after_ms(100), 10_000);
    }

    #[test]
    fn kinds_round_trip() {
        for k in [TaskKind::Drop, TaskKind::Erase] {
            assert_eq!(TaskKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(TaskKind::parse("other"), None);
    }
}
