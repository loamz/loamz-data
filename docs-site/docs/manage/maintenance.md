# Table Maintenance

Table maintenance keeps managed Apache Iceberg tables healthy, fast, and storage-efficient over time. Loamz Data provides server-managed compaction to consolidate fragmented small data files and snapshot expiry to purge obsolete table history.

---

## Overview

Frequent write and append operations create many small Parquet files and deep snapshot history. Loamz Data exposes dedicated maintenance actions executed safely on workspace compute:
- **Compaction**: Combines multiple small data files into fewer, larger Parquet files without altering table schema or row contents.
- **Snapshot Expiry**: Prunes historical snapshots older than a configured age, removing orphan data and manifest files from object storage while preserving active table states.

---

## Safety & Security Model

### Strict Access Requirements
- **Privileges**: Triggering maintenance requires the `MODIFY` privilege on the target table.
- **Rule Isolation**: If the caller is subject to any row filter or column masking rule on the target table, the maintenance request is **refused**. This prevents callers from bypassing data masking through raw-file rewrite operations.
- **Fixed Server Script**: Maintenance runs a deterministic, server-generated script (`maintenance_job.py`). Callers cannot supply custom Python or arbitrary SQL statements.

### Compaction Verification
Compaction runs inside an explicit transaction:
1. DuckDB rewrites fragmented Parquet files using `iceberg_rewrite_data_files`.
2. Before committing, the worker verifies that the **total row count** and an **order-independent content checksum** of all rows match the original table exactly.
3. If row counts differ or any byte checksum mismatches, the transaction is rolled back immediately and no new snapshot is published.
4. If another writer committed to the table during compaction, the catalog's compare-and-set detects the conflict and rolls back safely.

### Safe Snapshot Expiry
Snapshot expiry follows a two-stage commit-first model:
1. **Metadata Commit First**: The worker determines snapshots older than the retention threshold (`older_than` seconds). It always retains:
   - The current active snapshot.
   - Any snapshot referenced by an active tag or branch.
   - All manifest lists and manifests reachable from retained snapshots.
2. The catalog commits the `remove-snapshots` metadata change with an expected-current-snapshot precondition.
3. **Object Deletion Second**: Only after the catalog commit succeeds does the worker delete unreferenced data files and manifest files from storage. If a commit conflict or error occurs, no files are deleted.

---

## Running Maintenance via the API

### Trigger Compaction
Queue a compaction run on any table by its securable ID:

```sh
curl -X POST "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/tables/<table-id>/maintenance" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "action": "compact"
  }'
```

Returns `202 Accepted` with the queued run descriptor. Monitor run progress through the standard compute run API:

```sh
curl -s "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/compute/runs/<run-id>" \
  -H "Authorization: Bearer $TOKEN"
```

### Trigger Snapshot Expiry
Expire snapshots older than a specified duration (in seconds). For example, to expire snapshots older than 14 days (1,209,600 seconds):

```sh
curl -X POST "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/tables/<table-id>/maintenance" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "action": "expire",
    "older_than": 1209600
  }'
```

The `older_than` parameter defaults to 7 days (604,800 seconds) if omitted. Values must be between 1 second and 365 days (31,536,000 seconds).

---

## Scheduled Table Maintenance

Table maintenance can be automated on a recurring cadence using workspace schedules:

```sh
curl -X POST "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/schedules" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "weekly-trips-compaction",
    "cron": "0 4 * * 0",
    "maintenance": {
      "table": "nyc.main.trips",
      "action": "compact"
    }
  }'
```

Each scheduled execution validates caller permissions and table policy afresh before submitting the job.

---

## What is Not Built Yet

- **Console UI Actions**: Triggering compaction and snapshot expiry directly from table pages in the web console is not built; maintenance is initiated via the REST API or schedules.
- **Orphan File Sweep after Crash**: If a worker crashes after the metadata commit but before every unreferenced object file is deleted, the remaining unreferenced files become safe orphans; a persistent deletion retry queue is not built.
- **Custom Compaction Tuning**: File target size tuning, sort-order rewrites, and partition-specific compaction strategies use fixed engine defaults.
- **Catalog-Wide Expiry Policies**: Automatic expiration policies applied at catalog or schema granularity without per-table schedule setup are not built.
