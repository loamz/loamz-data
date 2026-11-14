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

Same machine and data (11.2 million trips). The machine column is a SQL statement through the API, run by one warm worker per
organization and target inside gVisor, reading through the storage broker. Seconds, best of three; the first run is in brackets.

| query | catalog warm | machine before | machine now | queued | worker prepares | worker runs |
|---|--:|--:|--:|--:|--:|--:|
| point | 0.13 | 1.98 | **0.28** (0.63) | 9 ms | 16 ms | 165 ms |
| filtered aggregate | 0.11 | 1.70 | **0.22** (0.56) | 15 ms | 11 ms | 125 ms |
| group by | 0.05 | 1.83 | **0.19** (0.22) | 12 ms | 9 ms | 81 ms |
| join | 0.12 | 1.85 | **0.29** (0.45) | 12 ms | 15 ms | 210 ms |
| full scan | 0.09 | 1.54 | **0.25** (0.31) | 14 ms | 11 ms | 130 ms |

The machine path is 5 to 9 times faster than the baseline and under the 0.5 s gate for every query, on WSL2, where gVisor can only use
its slow `ptrace` mode. What changed, in order of effect:
- **No sandbox start and no table download per statement:** one warm worker is reused (about 1.4 s).
- **One DuckDB reused across statements:** extensions loaded once, each statement on its own connection with its temporary objects
  removed afterwards (prepare fell from about 320 ms to 10 to 16 ms).
- **The wait to be leased fell from about 350 ms to under 15 ms:** the agent and the waiting request are woken the moment work is
  queued or a run ends, instead of polling.
- **The storage broker keeps connections open and remembers files that never change** (Iceberg metadata, manifests, Parquet footers
  and small files), so repeat reads come from memory.

The gap to reading the bucket directly (the "direct" and "catalog" columns) is now about 0.1 s: the run through the sandbox's network
bridge. Not measured yet: bytes and requests, CPU, memory, a larger table, a customer's remote bucket (network time will dominate).

## Agent sandbox reads through the broker (`bench/broker_readers.py`, 14 Nov 2026)

Can Polars, DuckDB and pyarrow read Parquet through the runner's storage broker, as a code sandbox would (a loopback port in front of
the broker's Unix socket, path-style, plain http, placeholder keys)? RustFS on this machine, a 233 MB file of 30 row groups and a
23 MB file. Not run inside gVisor yet (phase 2 of proposal 0006); the broker and the readers are the real ones.

| read | bytes the broker served | cold | warm (from cache) | local disk, Polars |
|---|--:|--:|--:|--:|
| `count(*)` (footer only) | 0.13 to 0.27 MB | 0.02 to 0.03 s | 0.01 s | 0.009 s |
| one column of four (`val`) | 24 MB of 233 MB | 0.11 to 0.14 s | 0.05 to 0.08 s | 0.011 s |
| `WHERE id < 1000` (row groups pruned) | 1.3 to 1.5 MB | 0.04 s | 0.01 to 0.02 s | 0.004 s |

- All three readers work unchanged. A column or a filter moves a fraction of the file: pushdown works through the broker.
- A repeat read is served entirely from the broker's cache (cache hits equal requests), and file size does not matter: ranges of the
  large file are cached too, not only whole small files.
- Isolation, with any signature: the leased file 200; another folder and the bucket root 403; write and delete 403; `..` 400.
- The `bytes` counter counts bytes served to a reader, cached or not, so storage traffic is `hits` against requests. Not measured
  yet: the same through gVisor, many concurrent readers, a remote bucket, and the sandbox holding no credential.
- Found for the build: the broker is authorized by a table list today. A code sandbox needs it authorized by folders (a volume's thread
  folder), the region from the lease instead of `auto`, and an optional session token (done).
