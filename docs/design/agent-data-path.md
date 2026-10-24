# Agent data path: analyse big data from a code interpreter while moving almost none of it

- **Status:** accepted plan. Slice 1 is built; the rest is ordered below.
- **Applies to:** `loamz-server` (gateway, grants, SQL planner, workers) and `loamz-data` (the client the code imports).

## The problem

A code interpreter that analyses warehouse data usually does three things badly:

1. It downloads the result into the sandbox and processes it there. Past a few GB that fails, and past a few hundred MB it is slow.
2. It holds a credential the model can print.
3. It keeps no state on the data side, so every step of a multi-step analysis moves the intermediate data out and back in.

Both halves of this platform are ours, so we can design the path end to end instead of wrapping a client around somebody else's warehouse. The rule is:

> **Compute where the data is. Move answers, not rows. When bytes must move, move them by reference, once, columnar, and as close as possible.**

## The measure

Every design choice is judged by two numbers per question the agent answers:

- **bytes into the sandbox** (what the agent had to receive), and
- **bytes read from storage** (what the engine had to scan).

The goal is a bytes-into-sandbox figure that is orders of magnitude below bytes-scanned, and a bytes-scanned figure well below the table size. Both are recorded per run and tested against budgets, so a regression is a failing test, not a feeling.

## Levels of movement, cheapest first

| Level | What moves | Mechanism |
|---|---|---|
| 0. Nothing | nothing | Answer from metadata (row counts, min/max, null counts, distinct estimates from table statistics) |
| 1. An answer | a few rows | Pushdown: SQL, the lazy frame compiled to SQL, sketches, plot-sized aggregates |
| 2. A reference | a handle | Server-side scratch tables and result handles; the next step reads them on the engine |
| 3. A slice, near | Arrow batches over a local path | Same-host zero-copy handoff (memory-mapped Arrow file), never the network |
| 4. A slice, far | Arrow batches over the network | Parallel streams, compressed, with a hard per-run byte budget |

Anything that can be done at a lower level must be. The planner picks the level, not the model.

## Design

### 1. No credential in the sandbox
The sandbox gets one socket per run and nothing else. The gateway on the host owns the run's token and attaches it to each outgoing request. A request's own `Authorization` header is dropped. Printing "the secret" yields nothing because the sandbox never had one. The token is bound to the run, short-lived, and revoked when the run ends.

### 2. Per-run limits at the gateway
The gateway counts the bytes it returns for each run and refuses further results once the run's budget is spent, with a structured problem that says what to do instead (aggregate, filter, save as a scratch table). The data service adds the per-user quotas it already has (rate, concurrency, daily scan budget).

### 3. Lazy tables compiled to SQL
`ld.table("main.nyc.trips")` returns a lazy frame with a Polars-style surface: `select`, `filter`, `with_columns`, `group_by().agg()`, `join`, `sort`, `limit`, plus time-series operations (`resample`, `rolling`, `asof_join`). Operations build a plan; nothing runs until `collect()`, `save()` or `plot`. The plan compiles to SQL for the engine. An operation outside the supported subset raises a clear error that names the SQL escape hatch. It never pulls the data and computes locally.

### 4. Scratch tables and result handles
`frame.save("scratch.top_zones")` writes the result on the platform, as the calling user, with a quota and a time to live, and returns a handle. The next step reads the scratch table on the engine. Multi-step analysis therefore moves no intermediate data. Scratch tables obey the same row filters and column masks as their sources.

### 5. Answer-sized results
- `frame.estimate()` returns rows and bytes to scan from table statistics, with no scan.
- Counts, minimums and maximums over unfiltered columns are answered from metadata.
- Approximate aggregates (distinct count, quantiles, histograms) run as sketches.
- Plots are computed on the engine: histograms and 2D binning for point clouds, and per-pixel min/max/first/last (M4) downsampling for time series. A line chart of a billion ticks moves a few thousand points.

### 6. Results by size
Small results arrive inline as Arrow. Medium results are written to a memory-mapped Arrow/Parquet file the sandbox scans lazily, so memory stays bounded. Large results are refused with guidance. On the same host the handoff is a mapped file (no copy, no socket); across hosts it is parallel Arrow streams (Flight) under the byte budget.

### 7. Run the sandbox next to the data
A sandbox worker role runs on the same node and zone as the SQL workers, so the common case is level 3, not level 4. Pre-aggregated rollups (for stock data, 1-minute bars rolled to hourly and daily) are maintained incrementally so the agent reads a small table instead of scanning ticks.

### 8. Do not re-read what was read
- A local NVMe cache of hot Parquet byte ranges, keyed by immutable file and range.
- Plan-time prefetch: the planner knows the files and the worker, so it tells the worker to warm them while the ticket is signed.
- Per-file partial aggregates, so an appended file recomputes one file, not the table.
- Cache keys include the caller's policy, so sharing a cache between users never leaks a row.

### 9. Code to data
For work SQL cannot express, `ld.run(fn, over=table)` ships a sandboxed function to the workers and returns a table. The data does not travel to the code.

### 10. One interface for every agent
The same operations (search, describe, query, save, plot, estimate) are exposed as an MCP server and an Arrow Flight endpoint, so any agent's interpreter, not only ours, gets the same behaviour and the same limits.

## Built in slices

| Slice | Content | Status |
|---|---|---|
| 1 | Per-run socket; gateway holds the token; per-run byte budget; the code interpreter is actually given a grant; bytes-moved recorded | built |
| 2 | Lazy tables and scratch tables; structured refusals; estimate and metadata answers | next |
| 3 | Plot pushdown; medium-result spill; same-host mapped handoff | |
| 4 | Benchmark and LLM eval harness on the taxi table and a synthetic tick table | |
| 5 | NVMe range cache, plan-time prefetch, per-file partials | |
| 6 | Sandbox worker role; `ld.run`; Flight; MCP surface | |

## How it is checked

- **Budget tests:** a scripted analysis on the taxi table must stay under a bytes-into-sandbox budget and a memory cap of 2 GB for the sandbox.
- **Policy tests:** row filters, column masks and scratch tables behave the same through every path.
- **Security tests:** the sandbox holds no credential; a request carrying its own `Authorization` header is ignored; a run cannot reach another run's socket.
- **LLM eval:** a fixed set of realistic tasks run against each model in use, recording success, retries and bytes moved. The result decides how much SQL and how much lazy syntax the prompt shows.
