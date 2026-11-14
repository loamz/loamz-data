# Job schedules and alerts

## Phase 1: think

### Goal and done

Save a SQL statement or job definition in a workspace schedule, run it as the
person who created it, and reuse the normal compute authorization and narrowing
path every time it fires. Cron and interval schedules must survive restarts,
coalesce downtime into one run, avoid overlapping runs for one schedule, and
be claimed once across server instances. A lost input permission pauses the
schedule rather than reusing old group claims. Failed and timed-out scheduled
runs leave a durable alert record and can notify a webhook whose URL is held
only in the vault.

Done means create/list/pause/resume/delete and alert state are routed and
documented; SQLite and Postgres tests cover due-time claims, rights, and
retries; tests prove a restart/double worker cannot duplicate one firing; and
the full repository and Postgres checks pass.

### Starting points and decisions

1. **Worker and exactly-once handoff:** extend `governance/lifecycle`'s durable
   task worker, not a detached timer that directly submits work. Store each
   due firing as a task in the same transaction that advances the schedule's
   next time. Use a unique task-to-run key on `runs` and insert it atomically
   with the run and its audit row. A retry then finds the existing run instead
   of queueing a duplicate. Rejected: run SQL directly in a new scheduler loop,
   which would lose work across restart and duplicate after a crash between
   queueing and acknowledging.
2. **Time and overlap:** cron expressions use UTC; fixed intervals are at
   least one minute. After downtime or a long run, dispatch one overdue
   occurrence and advance to the first future occurrence. Do not start a
   second run while the previous run is queued/running; coalesce its missed
   ticks. Rejected: local timezone support and catch-up bursts, which add DST
   ambiguity and can overwhelm customer compute.
3. **Run identity and rights:** save the creator's person subject, not a bearer
   token or stale group list. On each firing, load current groups from the
   connected `Directory`, build the normal workspace context, and call
   `submit_sql`/`submit_job`. If an input is no longer visible/authorized,
   pause and audit the schedule; other transient failures use task retry.
   Rejected: replay the groups captured at schedule creation, because leaving
   a group would keep stale access.
4. **Alerts:** store an optional vault `SecretRef`, never a webhook URL or its
   reference in API responses. Record each failure in an outbox with a stable
   alert id, then send through HTTPS only, with redirects disabled, a short
   timeout, DNS results pinned after rejecting non-public addresses, and
   bounded retry/backoff. Coalesce pending notifications per schedule so a
   fast-failing schedule cannot send an alert storm; retain all failure rows.
   Rejected: direct webhook calls in the run-report path, which could block
   compute and lose notifications on restart.

### Attack and failure inventory

| Risk | Where it appears / test direction |
|---|---|
| An invalid cron expression schedules at an unexpected time | Lifecycle schedule model parser and create validation. |
| Zero, negative, or excessively short intervals create a hot loop | Interval validation; reject under 60 seconds and test boundaries. |
| Cron is accidentally interpreted in the host timezone | Cron-to-epoch conversion; fixed UTC test timestamps. |
| A DST transition fires twice or skips a run | UTC-only rule; test across a DST boundary. |
| Multiple missed ticks cause a restart burst | Due-time advancement; advance several intervals and require one task. |
| Two server instances claim the same due occurrence | Conditional schedule update and durable task insert; concurrent SQLite/Postgres test. |
| A crash occurs after task insert but before schedule advancement | Same transaction for both; transaction rollback test. |
| A crash occurs after run insert but before task acknowledgement | Unique task/run key; retry must return the same run id. |
| A queued run is counted as overlap and prevents future runs forever | Active-run detection must clear on every terminal state. |
| Two concurrent runs of one schedule consume all capacity | At most one queued/running schedule run; test overlap. |
| Schedule executes with the creator's stale group membership | Refresh groups at firing; remove group grant and assert pause. |
| A direct input grant is revoked after schedule creation | Reuse submit-time authorization; pause on hidden/not-authorized input. |
| A workspace is deleted or made inaccessible | Re-open workspace on every fire and pause instead of running with stale context. |
| Creator identity is a service principal or acting agent | Reject schedule creation unless it is a person acting directly. |
| A token, group list, or temporary credentials are persisted | Store only subject and definition; inspect schema and persisted rows. |
| Schedule SQL bypasses SQL parsing, policy rewrite, or row limits | Submit via `compute::service::submit_sql`; inspect queued run spec. |
| Job inputs/output bypass ordinary grants or classification | Submit via `submit_job`; revoke access and test pause. |
| User changes the saved SQL/job after authorizing it | Store an immutable schedule definition; no mutable update endpoint. |
| One schedule can pause/delete another person's schedule | Scope list and mutation authorization to creator or workspace admin; 404 test. |
| A secret ref or webhook value is returned by list/create | API view exposes only `alerts_enabled`; assert neither value appears. |
| A webhook redirects to loopback/private metadata | Disable redirects and reject any non-public resolved address; unit tests. |
| DNS rebinding changes a public hostname to a private address | Resolve once and pin vetted addresses in the request client. |
| A slow webhook blocks run execution or worker capacity | Separate outbox delivery, short HTTP timeout, retry later. |
| Webhook error text logs or stores credentials/URL | Replace transport errors with a generic safe reason. |
| Repeated scheduled failures cause an alert storm | Persist all failures but coalesce outbound sends with a per-schedule cooldown. |
| A webhook accepts a request but the response is lost, causing duplicate alert | Stable idempotency key; document delivery as at-least-once. |
| Server shutdown occurs while webhook send is in progress | Alert lease expires and a retry is safe. |
| A deleted/paused schedule still fires queued tasks | Check current schedule state when task is handled; discard stale task. |
| An erased owner leaves private SQL, a webhook ref, or pending scheduled tasks behind | Clear and tombstone schedule ownership and cancel its pending alert/task references in the existing erase transaction. |
| SQLite and Postgres differ on conditional updates/unique NULL behavior | Run the same due, retry, and concurrent-claim tests on both backends. |

