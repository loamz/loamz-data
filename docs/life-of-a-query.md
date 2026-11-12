# Life of a query

One SQL statement from the SQL editor to its rows, as the code works today (12 Nov 2026). Paths are under `server/server/src`
unless they start with `web/` or `server/runner`. What changes next: [RFC 001](rfcs/001-fast-data-plane.md).

## 1. The browser sends it
`web/src/screens/SqlEditor.tsx::run` refuses if no running compute is picked, then `web/src/api.ts::runStatement` sends
`POST /api/v1/workspaces/{ws}/sql/statements` with `{sql, limit, wait_s: 0, target}`. It then asks for the run every 350 ms
(`api.ts::run`) until it has ended.

## 2. Who is asking
`compute/api.rs::statement` takes a `WsCtx` (`governance/context.rs`). Building it checks the bearer token
(`kernel/auth.rs::Verifier::verify`, refreshing the signing keys once on an unknown key) and opens the workspace with the caller's
standing (`workspaces::service::open`).

## 3. May they run it
`compute/service.rs::submit_sql`:
- The caller needs the workspace entitlement `sql-access`; a refusal is audited.
- `compute/sql.rs::analyze` accepts exactly one plain query, with every table named `catalog.schema.table`, and refuses file reads.
- Each table passes `readable_table`: resolve it in the catalog, then `iam::require(Select)`. Missing and forbidden give the same 404.
- `sealed` data is refused; anything not `normal` is marked private, which keeps it off serverless compute.

## 4. Where it will run
`pick` chooses the compute: the one asked for, else the first running one big enough, the organization's machines before serverless.
A stopped serverless target answers 409 "Start it first".
`queue` then, in one transaction: locks the organization's row and counts its active runs (`store::has_capacity`; full means 429),
inserts the run, audits `sql.submit`, and wakes the dispatchers.

## 5. Handing it over
- **Machine:** the runner long-polls `POST /api/v1/agent/lease` (`compute/api.rs::lease`). `service::lease` claims the oldest queued
  run, re-checks access as it is now (`authorize_lease`), and adds read-only credentials for exactly its tables' folders
  (`storage_for` → `storage/tables::vend`).
- **Serverless:** `compute/serverless.rs::serve_forever` leases the run in-process and `call`s the endpoint with the SQL, the tables'
  metadata locations and the same credentials, heartbeating every 20 s.

## 6. Running it
- **Machine** (`server/runner`): `agent.rs::run_one` heartbeats every 15 s; `work.rs::execute` downloads each table's files
  (`fetch_table`), writes the statement and `run_sql.py`, and starts gVisor with no network and fixed limits (`spec.rs::oci_config`,
  checked by `check.rs::verify`). Inside, `run_sql.py` opens DuckDB (one thread), creates a view per table over `iceberg_scan`, locks
  DuckDB down and writes the rows as JSON.
- **Serverless:** `server/serverless/modal_app.py::sql` checks its token, does the same lockdown, reads straight from the bucket and
  answers with the rows and its timings.

## 7. The result
`POST /api/v1/agent/runs/{id}/complete` → `service::complete`: in one transaction it finishes the run, deletes its token and audits
`run.finished`. The browser's next `GET /runs/{id}` (`api.rs::get_run`) returns it only to the person who asked, the person it was
done for, or an admin, and shows the rows only if they may still read every table (`require_detail`).

## Where the time goes
Steps 1 and 6 poll; step 6 downloads whole tables and starts a sandbox, Python and DuckDB per statement; results are JSON.
RFC 001 removes each of these.
