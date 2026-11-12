# 001 Fast data plane: tasks

Do in order. After each task, rerun the benchmark and record the result in the commit message.

| # | Task | Done when | Check |
|---|---|---|---|
| 1 | Benchmark `bench/run.py`: NYC week (then a year), 6 queries (point, filtered aggregate, group-by, join, full scan, rewrite), cold / warm / repeated, machine and Modal, plus plain `iceberg_scan` | a table of every metric per query | `bench/run.py` |
| 2 | Rust worker with embedded DuckDB, warm per tenant, all granted cores; Modal runs the same worker | no Python on the query path; same results | `cargo test -p loamz-runner`, `scripts/compute-smoke.py`, bench |
| 3 | Arrow IPC results streamed to the SQL editor and clients | no JSON per value; first rows before the query ends | web tests, bench time to first row |
| 4 | `data_files` written at commit, in the pointer's transaction; pruning at submit | selective queries read fewer files; results equal `iceberg_scan` | tests on SQLite and Postgres, bench bytes read |
| 5 | Ranged reads of planned files only | `fetch_table` deleted; bytes moved ≈ bytes needed | bench bytes and requests |
| 6 | Notifications instead of polling; per-organization lock; one authorization read; credential cache | no sleep loops left on the query path | tests, bench planning time |
| 7 | Sort on write, compaction, snapshot expiry (removes `data_files` rows) | file count and small files stay bounded | lifecycle tests, bench after a week of writes |
