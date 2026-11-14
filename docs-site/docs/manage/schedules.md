# Schedules and Failure Alerts

Loamz Data supports recurring scheduled execution of SQL statements, sandboxed jobs, and table maintenance actions. Schedules are evaluated durably by the workspace lifecycle worker and run under the identity and permissions of the person who created them.

---

## Overview

A schedule defines an automated recurring task configured with either:
- **A 5-part UTC cron expression**: Standard syntax evaluated strictly in UTC (e.g. `*/15 * * * *` for every 15 minutes, or `0 2 * * *` for daily at 02:00 UTC).
- **A fixed interval in seconds**: Positive integer specifying repetition delay (`interval_seconds`, minimum 60 seconds).

Each schedule contains an immutable execution definition:
- `sql`: A SQL query string executed through workspace compute (e.g. `INSERT INTO ... SELECT ...`).
- `job`: A full sandboxed Python or shell compute job specification.
- `maintenance`: A targeted table maintenance action (`compact` or `expire`).

---

## Execution Guarantees

### Creator Identity & Fresh Rights
Schedules execute as the person who created them, not as an unconstrained service identity or static token. Every time a schedule fires:
1. The server loads the creator's current directory group memberships from Keycloak.
2. The normal workspace access rules, table permissions, and column masks are re-evaluated.
3. If the creator has lost access to any required input table or target catalog, the schedule is automatically **paused** with an audit log entry rather than executing with stale privileges.

### Exactly-Once Claims & Overlap Prevention
- **Durable Task Queue**: The lifecycle worker claims due occurrences and schedules tasks in the database atomically.
- **Overlap Prevention**: If a prior scheduled execution is still queued or running on compute, the next firing is suppressed. Missed intervals during downtime coalesce into a single execution when service resumes.
- **No Storms**: After server restarts or extended offline periods, the scheduler advances directly to the next future cadence without queuing duplicate backlogged runs.

---

## Managing Schedules via the API

### Create a Schedule
Only directly authenticated people (not service principals) can create schedules:

```sh
curl -X POST "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/schedules" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "daily-aggregates",
    "cron": "0 3 * * *",
    "sql": "INSERT INTO analytics.marts.daily_trips SELECT date_trunc(\"day\", pickup_ts) AS day, count(*) AS trips FROM nyc.main.trips GROUP BY 1"
  }'
```

### List Schedules
List schedules owned by the current user, or all workspace schedules if caller is a workspace admin:

```sh
curl -s "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/schedules" \
  -H "Authorization: Bearer $TOKEN"
```

Response includes schedule ID, name, cadence, next due timestamp, state (`active` or `paused`), and last run status. Saved SQL and job definitions remain private and are not returned in responses.

### Pause and Resume
Pause a schedule to temporarily halt upcoming executions:

```sh
curl -X PUT "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/schedules/<schedule-id>/pause" \
  -H "Authorization: Bearer $TOKEN"
```

Resume a paused schedule. The next firing time is calculated from the current timestamp:

```sh
curl -X PUT "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/schedules/<schedule-id>/resume" \
  -H "Authorization: Bearer $TOKEN"
```

### Delete a Schedule
Permanently removes the schedule and cancels any pending unstarted tasks:

```sh
curl -X DELETE "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/schedules/<schedule-id>" \
  -H "Authorization: Bearer $TOKEN"
```

---

## Failure Alerts & Webhooks

When a scheduled run fails or times out, the server records a durable alert.

### Querying Recorded Alerts
Inspect recent failure alerts for any schedule:

```sh
curl -s "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/schedules/<schedule-id>/alerts" \
  -H "Authorization: Bearer $TOKEN"
```

### Vault-Custodied Webhook Notifications
Schedules can specify an optional `webhook_ref` (e.g. `secret/data/alerts#slack_url`) pointing to an HTTPS destination stored inside OpenBao:
- The webhook URL and its secret reference are write-only and are **never returned** by list or get API responses.
- Alert delivery uses an asynchronous outbox worker with exponential backoff and a 5-second timeout.
- **SSRF Defenses**: Webhook delivery enforces HTTPS, disables HTTP redirects, resolves DNS and verifies the IP address is public (loopback, private RFC 1918, link-local, and cloud metadata addresses are strictly refused), and pins the resolved IP for the HTTP exchange.
- **Storm Protection**: If repeated failures occur in rapid succession, outgoing alerts are coalesced with a per-schedule cooldown while retaining full durable alert logs in the database.

---

## What is Not Built Yet

- **Console UI for Schedules**: Schedules and failure alerts are managed via the REST API and SDK; a dedicated visual cron scheduler is not built into the web console.
- **Mutable Definitions**: Schedule SQL and job bodies cannot be edited in place; modifying a statement requires deleting and recreating the schedule.
- **Non-UTC Time Zones**: Cron schedules cannot specify local time zones or Daylight Saving Time offsets; all schedules evaluate in UTC.
- **Native Third-Party Alert Integrations**: Direct OAuth-managed integrations for Slack, Microsoft Teams, or PagerDuty are not built; alert notifications route through HTTPS webhook endpoints.
