"""Runs one SQL statement over the tables a run declared, inside the sandbox. Written by Loamz, not by the person: it is the only
thing that touches DuckDB, and it locks DuckDB down before the person's statement is run.

Reads /inputs/job/spec.json, reads tables from /inputs/tables/<id>/ (an Iceberg table's folder, beside loamz-table.json that names
its current metadata file), and writes /workspace/result.json: {"columns", "rows", "truncated"} or {"error"}. A mistake in the SQL is
a result, not a crash."""

import json
import os
import sys

import duckdb
from loamz_sql import answer, lit, q

OUT = "/workspace/result.json"


def write(obj):
    with open(OUT, "w") as f:
        json.dump(obj, f)


def fail(message):
    write({"error": message})
    sys.exit(0)


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

result = answer(con, spec["sql"], limit)
write(result)
