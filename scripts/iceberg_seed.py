#!/usr/bin/env python3
"""Makes an Iceberg table through a Loamz Data catalog and fills it with a few rows, the way any DuckDB client would.
Needs DuckDB 2.0 (`~/.cache/loamz/modal-venv/bin/python` has it). The token comes from the environment, never the command line.

  LZ_TOKEN=… scripts/iceberg_seed.py http://127.0.0.1:8020/api/v1/workspaces/<ws>/iceberg <catalog> <schema.table> '<columns>' < rows.json
  LZ_TOKEN=… scripts/iceberg_seed.py <endpoint> <catalog> <schema.table> - < rows.json        (the table exists: only add the rows)

rows.json is a list of rows, for example [[1,"north",120],[2,"south",80]]."""
import json, os, sys
import duckdb

endpoint, catalog, table, columns = sys.argv[1:5]
rows = json.load(sys.stdin)
con = duckdb.connect()
con.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
token = os.environ["LZ_TOKEN"]
con.execute(f"ATTACH '{catalog}' AS lz (TYPE iceberg, ENDPOINT '{endpoint}', TOKEN '{token}')")
if columns != "-":
    con.execute(f"CREATE TABLE lz.{table} ({columns})")
if rows:
    marks = ",".join("?" * len(rows[0]))
    con.executemany(f"INSERT INTO lz.{table} VALUES ({marks})", rows)
print(f"{table}: {len(rows)} rows")
