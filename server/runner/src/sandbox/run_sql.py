"""Runs one SQL statement over the tables a run declared, inside the sandbox. Written by Loamz, not by the person: it is the only
thing that touches DuckDB, and it locks DuckDB down before the person's statement is run.

Reads /inputs/job/spec.json, reads tables from /inputs/tables/<id>/ (CSV or Parquet files, one kind per table), and writes
/workspace/result.json: {"columns", "rows", "truncated"} or {"error"}. A mistake in the SQL is a result, not a crash."""

import datetime
import decimal
import glob
import json
import os
import sys

import duckdb

OUT = "/workspace/result.json"
MAX_BYTES = 3_500_000
SAFE_INT = 2**53


def write(obj):
    with open(OUT, "w") as f:
        json.dump(obj, f)


def fail(message):
    write({"error": message})
    sys.exit(0)


def q(name):
    return '"' + name.replace('"', '""') + '"'


def lit(path):
    return "'" + path.replace("'", "''") + "'"


def cell(v):
    if v is None or isinstance(v, (bool, str)):
        return v
    if isinstance(v, int):
        return v if abs(v) < SAFE_INT else str(v)
    if isinstance(v, float):
        return v if v == v and v not in (float("inf"), float("-inf")) else str(v)
    if isinstance(v, (bytes, bytearray)):
        return v.hex()
    if isinstance(v, (datetime.datetime, datetime.date, datetime.time)):
        return v.isoformat()
    if isinstance(v, (list, tuple)):
        return [cell(x) for x in v]
    if isinstance(v, dict):
        return {str(k): cell(x) for k, x in v.items()}
    if isinstance(v, decimal.Decimal):
        return str(v)
    return str(v)


spec = json.load(open("/inputs/job/spec.json"))
limit = int(spec["limit"])

con = duckdb.connect(":memory:", config={"threads": "1", "memory_limit": f"{max(64, int(spec['memory_mb'] * 0.7))}MB"})

catalogs, schemas = set(), set()
for t in spec["tables"]:
    catalog, schema, name = t["name"].split(".")
    folder = f"/inputs/tables/{t['id']}"
    files = sorted(p for p in glob.glob(folder + "/**", recursive=True) if os.path.isfile(p))
    parquet = [p for p in files if p.lower().endswith(".parquet")]
    csv = [p for p in files if p.lower().endswith((".csv", ".csv.gz"))]
    if parquet and csv:
        fail(f"{t['name']} has both Parquet and CSV files; a table holds one kind")
    if not (parquet or csv):
        fail(f"{t['name']} has no files yet: upload CSV or Parquet files to it first")
    if catalog not in catalogs:
        con.execute(f"ATTACH ':memory:' AS {q(catalog)}")
        catalogs.add(catalog)
    if (catalog, schema) not in schemas:
        con.execute(f"CREATE SCHEMA IF NOT EXISTS {q(catalog)}.{q(schema)}")
        schemas.add((catalog, schema))
    reader = "read_parquet" if parquet else "read_csv"
    paths = "[" + ", ".join(lit(p) for p in (parquet or csv)) + "]"
    try:
        con.execute(f"CREATE VIEW {q(catalog)}.{q(schema)}.{q(name)} AS SELECT * FROM {reader}({paths}, union_by_name = true)")
    except Exception as e:  # noqa: BLE001
        fail(f"{t['name']} could not be read: {e}")

# From here the statement can reach only the table files, nothing else on this machine, and cannot change any of this.
con.execute("SET allowed_directories = ['/inputs/tables']")
con.execute("SET enable_external_access = false")
con.execute("SET lock_configuration = true")

try:
    cur = con.execute(spec["sql"])
    columns = [{"name": d[0], "type": str(d[1])} for d in (cur.description or [])]
    rows = cur.fetchmany(limit + 1)
except Exception as e:  # noqa: BLE001
    fail(str(e))

truncated = len(rows) > limit
out, size = [], 0
for r in rows[:limit]:
    row = [cell(v) for v in r]
    size += len(json.dumps(row))
    if size > MAX_BYTES:
        truncated = True
        break
    out.append(row)
write({"columns": columns, "rows": out, "truncated": truncated})
