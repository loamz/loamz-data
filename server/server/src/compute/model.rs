//! What a target and a run are, and the rules for asking for one. Pure.

use sha2::{Digest, Sha256};

/// An agent that has not been heard from for this long is offline.
pub const ONLINE_MS: i64 = 45_000;
/// How long an agent owns a run before the server gives up on it and fails it. The agent renews it while it works.
pub const LEASE_MS: i64 = 90_000;
/// A run nobody picked up for this long is failed, so a question never waits for ever on compute that is gone.
pub const QUEUE_MS: i64 = 600_000;

/// How long serverless compute stays up after its last statement (what the endpoint is deployed with).
pub const IDLE_STOP_MS: i64 = 60_000;

pub const MAX_ROWS: i64 = 10_000;
pub const DEFAULT_ROWS: i64 = 1_000;
pub const MAX_TIMEOUT_S: i64 = 3_600;
pub const MAX_OUTPUT: usize = 64 * 1024;
pub const MAX_RESULT: usize = 4 * 1024 * 1024;

pub const AGENT: &str = "agent";
pub const SERVERLESS: &str = "serverless";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub org_id: String,
    pub name: String,
    /// `agent` or `serverless`.
    pub kind: String,
    /// For a serverless target, the https address of the provider's endpoint.
    pub endpoint: String,
    pub max_memory_mb: i64,
    pub max_cpu_millis: i64,
    pub created_by: String,
    pub created_ms: i64,
    pub last_seen_ms: i64,
}

impl Target {
    #[must_use]
    pub fn serverless(&self) -> bool {
        self.kind == SERVERLESS
    }

