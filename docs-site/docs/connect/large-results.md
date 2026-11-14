# Large Results

When working with queries that produce millions of rows or large analytical outputs, transporting rows as JSON arrays over HTTP is slow and memory-intensive.

Loamz Data provides two governed mechanisms to handle large results:
1. **Statement Export (`export: true`)**: Streams query results into an Apache Arrow IPC stream file kept for 24 hours, downloadable directly via a short-lived presigned URL.
2. **Statement Save (`save: { volume, thread }`)**: Saves query results as governed Apache Parquet files directly into a thread in your volume, ready for downstream processing.

Additionally, sandboxed compute jobs can read volume data **in place** through the `loamz` Python module without downloading files upfront, and commit generated output artifacts back to volume threads.

---

## 1. Statement Export

To export the complete result of a SQL query without loading all rows into memory or the HTTP response:

Pass `"export": true` when submitting a statement to `POST /api/v1/workspaces/{ws}/sql/statements`:

```json
{
  "sql": "SELECT order_id, region, amount FROM shop.main.orders",
  "export": true,
  "wait_s": 60
}
```

### How Export Works

- **Storage Location**: The compute machine streams the query output directly into an Apache Arrow IPC stream file (`application/vnd.apache.arrow.stream`, up to 1 GiB) in the object store beside the first table the statement reads. Statements that read no table cannot be exported.
- **Run Response**: In the returned run object, inline `result.rows` is omitted. Instead, `result.export` contains the row count and byte size:
  ```json
  {
    "id": "run_01j7...",
    "state": "succeeded",
    "result": {
      "columns": [
        {"name": "order_id", "type": "BIGINT"},
        {"name": "region", "type": "VARCHAR"},
        {"name": "amount", "type": "BIGINT"}
      ],
      "export": {
        "rows": 15625,
        "bytes": 482104
      }
    }
  }
  ```
- **Result Expiry**: The exported Arrow file is retained for **24 hours**, after which it is automatically removed. If the user who initiated the run is deleted or erased, the file is deleted immediately.

### Fetching the Exported Result Address

Call `GET /api/v1/workspaces/{ws}/runs/{id}/result` to obtain a signed URL for the Arrow stream:

```http
GET /api/v1/workspaces/{ws}/runs/{run_id}/result
Authorization: Bearer <token>
```

Response (`200 OK`):
```json
{
  "url": "https://storage.example.com/loamz-data/exports/run_01j7...arrow?X-Amz-Signature=...",
  "bytes": 482104,
  "format": "application/vnd.apache.arrow.stream",
  "expires_ms": 1731580800000
}
```

The signed `url` points directly to object storage. You can fetch and stream it without passing any Loamz tokens or storage credentials.

---

## 2. Statement Save & Idempotency

When you want to persist the query result permanently in a governed volume for analytics pipelines or AI agent workflows, use `save`.

Pass `"save"` with a `volume` ID and a `thread` identifier:

```json
{
  "sql": "SELECT order_id, region, amount FROM shop.main.orders",
  "save": {
    "volume": "vol_01j7...",
    "thread": "analysis-run-1"
  },
  "idempotency_key": "step-aggregate-2026-11-14",
  "wait_s": 60
}
```

### Parameters

- `save.volume` (`string`, required): The ID of a volume where you have `WRITE` permission.
- `save.thread` (`string`, required): A logical thread or conversation name (up to 64 ASCII alphanumeric characters, dashes, and underscores: `[a-zA-Z0-9_-]`).
- `idempotency_key` (`string`, optional): A client-chosen key that prevents duplicate query execution upon retries. Submitting the same idempotency key from the same caller returns the existing run and its saved result rather than launching a new compute job.

### Saved Output Details

- **File Path**: The compute runner writes the result directly as an Apache Parquet file at:
  ```text
  threads/<thread>/results/<run_id>.parquet
  ```
- **Inline Preview**: Unlike `export`, `save` returns a 20-row preview in `result.rows` alongside the `export` metadata:
  ```json
  {
    "id": "run_01j7...",
    "state": "succeeded",
    "result": {
      "columns": [...],
      "rows": [
        [1, "north", 120],
        [2, "south", 80]
      ],
      "export": {
        "format": "parquet",
        "volume": "vol_01j7...",
        "path": "threads/analysis-run-1/results/run_01j7....parquet",
        "rows": 15625,
        "bytes": 395120
      }
    }
  }
  ```
