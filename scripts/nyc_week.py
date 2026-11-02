#!/usr/bin/env python3
"""Puts one week of NYC yellow taxi trips into a Loamz Data workspace as the table nyc.main.yellow_trips, the way any DuckDB client would:
the catalog and schema are made through the API, then the rows are written through the catalog (so they land in the workspace's store).

The trips are read from a folder of Parquet files (an Iceberg table's `data/` folder is one) on an S3 endpoint. Needs DuckDB 2.0
(`~/.cache/loamz/modal-venv/bin/python` has it). Keys and the token come from the environment, never the command line:

  LZ_TOKEN=… SRC_KEY=… SRC_SECRET=… scripts/nyc_week.py http://127.0.0.1:8020 <workspace name or id> [first day, default 2025-03-03]

SRC_ENDPOINT (default 127.0.0.1:8333, no TLS) and SRC_PREFIX (default s3://warehouse/main.nyc/yellow_trips/data) say where they are.
A week is seven days from the first day, by pickup time. Run once: the table is not replaced."""
import datetime as dt, json, os, re, sys, urllib.request
import duckdb

base, which = sys.argv[1].rstrip("/"), sys.argv[2]
first = dt.date.fromisoformat(sys.argv[3]) if len(sys.argv) > 3 else dt.date(2025, 3, 3)
token = os.environ["LZ_TOKEN"]
prefix = os.environ.get("SRC_PREFIX", "s3://warehouse/main.nyc/yellow_trips/data")


def api(method: str, path: str, body: dict | None = None) -> dict:
    req = urllib.request.Request(base + path, method=method, data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req))


me = api("GET", "/api/v1/me")
ws = which if re.fullmatch(r"[0-9a-f-]{36}", which) else next(w["id"] for w in me["workspaces"] if w["name"] == which)
W = f"/api/v1/workspaces/{ws}"


def ensure(kind: str, parent: str, name: str) -> str:
    for s in api("GET", f"{W}/securables?parent_id={parent}")["items"]:
        if s["name"] == name and s["kind"] == kind:
            return s["id"]
    return api("POST", f"{W}/securables", {"kind": kind, "parent_id": parent, "name": name})["id"]


catalog = ensure("catalog", "", "nyc")
ensure("schema", catalog, "main")

con = duckdb.connect()
con.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
con.execute(
    "CREATE SECRET src (TYPE s3, KEY_ID ?, SECRET ?, ENDPOINT ?, URL_STYLE 'path', USE_SSL false, SCOPE ?)",
    [os.environ["SRC_KEY"], os.environ["SRC_SECRET"], os.environ.get("SRC_ENDPOINT", "127.0.0.1:8333"), "s3://" + prefix.split("/")[2]],
)
files = f"{prefix}/**/*.parquet"
columns = ", ".join(f'"{n}" {t}' for n, t, *_ in con.execute(f"DESCRIBE SELECT * FROM read_parquet('{files}')").fetchall())
con.execute(f"ATTACH 'nyc' AS lz (TYPE iceberg, ENDPOINT '{base}{W}/iceberg', TOKEN '{token}')")
con.execute(f"CREATE TABLE lz.main.yellow_trips ({columns})")
con.execute(
    f"INSERT INTO lz.main.yellow_trips SELECT * FROM read_parquet('{files}') WHERE tpep_pickup_datetime >= ? AND tpep_pickup_datetime < ?",
    [first.isoformat(), (first + dt.timedelta(days=7)).isoformat()],
)
n = con.execute("SELECT count(*) FROM lz.main.yellow_trips").fetchone()[0]
print(f"nyc.main.yellow_trips: {n:,} trips from {first} for seven days")
