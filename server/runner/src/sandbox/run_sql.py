"""Runs one SQL statement over the tables a run declared, inside the sandbox. Written by Loamz, not by the person: it is the only
thing that touches DuckDB, and it locks DuckDB down before the person's statement is run.

Reads /inputs/job/spec.json, reads tables from /inputs/tables/<id>/ (an Iceberg table's folder, beside loamz-table.json that names its current metadata file), and writes
/workspace/result.json: {"columns", "rows", "truncated"} or {"error"}. A mistake in the SQL is a result, not a crash."""

import datetime
import decimal
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

con = duckdb.connect(
    ":memory:",
    config={
        "threads": "1",
        "memory_limit": f"{max(64, int(spec['memory_mb'] * 0.7))}MB",
        # The Iceberg reader was installed when this filesystem was built: nothing is downloaded or installed here.
        "extension_directory": os.environ.get("DUCKDB_EXTENSIONS", "/opt/duckdb-extensions"),
        "autoinstall_known_extensions": "false",
        "autoload_known_extensions": "false",
    },
)
con.execute("LOAD iceberg")

catalogs, schemas = set(), set()
for t in spec["tables"]:
    catalog, schema, name = t["name"].split(".")
    folder = f"/inputs/tables/{t['id']}"
    try:
        version = os.path.basename(json.load(open(folder + "/loamz-table.json"))["metadata"]).removesuffix(".metadata.json")
    except (OSError, KeyError, ValueError):
        fail(f"{t['name']} could not be found on this machine")
    if catalog not in catalogs:
        con.execute(f"ATTACH ':memory:' AS {q(catalog)}")
        catalogs.add(catalog)
    if (catalog, schema) not in schemas:
        con.execute(f"CREATE SCHEMA IF NOT EXISTS {q(catalog)}.{q(schema)}")
        schemas.add((catalog, schema))
    try:
        # The table's files were fetched beside its metadata, which still names them by their address in the bucket.
        con.execute(f"CREATE VIEW {q(catalog)}.{q(schema)}.{q(name)} AS SELECT * FROM iceberg_scan({lit(folder)}, version = {lit(version)}, allow_moved_paths = true)")
    except Exception as e:  # noqa: BLE001
        fail(f"{t['name']} could not be read: {e}")

# From here the statement can reach only the table files, nothing else on this machine, and cannot change any of this.
con.execute("SET allowed_directories = ['/inputs/tables']")
con.execute("SET enable_external_access = false")
con.execute("SET lock_configuration = true")

try:
    relation = con.sql(spec["sql"])
    columns = [{"name": n, "type": str(t)} for n, t in zip(relation.columns, relation.types)]
    rows = relation.limit(limit + 1).fetchall()
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
