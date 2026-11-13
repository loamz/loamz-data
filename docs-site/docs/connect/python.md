# Python SDK and CLI

The `loamz` Python package provides programmatic access to Loamz Data catalogs, SQL execution engines, and DuckDB table attachments.

---

## Installation

```bash
pip install loamz

# With optional dependencies for Arrow, Pandas, and DuckDB:
pip install "loamz[arrow,pandas,duckdb]"
```

---

## Authentication

Set your token via the `LOAMZ_TOKEN` environment variable or provide it explicitly when creating the client:

```bash
export LOAMZ_TOKEN="eyJhbGciOi..."
```

---

## Python SDK Usage

```python
from loamz import Client

# Initialize client (defaults to LOAMZ_URL or http://127.0.0.1:8020)
client = Client("http://127.0.0.1:8020")

# Inspect identity
me = client.me()
print(f"Signed in as {me.name} ({me.email})")

# Select a workspace by name or ID
ws = client.workspace("Main")

# List catalogs and tables
catalogs = ws.catalogs()
tables = ws.tables("nyc", "main")
print(f"Tables in nyc.main: {[t.name for t in tables]}")

# Execute a query and fetch rows
rows = ws.sql("SELECT count(*) FROM nyc.main.trips")
print(f"Columns: {rows.columns}")
print(f"Result: {rows.rows[0][0]:,} trips")

# Export rows to Pandas or PyArrow
df = rows.to_pandas()
arrow_tbl = rows.to_arrow()

# Direct DuckDB attachment with vended Iceberg credentials
handle = ws.table("nyc.main.trips")
con = handle.duckdb()
print(con.execute("SELECT avg(fare) FROM lz.main.trips").fetchone()[0])
```

---

## Command-Line Tool (`loamz`)

The package includes a command-line utility for shell scripting:

```bash
# Check current identity
loamz whoami

# List catalogs
loamz ls

# List tables in a catalog and schema
loamz ls nyc.main

# Run a query and output tabular results
loamz sql "SELECT count(*) FROM nyc.main.trips"

# Output structured JSON for piping into jq
loamz sql "SELECT * FROM nyc.main.zones LIMIT 5" --json
```
