# Benchmark of the query path

The baseline every change to the query path has to beat ([proposal 0001](../docs/proposals/0001-fast-data-plane.md), task 1).
`bench/run.py [months]` loads the real NYC yellow taxi trips (the TLC's public Parquet files, three months by default) through the
catalog, checks that every way sees every row, then times five queries three ways, three times each (the first run is cold, the best of
the rest is warm).

| way | what it is |
|---|---|
| direct | DuckDB reads the table straight from the bucket with the bucket's own key: the best case, no governance |
| catalog | DuckDB attaches our REST catalog: the catalog's round trips, and credentials limited to the table's folder |
| machine | a SQL statement through the API, run on a compute machine in a gVisor sandbox (what the SQL editor does today) |

## Baseline: 11.2 million real trips (January to March 2025), 13 November 2026

One computer (WSL2), RustFS and the server on the same machine, so network time is close to zero. Seconds.

| query | direct cold | direct warm | catalog cold | catalog warm | machine |
|---|--:|--:|--:|--:|--:|
| point (`WHERE id = …`) | 0.17 | 0.11 | 0.17 | 0.11 | 1.98 |
| filtered aggregate | 0.17 | 0.09 | 0.16 | 0.08 | 1.7 |
| group by | 0.09 | 0.04 | 0.12 | 0.06 | 1.83 |
| join (with the 265-row zone table) | n/a | n/a | 0.14 | 0.08 | 1.85 |
| full scan | 0.11 | 0.03 | 0.12 | 0.05 | 1.83 |

What it says:
- **The catalog costs little here.** About 0.02 s warm over reading the bucket directly: governance on the plan is cheap on one
  machine. Over a real network every catalog round trip costs more, so this is a floor, not a promise.
- **The machine path is the slow one: about 1.8 to 2.0 s for any query,** 20 to 60 times a warm direct read. The time does not depend on
  the query, so it is the fixed cost: the statement waits 0.3 to 0.4 s to be leased, then about 1.4 to 1.6 s runs, which is starting a
  sandbox and downloading the whole table folder before DuckDB starts. That is what a warm engine and ranged reads (0001 tasks 2 and
  5) remove.
- **The one odd cell was the clock, not the query.** In earlier runs one query on the machine looked fast (0.2 to 0.7 s). WSL2 steps
  its wall clock back now and then, and the benchmark and the server both measured with it: one run even logged "finished" before
  "starting", and its `ran` time came out negative. The benchmark now uses a monotonic clock and the API clamps such differences at
  zero. With that, every query on the machine takes 1.5 to 2.3 s, so the cost is fixed, as above.

Not measured yet: bytes and requests, CPU, memory, cost, a year of data, Modal, and time to first row (results are not
streamed until 0001 task 3). Add them when the change that needs them is made.

`bench/results/*.json` keeps each run. Compare a change against the last one on the same machine; do not compare across machines.

## After the warm worker (13 November 2026, evening)

Same machine and data (11.2 million trips). The machine column is a SQL statement through the API, now run by one warm worker per
organization and target inside gVisor, reading through the storage broker (kept-open connections, a cache of the files that never
change). Seconds, best of three.

| query | catalog warm | machine before | machine now | queued | worker prepares | worker runs |
|---|--:|--:|--:|--:|--:|--:|
| point | 0.12 | 1.98 | 0.79 | 2 ms | 315 ms | 355 ms |
| filtered aggregate | 0.08 | 1.70 | 0.94 | 2 ms | 339 ms | 432 ms |
| group by | 0.04 | 1.83 | 0.72 | 3 ms | 319 ms | 278 ms |
| join | 0.07 | 1.85 | 0.84 | 3 ms | 340 ms | 376 ms |
| full scan | 0.05 | 1.54 | 0.78 | 2 ms | 339 ms | 303 ms |

What changed: the wait to be leased fell from about 350 ms to 2 ms (the agent is woken when work is queued); the sandbox and the table
download are gone (one worker is reused); and the files DuckDB reads again and again come from the broker's memory.

What is left, in order: **about 300 ms of every statement is the worker preparing**: opening a DuckDB instance (about 90 ms) and
loading the `httpfs` and `iceberg` extensions (about 220 ms). Reusing one instance for many statements, with each statement's
temporary objects removed afterwards, removes most of it. After that the query itself (280 to 430 ms) is the reads through the
sandbox's network bridge under gVisor's `ptrace` mode, the only mode WSL2 allows; on a normal Linux host (KVM or `systrap`) syscalls are
much cheaper, so the gate (under 0.5 s) is measured there. Not measured yet: bytes and requests, CPU, memory.

