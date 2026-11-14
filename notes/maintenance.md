# Table maintenance

## Goal and done

Let a table's authorized modifier compact its data files or expire old
snapshots through the existing compute write path. Compaction must compare
row count and a content checksum before making its new snapshot visible.
Snapshot expiry keeps the current snapshot and any snapshot still referenced
by a branch or tag, then removes only files not referenced by any snapshot
that remains. A crash may leave unreferenced objects, but must not change the
table before its catalog commit.

Done means the new workspace table-maintenance route returns a queued run,
its run is a fixed server-authored job scoped to the named table, and neither
people without MODIFY nor people subject to that table's row/column rules can
queue it. A maintenance schedule uses the same validation at creation and
again when it fires. SQLite and Postgres integration tests cover the route and
authorization; focused logic tests cover checksum mismatch and live-file
retention. The compute smoke test demonstrates compact and expire on RustFS
through gVisor. OpenAPI and the write-path proposal describe the feature.

## Decisions

1. **Use a fixed maintenance script and the existing job path.** Generate a
   server-owned script from validated JSON values and submit it with exactly
   one `table_writes` entry. Reuse `submit_job` so MODIFY, row/column rules,
   compute policy, audit, lease-time authorization and scoped storage rights
   are not duplicated. Rejected: accept SQL or Python from the caller; either
   would turn a maintenance endpoint into arbitrary compute.
2. **Stage, check, then commit compaction.** Run DuckDB's
   `iceberg_rewrite_data_files` inside one explicit transaction, compare
   before/after row counts and a stable, order-independent content checksum,
   and commit only on equality. Roll back on mismatch. Rejected: verify after
   an autocommit, because a bad snapshot would already be visible. The real
   gVisor/RustFS smoke must prove the extension and catalog really defer the
   commit until the transaction commits; the local DuckDB extension is absent.
3. **Expire metadata before deleting objects.** Use an expected-current-snapshot
   requirement and the catalog's `remove-snapshots` update, preserving the
   current snapshot and all snapshots named by refs. Then compare object paths
   reachable from every remaining snapshot and delete only unreferenced
   data/manifest objects. Rejected: delete first; a commit conflict or crash
   would leave a still-live snapshot pointing at missing data.
4. **Make scheduled maintenance a first-class saved definition.** Extend the
   schedule draft with one typed maintenance target/action, and dispatch it
   through the same submission helper on every fire. Rejected: store a
   generated script as an ordinary user job; that makes server-owned intent
   forgeable and risks skipping fresh table authorization.
5. **Define `older_than` as seconds.** It is an optional positive integer,
   defaulting to 604800 and capped at 365 days. The current snapshot is always
   kept. Rejected: interpret a date string or an unbounded duration, which is
   ambiguous across time zones or can remove nearly all history by mistake.

## Attack list

| Attack or failure | Where it must be handled |
|---|---|
| Modify a table in another workspace by guessing its ID | Maintenance context and table lookup in `compute/service.rs`; table path in `storage/tables/service.rs`. |
| Queue work without `MODIFY` | `checked_table_writes` and `iam::require` in `compute/service.rs`. |
| Bypass row or column rules by reading raw files | `catalogs::is_restricted` in `checked_table_writes`; repeat at lease time. |
| Name a view, schema, volume, or missing table | Maintenance request validation and `checked_table_writes`. |
| Use an unknown action or typo to select arbitrary SQL | Request validation in `compute/service.rs`; fixed script dispatch. |
| Supply zero, negative, huge, or malformed `older_than` | Request parsing and duration bounds in `compute/service.rs`. |
| Inject a table name or action into Python or SQL | Validate three table-name components; JSON-encode the embedded spec; quote identifiers. |
| Run a client-provided script through the maintenance route | Fixed `maintenance_job.py`; never accept a script field. |
| Run as someone whose rights were revoked after queueing | Existing lease-time table-write revalidation in `compute/service.rs`. |
| Schedule maintenance and later lose MODIFY | Lifecycle schedule firing calls maintenance submission again; invalid access pauses it. |
| Schedule a table with rules and wait for a later raw-file access | Schedule creation and firing both use maintenance validation. |
| Reordering rows makes a correct compaction look different | Order-independent per-row content hash and explicit count in the fixed script. |
| A checksum mismatch is discovered after catalog commit | Transaction boundary around rewrite, checksum comparison, and commit. |
| A crash occurs after new Parquet objects are written but before commit | Staged transaction; unreferenced objects may remain but old metadata stays current. |
| Another writer commits while compaction is running | Catalog compare-and-set on the base snapshot; conflict rolls back and leaves the newer table intact. |
| Expire removes the current snapshot | Expiry plan always includes the current snapshot in its keep set. |
| An old tag or branch still references a snapshot | Keep every snapshot named by a ref; catalog also rejects removing a referenced snapshot. |
| Delete a data file shared by a retained snapshot | Subtract paths reachable from all retained snapshots before deletion. |
| A new commit races between expiry planning and metadata commit | Require the planned current `main` snapshot; only delete after successful catalog commit. |
| Crash after metadata expiry but before object deletion | Safe for live data, but the current run can leave orphan objects that a later expiry cannot identify after their snapshots have been removed. |
| Delete metadata still reachable from the current metadata log | Keep current metadata, version hint target, metadata-log targets, and all live manifests. |
| Delete outside this table's storage prefix | Storage door's write-prefix check must reject it; never broaden a lease. |
| A bucket/list response contains an unsafe path or another table's object | Validate the returned key stays under the exact table folder before issuing DELETE. |
| Multiple old snapshots or thousands of files exhaust the sandbox | Bound work by the existing run memory/time limits; fail before commit on an exceeded limit. |
| The maintenance refusal leaks whether a hidden table exists | Use the existing hidden table lookup and RFC 9457 errors. |
| Denial or commit is absent from the audit trail | Reuse the normal job and catalog commit Uow/audit paths. |
| SQLite and Postgres differ on compare-and-set/transactions | Run the same API and commit tests on both backends and the live smoke against the actual catalog. |