- **Accessing Saved Files**: The saved Parquet file is a regular volume file and can be downloaded via `GET /api/v1/workspaces/{ws}/volumes/{id}/files/{path}` or read in place by jobs.

---

## 3. Reading Results in Python

### Reading an Export (Arrow IPC Stream)

Use `pyarrow.ipc` or `polars` to read the signed address directly from storage without Loamz credentials:

```python
import urllib.request
import polars as pl
import pyarrow.ipc as ipc

# 1. Obtain the signed URL from Loamz Data API
# GET /api/v1/workspaces/{ws}/runs/{run_id}/result -> link
url = link["url"]

# Option A: Read into Apache Arrow Table
with urllib.request.urlopen(url, timeout=60) as response:
    reader = ipc.open_stream(response)
    arrow_table = reader.read_all()
    print(f"Arrow Table: {arrow_table.num_rows} rows, {arrow_table.num_columns} columns")

# Option B: Read into Polars DataFrame
df = pl.read_ipc(url)
print(f"Polars DataFrame: {df.shape}")
```

### Reading a Saved Result (Parquet)

To read a saved result file downloaded from a volume:

```python
import io
import urllib.request
import polars as pl
import pyarrow.parquet as pq

# Fetch the file bytes from the volume API using your Loamz token
req = urllib.request.Request(
    f"https://data.loamz.com/api/v1/workspaces/{ws}/volumes/{volume_id}/files/{saved_path}",
    headers={"Authorization": f"Bearer {token}"}
)
with urllib.request.urlopen(req) as resp:
    parquet_bytes = resp.read()

# Option A: Read with PyArrow
table = pq.read_table(io.BytesIO(parquet_bytes))
print(f"Read {table.num_rows} rows with schema: {table.schema.names}")

# Option B: Read with Polars
df = pl.read_parquet(io.BytesIO(parquet_bytes))
print(f"Aggregated total: {df['amount'].sum()}")
```

---

## 4. In-Place Job Reads (`reads`)

When running Python transformations or AI agent scripts via `POST /api/v1/workspaces/{ws}/runs`, copying large datasets into the sandbox is inefficient.

By specifying volumes in `reads`, the job accesses data **in place** over byte-range requests without any initial download:

```json
{
  "script": "...",
  "reads": ["vol_01j7..."],
  "commit": {
    "volume": "vol_01j7...",
    "thread": "analysis-run-1"
  },
  "timeout_s": 120
}
```

### The `loamz` Runtime Module

Inside the locked-down sandbox, the standard library includes the `loamz` helper module:

| Function | Description |
|---|---|
| `loamz.path(volume_name, relative_path)` | Returns the `s3://` URI for a file in a declared volume. Polars, DuckDB, and PyArrow use this address to issue byte-range queries through the local storage broker. |
| `loamz.duckdb()` | Returns a DuckDB connection pre-configured with `httpfs` loaded and an S3 secret pointing to the local broker. |
| `loamz.fs()` | Returns a `pyarrow.fs.S3FileSystem` configured to query the local broker. |
| `loamz.fetch(volume_name, relative_path)` | Copies a single file from the volume to `/tmp/loamz-fetch/...` and returns its local path (for libraries that require local POSIX paths). |
| `loamz.out(relative_name)` | Returns a path under `/workspace/out/<relative_name>` ensuring parent directories are created, ready to be committed when the run ends. |

### Sandboxed Broker Governance

- **Zero Network Egress**: The sandbox has no internet access and no cloud storage credentials in its environment.
- **Enforced Isolation**: The runner opens a local Unix socket bridge to a storage broker that mediates all reads.
- **Read-Only**: Any attempt to issue `PUT`, `DELETE`, or access an undeclared volume path is rejected with `HTTP 403 Forbidden`.

---

## 5. Job Output Commit (`commit`)

To persist outputs generated by a job without manual upload steps, specify `"commit"` in the job submission:

