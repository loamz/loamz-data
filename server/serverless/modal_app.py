"""A serverless SQL endpoint on Modal: it starts when a statement arrives and stops after a minute with nothing to do.

Loamz Data calls it over https with the statement, where each table's Iceberg metadata is, and short-lived read-only credentials for
exactly those tables' folders. It reads the tables straight from the bucket, answers with the rows, and holds nothing afterwards: no
storage key of the product, no data of its own.

  modal deploy server/serverless/modal_app.py          (once; needs the secret `loamz-sql` with LOAMZ_TOKEN, see CLAUDE.md)
  LOAMZ_SQL_REGION=ap-south modal deploy …              (the region it runs in: pick the one nearest the bucket)

The statement has already been parsed and every table authorized by the Data server. Here DuckDB is locked down a second time."""

import hmac
import os
import time

import modal

HERE = os.path.dirname(os.path.abspath(__file__))
EXTENSIONS = "/opt/duckdb-extensions"

image = (
    modal.Image.debian_slim(python_version="3.12")
    .pip_install("duckdb==2.0.0", "fastapi[standard]==0.115.6")
    # Installed now, signed, into a fixed folder: a running container installs and downloads nothing.
    .run_commands(
        f"python -c \"import duckdb; c = duckdb.connect(config={{'extension_directory': '{EXTENSIONS}'}}); "
        "[c.execute(f'INSTALL {e}') for e in ('httpfs', 'avro', 'iceberg')]\""
    )
    .add_local_file(os.path.join(HERE, "../runner/src/sandbox/loamz_sql.py"), "/root/loamz_sql.py")
)

app = modal.App("loamz-sql", image=image)

# Imported only where the image has them (the container), not on the machine that deploys.
with image.imports():
    import duckdb
    from fastapi import HTTPException, Request
    from loamz_sql import answer, lit, q

# Two containers at most, so a runaway caller cannot spend the account; one minute idle and it is gone.
@app.function(
    secrets=[modal.Secret.from_name("loamz-sql")],
    cpu=1.0,
    memory=2048,
    timeout=300,
    scaledown_window=60,
    max_containers=2,
    # Close to the bucket: reading an Iceberg table is several small round trips, and each is far slower across an ocean.
    region=os.environ.get("LOAMZ_SQL_REGION", "ap-southeast"),
)
@modal.fastapi_endpoint(method="POST")
def sql(body: dict, request: Request):
    sent = request.headers.get("authorization", "")
    if not hmac.compare_digest(sent, "Bearer " + os.environ["LOAMZ_TOKEN"]):
        raise HTTPException(status_code=401)

    # Start and stop on demand. "warm" is answered the moment the container is up (so asking starts it); "stop" makes this
    # container take no more work after this call and exit, instead of waiting out its minute of idle.
    action = body.get("action")
    if action == "warm":
        return {"ready": True}
    if action == "stop":
        modal.experimental.stop_fetching_inputs()
        return {"stopping": True}

    began = time.perf_counter()
    storage, limit = body["storage"], int(body["limit"])
    con = duckdb.connect(
        ":memory:",
        config={
            "threads": "1",
            "memory_limit": f"{max(64, int(int(body['memory_mb']) * 0.7))}MB",
            "extension_directory": EXTENSIONS,
            "autoinstall_known_extensions": "false",
            "autoload_known_extensions": "false",
        },
    )
    con.execute("LOAD iceberg")
    con.execute("LOAD httpfs")
    loaded = time.perf_counter()
    host = storage["endpoint"].split("://", 1)[1]
    con.execute(
        f"CREATE SECRET (TYPE s3, KEY_ID {lit(storage['key_id'])}, SECRET {lit(storage['secret'])}, "
        f"SESSION_TOKEN {lit(storage['session_token'])}, ENDPOINT {lit(host)}, URL_STYLE 'path', REGION 'auto')"
    )
    roots, catalogs, schemas = [], set(), set()
    for t in body["tables"]:
        catalog, schema, name = t["name"].split(".")
        root, _, file = t["metadata"].rpartition("/metadata/")
        if not root.startswith(f"s3://{storage['bucket']}/") or "/" in file:
            return {"error": f"{t['name']} is not where it should be"}
        roots.append(root + "/")
        if catalog not in catalogs:
            con.execute(f"ATTACH ':memory:' AS {q(catalog)}")
            catalogs.add(catalog)
        if (catalog, schema) not in schemas:
            con.execute(f"CREATE SCHEMA IF NOT EXISTS {q(catalog)}.{q(schema)}")
            schemas.add((catalog, schema))
        version = file.removesuffix(".metadata.json")
        try:
            con.execute(
                f"CREATE VIEW {q(catalog)}.{q(schema)}.{q(name)} AS "
                f"SELECT * FROM iceberg_scan({lit(root)}, version = {lit(version)})"
            )
        except Exception as e:  # noqa: BLE001
            return {"error": f"{t['name']} could not be read: {e}"}
    attached = time.perf_counter()
    # From here the statement reaches only those tables' folders, and cannot change any of this.
    allowed = ", ".join(lit(r) for r in roots)
    con.execute(f"SET allowed_directories = [{allowed}]")
    con.execute("SET enable_external_access = false")
    con.execute("SET lock_configuration = true")
    result = answer(con, body["sql"], limit)
    done = time.perf_counter()
    # Where the time went inside the container, in milliseconds, so a slow statement can be explained.
    result["timings_ms"] = {
        "extensions": round((loaded - began) * 1000),
        "tables": round((attached - loaded) * 1000),
        "statement": round((done - attached) * 1000),
    }
    return result
