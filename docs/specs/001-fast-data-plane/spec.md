# 001 Fast data plane: spec

## Problem
A query today does far more work than it needs. Measured from the code (12 Nov 2026):

| Step | Today | Cost |
|---|---|---|
| Find files | DuckDB walks Iceberg metadata → manifest list → manifests on R2 | sequential round trips per table |
| Move data | the machine runner downloads every object of each table first (`runner/src/work.rs::fetch_table`) | whole table over the network |
| Start | per statement: gVisor, Python, DuckDB, `LOAD iceberg`; Modal cold start | 100s of ms to seconds |
| Execute | DuckDB with `threads=1` | one core |
| Return | every value turned into JSON in Python, 3.5 MB cap | slow, small results only |
| Wait | polling every 150 ms to 1 s (run wait, lease, serverless loop, SQL editor) | up to ~1 s idle |
| Govern | one process-wide write lock; grants read per check; credentials minted per run | serialized, repeated work |

## Goal
Interactive SQL and batch on the same fast path, for people, agents and apps, on any compute (machine or Modal).

## Requirements
1. A benchmark that measures every query end to end, by phase.
2. One Rust worker with embedded DuckDB, kept warm per tenant, using every granted core; Modal runs the same worker.
3. Results stream as Arrow to the browser and clients.
4. File metadata indexed in Postgres at commit; queries pruned with it before any read.
5. Workers read only the pruned files, in place, with ranged reads.
6. Governance off the hot path: notifications not polling, per-organization lock, one authorization read, cached credentials.
7. Tables kept fast: sort on write, compaction, snapshot expiry.

## Acceptance
- Every benchmark run records: planning time, time to first row, total time, bytes and requests to storage, CPU, peak memory,
  cache hits, cost per query; cold, warm and repeated; compared with plain DuckDB `iceberg_scan` on the same snapshot.
- Each requirement beats the previous run on the queries it targets, and returns identical results.
- A warm worker never serves two tenants or trust scopes; credentials cover only the planned files.

## Out of scope
Our own table format, engine, cache tier or storage engine; distributed scans; other sandboxes. Each needs benchmark evidence first.