```json
{
  "commit": {
    "volume": "vol_01j7...",
    "thread": "analysis-run-1"
  }
}
```

### Commit Behavior and Constraints

1. **Output Directory**: Files placed in `/workspace/out/` (or created via `loamz.out("filename")`) are inspected upon job completion.
2. **Safe Collection**: Files are resolved using `openat2` with `RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS`. Symlinks, devices, FIFO sockets, and files pointing outside `/workspace/out` are refused.
3. **Naming Rules**: Plain ASCII names only (letters, digits, `.`, `_`, `-`, and spaces; up to 128 characters per component). Folders can be nested up to **4 levels deep**.
4. **Quotas**:
   - Maximum **200 files** per run.
   - Maximum **256 MiB** per file.
   - Maximum **1 GiB** total across all files.
5. **Create-Only (`If-None-Match: *`)**: Files are committed to:
   ```text
   threads/<thread>/runs/<run_id>/<relative_path>
   ```
   Files are never overwritten.
6. **Non-Transactional Reporting**: Files are committed individually. The run's `result` field reports:
   - `committed`: Array of `{ path, bytes }` objects for successfully uploaded files.
   - `failed`: Array of strings explaining any files that could not be committed.
   > [!IMPORTANT]
   > If any file in `/workspace/out` cannot be committed (e.g. invalid name, symlink, quota exceeded), the entire run transitions to `failed`, but valid files already committed remain recorded in `committed`.
7. **Thread Concurrency**: Only **one active job** can run per thread at a time. Submitting a second job targeting the same thread returns `HTTP 409 Conflict`.

---

## 6. Complete Job Example

Here is a complete Python job script demonstrating in-place reads, processing with Polars and DuckDB, and committing reports to the volume:

```python
import json
import loamz
import polars as pl

# 1. Locate the saved Parquet result in the volume
parquet_uri = loamz.path("agent_files", "threads/analysis-run-1/results/run_01j7.parquet")

# 2. Query in place with Polars (reads only needed byte ranges)
df = (
    pl.scan_parquet(parquet_uri)
    .filter(pl.col("amount") > 100)
    .group_by("region")
    .agg(
        pl.len().alias("count"),
        pl.col("amount").sum().alias("total_amount")
    )
    .collect()
)

# 3. Query in place using DuckDB
con = loamz.duckdb()
duck_summary = con.execute(
    f"SELECT count(*) FROM read_parquet('{parquet_uri}') WHERE region = 'north'"
).fetchone()[0]

# 4. Fetch a specific file to /tmp for libraries requiring local paths
local_config = loamz.fetch("agent_files", "configs/thresholds.json")
thresholds = json.load(open(local_config))

# 5. Commit output files to the thread's run directory
# Write CSV summary
df.write_csv(loamz.out("reports/regional_summary.csv"))

# Write JSON metadata
with open(loamz.out("reports/metadata.json"), "w") as f:
    json.dump({
        "north_count": duck_summary,
        "threshold_applied": thresholds.get("min_amount", 100)
    }, f)
```

When this run completes:
- `threads/analysis-run-1/runs/<run_id>/reports/regional_summary.csv`
- `threads/analysis-run-1/runs/<run_id>/reports/metadata.json`

are committed to `agent_files` and listed in the run's `result.committed` field.

---

## 7. Deleting a Thread

When a conversation, analysis session, or workflow completes, you can purge all thread files in a volume:

```http
DELETE /api/v1/workspaces/{ws}/volumes/{id}/threads/{thread}
Authorization: Bearer <token>
```

### Execution Order & Lifecycle

1. **Closure**: The thread is immediately closed to new saved results (`save`) and committed outputs (`commit`).
2. **Cancellation**: Any running job currently executing on the thread is cancelled immediately.
3. **Deletion**: All files under `threads/<thread>/` (both `results/` and `runs/`) are removed from storage.
4. **Cooldown Window**: The thread remains locked and closed for **10 minutes**. Any attempt to start new work on the thread during this window returns `HTTP 409 Conflict`.
5. **Final Cleanup**: After 10 minutes, the server performs a second deletion sweep to ensure any files written by lagging or ungraceful compute processes are completely purged.
