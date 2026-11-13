# Loamz Data Python SDK and CLI

Python client library and command-line interface for Loamz Data.

## Installation

```bash
pip install loamz
# Optional extras:
pip install "loamz[arrow,pandas,duckdb]"
```

## CLI Usage

```bash
# Authenticate (set LOAMZ_TOKEN or pass --token)
export LOAMZ_TOKEN="$(python deploy/dev/token.py ana)"

# Show current identity and workspaces
loamz whoami

# List catalogs, schemas, or tables
loamz ls
loamz ls nyc
loamz ls nyc.main

# Run a query
loamz sql "SELECT count(*) FROM nyc.main.trips"
loamz sql "SELECT * FROM nyc.main.zones LIMIT 5" --json
```

## Python SDK

```python
from loamz import Client

# Connect with environment token (LOAMZ_TOKEN)
client = Client("http://127.0.0.1:8020")

# Check identity
me = client.me()
print(f"Logged in as {me.name} ({me.email})")

# Select a workspace
ws = client.workspace("Main")

# Inspect securables
catalogs = ws.catalogs()
tables = ws.tables("nyc", "main")

# Run a SQL statement
rows = ws.sql("SELECT * FROM nyc.main.zones LIMIT 10")
print(rows.columns)
for row in rows:
    print(row)

# Export to Arrow or Pandas
df = rows.to_pandas()
table = rows.to_arrow()

# Direct DuckDB Iceberg integration
handle = ws.table("nyc.main.trips")
con = handle.duckdb()
res = con.execute("SELECT count(*) FROM lz.main.trips").fetchall()
print(res)
```

## Bring data in

```sh
loamz import trips.parquet nyc.main.trips            # a new table from a CSV or Parquet file
loamz import more.csv nyc.main.trips --append        # add rows to a table that exists
```

```python
rows = client.workspace("Main").table("nyc.main.trips").import_file("trips.parquet")
```

The file is read on your machine and written straight to the table's storage with short-lived credentials from the catalog, so
the data does not pass through the server. You need permission to create (or modify) the table. A table with row or column rules
cannot be written this way by someone the rules apply to.

A file that is already in a volume (uploaded from the console, or by a job) is imported on the workspace's own compute instead, with
no local DuckDB:

```sh
loamz import-volume uploads.main.incoming trips.csv nyc.main.trips          # a new table
loamz import-volume uploads.main.incoming more.csv nyc.main.trips --append   # add rows
```

```python
run = client.workspace("Main").import_from_volume("nyc.main.trips", "uploads.main.incoming", "trips.csv")
```