    /// Serverless compute is started when work arrives, so it is always there to be asked.
    #[must_use]
    pub fn online(&self, now: i64) -> bool {
        self.serverless() || self.last_seen_ms > 0 && now - self.last_seen_ms < ONLINE_MS
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub id: String,
    pub org_id: String,
    pub workspace_id: String,
    /// `sql` or `job`.
    pub kind: String,
    pub requested_by: String,
    pub on_behalf_of: String,
    pub target_id: String,
    pub state: String,
    pub title: String,
    pub spec: String,
    pub result: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i64,
    pub error: String,
    pub cancel: bool,
    pub lease_until: i64,
    pub created_ms: i64,
    pub started_ms: i64,
    pub finished_ms: i64,
}

/// What a list of runs is narrowed to. Every field left empty narrows nothing.
#[derive(Default, Clone)]
pub struct RunFilter {
    /// What one person asked for (their own, or what ran on their behalf).
    pub by: Option<String>,
    /// `sql` or `job`.
    pub kind: Option<String>,
    pub target: Option<String>,
    pub state: Option<String>,
    /// Started asking at or after this time.
    pub since_ms: Option<i64>,
    /// Started asking before this time (also the page cursor: the oldest `created_ms` of the page before).
    pub before_ms: Option<i64>,
}

pub const STATES: [&str; 6] = [
    "queued",
    "running",
    "succeeded",
    "failed",
    "cancelled",
    "timed_out",
];

#[must_use]
pub fn is_terminal(state: &str) -> bool {
    matches!(state, "succeeded" | "failed" | "cancelled" | "timed_out")
}

/// What an agent may report a run as having ended in.
#[must_use]
pub fn is_final_report(state: &str) -> bool {
    is_terminal(state)
}

/// The hash a secret or token is kept as.
#[must_use]
pub fn hash_secret(secret: &str) -> String {
    Sha256::digest(secret.as_bytes())
        .iter()
        .fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// A target's name is what people pick it by: words, digits, spaces, dashes, up to 64 characters.
#[must_use]
pub fn valid_target_name(n: &str) -> bool {
    let t = n.trim();
    !t.is_empty()
        && t == n
        && n.chars().count() <= 64
        && n.chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub memory_mb: i64,
    pub cpu_millis: i64,
    pub timeout_s: i64,
}

/// The limits a run gets: what was asked for (or a default), never more than the target has, and never nothing.
///
/// # Errors
/// What was asked for is not possible on this target.
pub fn limits_for(
    target: &Target,
    memory_mb: Option<i64>,
    cpu_millis: Option<i64>,
    timeout_s: Option<i64>,
    default_timeout_s: i64,
) -> Result<Limits, String> {
    let memory = memory_mb.unwrap_or(target.max_memory_mb.min(1024));
    let cpu = cpu_millis.unwrap_or(target.max_cpu_millis.min(1000));
    let timeout = timeout_s.unwrap_or(default_timeout_s);
    if memory < 64 || cpu < 100 || timeout < 1 {
        return Err("memory must be at least 64 MB, cpu at least 100 thousandths of a CPU, and the timeout at least 1 second".into());
    }
    if memory > target.max_memory_mb || cpu > target.max_cpu_millis {
        return Err(format!(
            "{} allows at most {} MB and {} thousandths of a CPU",
            target.name, target.max_memory_mb, target.max_cpu_millis
        ));
    }
    if timeout > MAX_TIMEOUT_S {
        return Err(format!("the timeout is at most {MAX_TIMEOUT_S} seconds"));
    }
    Ok(Limits {
        memory_mb: memory,
        cpu_millis: cpu,
        timeout_s: timeout,
    })
}

/// Names for mounting inputs under `/inputs/`: each its own name, made different from the others when two are the same.
#[must_use]
pub fn mount_names(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let mut candidate = n.clone();
        let mut i = 2;
        while out.contains(&candidate) || candidate == "job" || candidate == "tables" {
            candidate = format!("{n}_{i}");
            i += 1;
        }
        out.push(candidate);
    }
    out
}

/// What a job is called when nobody named it: the script's leading comment if it opens with one, else "Untitled job".
#[must_use]
pub fn job_title(given: &str, script: &str) -> String {
    if !given.trim().is_empty() {
        return cap(given.trim(), 200);
    }
    let first = script
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    match first.strip_prefix('#') {
        Some(comment) if !comment.trim().is_empty() => {
            cap(comment.trim_start_matches('#').trim(), 80)
        }
        _ => "Untitled job".into(),
    }
}

/// Cuts text to at most `max` bytes, on a character boundary.
#[must_use]
pub fn cap(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn target() -> Target {
        Target {
            id: "t".into(),
            org_id: "o".into(),
            name: "oracle".into(),
            kind: AGENT.into(),
            endpoint: String::new(),
            max_memory_mb: 2048,
            max_cpu_millis: 1000,
            created_by: String::new(),
            created_ms: 0,
            last_seen_ms: 100_000,
        }
    }

    #[test]
    fn a_target_is_online_only_if_recently_heard_from() {
        let t = target();
        assert!(t.online(100_000 + ONLINE_MS - 1));
        assert!(!t.online(100_000 + ONLINE_MS));
        assert!(
            !Target {
                last_seen_ms: 0,
                ..t
            }
            .online(1),
            "never seen is not online"
        );
    }

    #[test]
    fn limits_default_sensibly_and_never_exceed_the_target() {
        let t = target();
        assert_eq!(
            limits_for(&t, None, None, None, 60).unwrap(),
            Limits {
                memory_mb: 1024,
                cpu_millis: 1000,
                timeout_s: 60
            }
        );
        assert_eq!(
            limits_for(&t, Some(2048), Some(500), Some(10), 60).unwrap(),
            Limits {
                memory_mb: 2048,
                cpu_millis: 500,
                timeout_s: 10
            }
        );
        for (m, c, s) in [
            (Some(4096), None, None),
            (None, Some(2000), None),
            (Some(10), None, None),
            (None, Some(10), None),
            (None, None, Some(0)),
            (None, None, Some(MAX_TIMEOUT_S + 1)),
        ] {
            assert!(limits_for(&t, m, c, s, 60).is_err(), "{m:?} {c:?} {s:?}");
        }
    }

    #[test]
    fn mounts_are_unique_and_never_hide_what_the_runner_adds() {
        let n = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            mount_names(&n(&["orders", "orders", "job", "tables"])),
            n(&["orders", "orders_2", "job_2", "tables_2"])
        );
    }

    #[test]
    fn secrets_are_kept_as_hashes_and_text_is_cut_on_a_boundary() {
        assert_eq!(
            hash_secret("a"),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
        assert_eq!(cap("héllo", 2), "h…");
        assert_eq!(cap("short", 10), "short");
    }

    #[test]
    fn a_job_is_named_by_its_comment_or_nothing() {
        assert_eq!(job_title("Daily summary", "print(1)"), "Daily summary");
        assert_eq!(
            job_title("  ", "\n# Totals by region\nimport csv"),
            "Totals by region"
        );
        assert_eq!(job_title("", "import csv\n# later"), "Untitled job");
        assert_eq!(job_title("", "#"), "Untitled job");
        assert_eq!(job_title("", ""), "Untitled job");
    }

    #[test]
    fn terminal_states_are_the_ones_that_end() {
        for s in STATES {
            assert_eq!(is_terminal(s), !matches!(s, "queued" | "running"), "{s}");
        }
    }
}
