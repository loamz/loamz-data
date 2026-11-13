# First Steps with Loamz Data

This guide walks you through signing in, selecting a workspace, exploring your data catalog, and running your first SQL query against the NYC taxi trips dataset (`nyc.main.trips`).

---

## 1. Sign In and Open Your Workspace

1. Open the Loamz Data console in your browser at [http://localhost:5180](http://localhost:5180).
2. Sign in with your organization account.
3. Select your organization and open your workspace.

Each workspace is an isolated environment containing its own data catalogs, compute engines, team members, and query histories.

---

## 2. Explore the Catalog

Navigate to **Catalog** in the left sidebar:

- **Catalogs**: Top-level containers grouping your database schemas. You will see the `nyc` catalog.
- **Schemas**: Namespaces inside a catalog. Open `main`.
- **Tables**: Managed Apache Iceberg tables stored in open object storage. Click on `trips` to view its schema:
  - `pickup_ts` (timestamp)
  - `dropoff_ts` (timestamp)
  - `passenger_count` (integer)
  - `trip_distance` (double)
  - `fare_amount` (double)
  - `tip_amount` (double)

---

## 3. Run a Query in the SQL Editor

1. Click **SQL** in the left sidebar.
2. Enter the following query to calculate average fares and trip counts by pickup zone:

```sql
SELECT
  zone,
  count(*) AS total_trips,
  round(avg(fare), 2) AS average_fare,
  round(sum(tip), 2) AS total_tips
FROM nyc.main.trips
GROUP BY zone
ORDER BY total_trips DESC
LIMIT 10;
```

3. Click **Run** (or press `Ctrl+Enter` / `Cmd+Enter`).
4. Results appear below the editor with execution time and byte stats. You can export results as CSV or JSON.

---

## 4. Query from the Command Line

You can also run queries directly from your terminal using the `loamz` command-line tool:

```bash
# Set your authentication token
export LOAMZ_TOKEN="$(python deploy/dev/token.py ana)"

# Check identity and workspace access
loamz whoami

# List tables in the catalog
loamz ls nyc.main

# Execute a query
loamz sql "SELECT count(*) FROM nyc.main.trips"
```

---

## 5. Query from Python or DuckDB

Loamz Data serves an Apache Iceberg REST catalog. You can attach it directly to DuckDB with standard Iceberg credentials:

```python
import duckdb

con = duckdb.connect()
con.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")

# Attach the catalog through the workspace endpoint
con.execute("""
    ATTACH 'nyc' AS lz (
        TYPE iceberg,
        ENDPOINT 'http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/iceberg',
        TOKEN 'your_token_here'
    )
""")

# Query the table directly with zero server compute overhead
results = con.execute("SELECT count(*) FROM lz.main.trips").fetchone()
print(f"Total trips: {results[0]:,}")
```
