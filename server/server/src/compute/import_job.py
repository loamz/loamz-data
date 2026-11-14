# Brings one file in as an Iceberg table. Written by the server, run by compute in the sandbox of a write lease: it has no network, and
# reaches the table catalog and storage only through the two doors (loamz_doors). Nothing here comes from the person except the
# values in SPEC, which the server has checked.
import json
import os
import sys

import duckdb

sys.path.insert(0, "/inputs/job")
import loamz_doors  # noqa: E402

SPEC = json.loads(bytes.fromhex("__SPEC__").decode())

catalog, schema, table = SPEC["target"].split(".")
if SPEC["source"].get("fetched"):
    # The machine downloaded it before this sandbox started.
    path = "/inputs/fetched/data"
    if not os.path.isfile(path):
        sys.exit("the file was not downloaded")
else:
    # The source volume is mounted under its own name; folders left over from other runs may also be there, so look for the file.
    relative = os.path.normpath(SPEC["source"]["path"])
    if relative.startswith(("..", "/")):
        sys.exit("the source file's path is not valid")
    found = [
        os.path.join("/inputs", d, relative)
        for d in sorted(os.listdir("/inputs"))
        if d not in ("job", "doors", "fetched") and os.path.isfile(os.path.join("/inputs", d, relative))
    ]
    if len(found) != 1:
        sys.exit(f"the source file was found in {len(found)} places, not one")
    path = found[0]

catalog_endpoint, storage_endpoint = loamz_doors.start(SPEC["workspace"])
con = duckdb.connect(
    ":memory:",
    config={
        "extension_directory": os.environ.get("DUCKDB_EXTENSIONS", "/opt/duckdb-extensions"),
        "autoinstall_known_extensions": "false",
        "autoload_known_extensions": "false",
    },
)
con.execute("LOAD httpfs")
con.execute("LOAD iceberg")
# The doors sign storage requests and add the run's token themselves: what is given here is only a placeholder.
con.execute(
    f"CREATE SECRET door (TYPE s3, KEY_ID 'door', SECRET 'door', ENDPOINT '{storage_endpoint}', URL_STYLE 'path', USE_SSL false, REGION 'auto')"
)
con.execute(
    f"ATTACH '{catalog}' AS lake (TYPE iceberg, ENDPOINT '{catalog_endpoint}', TOKEN 'door', ACCESS_DELEGATION_MODE 'none')"
)

reader = {"csv": "read_csv", "parquet": "read_parquet"}[SPEC["format"]]
target = f'lake."{schema}"."{table}"'
if SPEC["mode"] == "append":
    con.execute(f"INSERT INTO {target} SELECT * FROM {reader}(?)", [path])
elif SPEC["mode"] == "replace":
    # The table keeps its columns: the file's must fit them, and the rows are swapped in one transaction.
    con.execute("BEGIN")
    con.execute(f"DELETE FROM {target}")
    con.execute(f"INSERT INTO {target} SELECT * FROM {reader}(?)", [path])
    con.execute("COMMIT")
else:
    con.execute(f"CREATE TABLE {target} AS SELECT * FROM {reader}(?)", [path])
rows = con.execute(f"SELECT count(*) FROM {reader}(?)", [path]).fetchone()[0]
print(json.dumps({"rows": rows, "table": SPEC["target"]}))