### Runtime facts that reading cannot establish

- Whether the database's conditional-update/transaction behavior really
  serializes two workers under SQLite and Postgres.
- Whether a killed worker leaves exactly one queued run after its task lease
  expires and is reclaimed.
- Whether the existing compute submission path fully rechecks authorization
  and policy for stored SQL and job definitions.
- Whether DNS pinning and no-redirect settings prevent a local redirect or
  rebinding request in the deployed reqwest/TLS stack.
- Whether repeated failure delivery remains bounded when the webhook times
  out while more runs fail.

## Phase 2: test-first evidence

Added `server/server/tests/schedules.rs` before production changes. The focused
`scripts/heavy.sh cargo test -p loamz-data-server --test schedules -- --nocapture`
compiled and failed for the expected reason: the new schedule create route
returned the problem response `404 no such route` for lifecycle, validation,
dedupe, permission-loss, and alert cases. The Postgres variants were ignored
in this first run because `LOAMZ_TEST_POSTGRES_URL` was not set.

## Phase 3: build

- Added UTC cron and bounded interval schedules with owner/admin list and mutation routes. Definitions stay immutable and are never
  returned. The saved person is rebuilt at firing with fresh directory groups, then passed through the existing SQL/job submitters.
- The lifecycle worker conditionally advances a due schedule and inserts its durable task in one transaction. Normal compute queueing
  links the run to that task in its run/audit transaction. Pending tasks and active runs suppress overlapping firings; missed ticks
  coalesce to the next future cadence.
- Losing workspace/input authorization pauses and audits the schedule. Other transient failures use lifecycle retry.
- Failed/timed-out runs create durable alert rows. Delivery reads the destination only from the vault, requires HTTPS, rejects
  non-public DNS answers, pins the selected public address, disables redirects, and times out after five seconds. Per-schedule
  cooldown and bounded retries limit storms; the webhook gets only identifiers/status.
- Updated the lifecycle/OpenAPI and governance design documentation. Test support now has a fake URL for its fake webhook secret;
  no test performs an outbound request.

## Phase 4: hostile review

| Attack | Result | Test | Fix |
|---|---|---|---|
| Erase a schedule owner, then inspect persisted SQL, owner, queued tasks, and webhook refs | Broke before review fix | `erasing_the_owner_clears_the_saved_definition_and_webhook_reference` | In the erase transaction, tombstone/delete schedules, blank definitions and refs, cancel their queued tasks and pending alerts. |
| Use loopback/link-local/reserved IPv6 webhook destinations, or stall DNS resolution | Broke before review fix | `webhook_dns_rejects_non_public_addresses`, `webhook_client_refuses_loopback_without_sending` | Reject non-public answers including special-use IPv6, pin the vetted address, and bound DNS and HTTP timeouts. |
| Create, resume, or fire a schedule while its owner is being erased | Broke before review fix | `erasing_the_owner_clears_the_saved_definition_and_webhook_reference` | Serialize creation and state changes against erase on the organization row, pause schedules with the erase request, and reject submission if the pending erase guard is set. |
| Race resume against delete so a deleted schedule becomes active again | Broke before review fix | `schedules_can_be_managed_without_returning_the_webhook_reference` | Update schedule state only while it is not deleted; a stale mutation reports 404 and cannot resurrect it. |
| Pause/delete after a due task was queued | Held | `a_paused_schedule_discards_an_already_queued_firing` | The worker checks current schedule state; queueing also locks the schedule row against pause/delete. |

No unresolved implementation defect was found in the review. A live successful webhook exchange is not exercised: tests intentionally
avoid external network calls, and cover URL/DNS refusal, outbox creation, and retry exhaustion locally instead.

The required `make check` initially exposed pre-existing strict Clippy failures in
`server/runner/src/door.rs`. Applied behavior-preserving lint fixes there so the
repository-wide check could complete.

Final validation:

- `nice -n 19 scripts/heavy.sh make check` — passed; final web test summary:
  `Test Files  2 passed (2)`, `Tests  25 passed (25)`.
- `LOAMZ_TEST_POSTGRES_URL=... nice -n 19 scripts/heavy.sh cargo test -p loamz-data-server -- --include-ignored --skip openbao_live --test-threads=4`
  — passed; schedule suite: `24 passed; 0 failed`, and doc tests: `0 passed; 0 failed`.
