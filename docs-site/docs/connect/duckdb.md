# Connecting DuckDB to Loamz Data

DuckDB connects natively to Loamz Data using the standard Apache Iceberg REST catalog protocol and vended S3 storage credentials.

---

## Prerequisites

- DuckDB version 1.1 or higher (DuckDB 2.0+ recommended)
- Network access to your Loamz Data server
- A valid bearer token or personal access token

---

## Connecting via SQL

In DuckDB, install the `iceberg` and `httpfs` extensions, then attach your catalog:

```sql
INSTALL iceberg;
INSTALL httpfs;
LOAD iceberg;
LOAD httpfs;

ATTACH 'nyc' AS lz (
    TYPE iceberg,
    ENDPOINT 'http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/iceberg',
    TOKEN '<your-token>'
);
```

Once attached, query tables using standard dot notation:

```sql
SELECT count(*), avg(fare) FROM lz.main.trips WHERE zone = 132;
```

---

## Python Integration

Using DuckDB's Python library:

```python
import os
import duckdb

token = os.environ["LOAMZ_TOKEN"]
endpoint = "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/iceberg"

con = duckdb.connect()
con.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
con.execute(f"ATTACH 'nyc' AS lz (TYPE iceberg, ENDPOINT '{endpoint}', TOKEN '{token}')")

df = con.execute("""
    SELECT
        date_trunc('day', pickup_ts) AS day,
        count(*) AS trip_count,
        round(sum(fare), 2) AS total_revenue
    FROM lz.main.trips
    GROUP BY 1
    ORDER BY 1
""").df()

print(df.head())
```

---

## How Vended Credentials Work

When DuckDB plans a query against an attached table:
1. DuckDB sends an HTTP request to the Loamz Iceberg REST endpoint (`/iceberg/v1/<catalog>/namespaces/<schema>/tables/<table>`).
2. Loamz Data verifies your access privileges for that table.
3. Loamz Data vends short-lived, prefix-scoped S3 credentials granting read access only to that table's directory in object storage.
4. DuckDB reads Parquet files directly from storage with full range-request parallelization. Your storage credentials never leave the engine and expire automatically after the session.