## Facts that require a run

- The local Python environment has DuckDB 2.0.0 but not its Iceberg extension at
  `/opt/duckdb-extensions`; loading it failed because the extension file is
  absent. The gVisor rootfs smoke must exercise the actual Iceberg functions.
- Documentation lists `iceberg_rewrite_data_files` but no snapshot-expiry
  function. The catalog already understands `remove-snapshots`, but the
  direct REST commit and its expected-current-snapshot condition need a
  RustFS/gVisor run.
- A real run must prove DuckDB keeps the rewrite uncommitted until the explicit
  transaction commits, and that DELETE through the existing table write lease
  works only inside that table's prefix.

## Hostile review

| Attack | Result | Test | Fix |
|---|---|---|---|
| Scan the table after `iceberg_rewrite_data_files` but before commit; DuckDB 2.0 returns the old files plus the staged replacement files. | Broke the original equality check (4 rows became 8); held after the fix. | `test_checksum_mismatch_rolls_back_without_committing`, `test_matching_rewritten_rows_commit`, and the RustFS/gVisor compute smoke. | Checksum only the newly written Parquet files from a table-scoped object listing, then commit only on equality. |
| Parse Iceberg metadata by casting its DuckDB struct to `VARCHAR`; the value is not JSON. | Broke snapshot expiry in the live smoke; held after the fix. | RustFS/gVisor compute smoke: `expiry kept the live snapshot readable`. | Serialize metadata with DuckDB's `to_json` before `json.loads`. |
| Assume DuckDB 2.0 names the `iceberg_manifests` file column `manifest_path`. | Broke snapshot expiry in the live smoke; held after the fix. | `test_expiry_collects_live_data_and_manifest_paths` and the RustFS/gVisor compute smoke. | Read the actual `path` column and keep the manifest-path check fail-closed. |

### Remaining limitation

If a process crashes after the metadata commit for expiry but before every
object deletion, the remaining objects are safe orphans. This version does not
persist a deletion queue, so a later run cannot recover paths that belonged
only to snapshots already removed. The current deletion order is retained
because deleting first could break a snapshot if the catalog compare-and-set
fails.

The live run confirmed that the existing storage door allows DELETE inside the
declared table write prefix and refuses DELETE outside it; no door change was
needed.

## Test-first plan

- Add SQLite/Postgres API cases for successful queueing, missing MODIFY,
  rule-based refusal, hidden/missing/non-table targets, invalid action/window,
  and maintenance schedules. Verify the run has only the requested table in
  `table_writes` and contains the fixed script, not caller code.
- Add focused tests for checksum mismatch causing rollback/no commit and for
  expiry retaining the current/ref-referenced files while deleting only
  unreferenced candidates.
- Extend `scripts/compute-smoke.py` to query the table before/after compaction,
  expire an old snapshot, and read the resulting table back. Capture its real
  gVisor/RustFS output.
