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
    /// `subject` is the id of a schedule to fire.
    ScheduleRun,
}

impl TaskKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Erase => "erase",
            Self::ScheduleRun => "schedule.run",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "drop" => Some(Self::Drop),
            "erase" => Some(Self::Erase),
            "schedule.run" => Some(Self::ScheduleRun),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub id: String,
    pub org_id: String,
    pub workspace_id: String,
    pub name: String,
    pub owner: String,
    pub cadence_kind: String,
    pub cadence: String,
    pub definition: String,
    pub webhook_ref: String,
    pub state: String,
    pub next_run_ms: i64,
    pub next_alert_ms: i64,
    pub alert_lease_until: i64,
    pub last_error: String,
    pub created_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub id: String,
    pub schedule_id: String,
    pub run_id: String,
    pub state: String,
    pub attempts: i64,
    pub created_ms: i64,
    pub sent_ms: i64,
    pub last_error: String,
}

/// Converts a five-field UTC cron expression to the parser's seconds/minutes/hours/day/month/weekday/year form.
pub fn cron_expression(expression: &str) -> Result<String, &'static str> {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    let expanded = match fields.as_slice() {
        [minute, hour, day, month, weekday] => {
            format!("0 {minute} {hour} {day} {month} {weekday} *")
        }
        [second, minute, hour, day, month, weekday] => {
            format!("{second} {minute} {hour} {day} {month} {weekday} *")
        }
        [_, _, _, _, _, _, _] => expression.to_owned(),
        _ => return Err("cron must have five, six, or seven fields"),
    };
    expanded
        .parse::<cron::Schedule>()
        .map_err(|_| "cron expression is not valid")?;
    Ok(expanded)
}

/// Returns the next UTC firing after `after_ms`.
pub fn next_fire_ms(kind: &str, cadence: &str, after_ms: i64) -> Result<i64, &'static str> {
    match kind {
        "interval" => {
            let seconds = cadence
                .parse::<i64>()
                .map_err(|_| "interval must be a whole number of seconds")?;
            if !(60..=31_536_000).contains(&seconds) {
                return Err("interval must be between 60 seconds and 365 days");
            }
            after_ms
                .checked_add(seconds * 1000)
                .ok_or("next interval is out of range")
        }
        "cron" => {
            let expression = cron_expression(cadence)?;
            let schedule = expression
                .parse::<cron::Schedule>()
                .map_err(|_| "cron expression is not valid")?;
            let after = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(after_ms)
                .ok_or("cron start time is out of range")?;
            schedule
                .after(&after)
                .next()
                .map(|next| next.timestamp_millis())
                .ok_or("cron has no future firing")
        }
        _ => Err("schedule cadence must be cron or interval"),
    }
}

/// Advances an overdue interval from its prior due time, coalescing any missed periods.
pub fn next_after_due(
    kind: &str,
    cadence: &str,
    due_ms: i64,
    now: i64,
) -> Result<i64, &'static str> {
    if kind == "interval" {
        let seconds = cadence
            .parse::<i64>()
            .map_err(|_| "interval must be a whole number of seconds")?;
        if !(60..=31_536_000).contains(&seconds) {
            return Err("interval must be between 60 seconds and 365 days");
        }
        let period = seconds * 1000;
        let periods = (now.saturating_sub(due_ms) / period).saturating_add(1);
        due_ms
            .checked_add(period.saturating_mul(periods))
            .ok_or("next interval is out of range")
    } else {
        next_fire_ms(kind, cadence, now)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub org_id: String,
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
    pub alert_ref: String,
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
        for k in [TaskKind::Drop, TaskKind::Erase, TaskKind::ScheduleRun] {
            assert_eq!(TaskKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(TaskKind::parse("other"), None);
    }

    #[test]
    fn cron_uses_utc_and_intervals_coalesce_missed_ticks() {
        assert_eq!(next_fire_ms("cron", "0 9 * * *", 0), Ok(9 * 60 * 60 * 1000));
        assert_eq!(
            next_after_due("interval", "60", 1_000, 301_000),
            Ok(361_000)
        );
        assert!(next_fire_ms("interval", "59", 0).is_err());
    }
}
