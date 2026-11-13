#!/usr/bin/env python3
"""Puts real data into the dev workspace so there is something to look at: the NYC yellow taxi trips (the TLC's public Parquet files) as
nyc.main.trips, and the taxi zones as nyc.main.zones, in the organization acme's workspace Main. The rows are written through the
catalog the way any DuckDB client would, so they land in the workspace's own prefix in RustFS. Needs the dev stack running
(scripts/dev.sh), DuckDB 2.0 (~/.cache/loamz/modal-venv) and internet. Safe to run again: it stops if the table is there.

  scripts/sample_data.py [months]        default 1 (January 2025, about 3.5 million trips, 60 MB)
"""
import json, os, subprocess, sys, urllib.error, urllib.request

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
API = os.environ.get("LOAMZ_API", "http://127.0.0.1:8020")
TLC = "https://d37ci6vzurychx.cloudfront.net"
DUCK = os.path.expanduser("~/.cache/loamz/modal-venv/bin/python")
months = int(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].isdigit() else 1
tok = subprocess.check_output([sys.executable, os.path.join(ROOT, "deploy/dev/token.py"), "ana"], text=True).strip().splitlines()[-1]


def call(method, path, body=None, ok=(200, 201, 204, 409)):
    req = urllib.request.Request(API + path, method=method, data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Authorization": "Bearer " + tok, "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            raw = r.read()
            return json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        if e.code in ok:
            return None
        sys.exit(f"{method} {path} -> {e.code}: {e.read()[:200]!r}")


me = call("GET", "/api/v1/me")
ws = next((w["id"] for w in me["workspaces"] if w["org_alias"] == "acme" and w["name"] == "Main"), None)
if not ws:
    sys.exit("ana has no workspace Main in acme: open the console as ana once, then run this again")
W = f"/api/v1/workspaces/{ws}"
items = lambda parent: (call("GET", f"{W}/securables?parent_id={parent}") or {"items": []})["items"]
cat = next((c["id"] for c in items("") if c["name"] == "nyc"), None)
if not cat:
    cat = call("POST", f"{W}/securables", {"kind": "catalog", "parent_id": "", "name": "nyc"})["id"]
schema = next((s["id"] for s in items(cat) if s["name"] == "main"), None)
if not schema:
    schema = call("POST", f"{W}/securables", {"kind": "schema", "parent_id": cat, "name": "main"})["id"]
if any(t["name"] == "trips" for t in items(schema)):
    sys.exit("nyc.main.trips is already there")

files = ", ".join(f"'{TLC}/trip-data/yellow_tripdata_2025-{m:02d}.parquet'" for m in range(1, months + 1))
code = f"""
import duckdb
c = duckdb.connect(); c.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
c.execute("ATTACH 'nyc' AS lz (TYPE iceberg, ENDPOINT '{API}{W}/iceberg', TOKEN '{tok}')")
c.execute("CREATE TABLE lz.main.zones (zone INTEGER, borough VARCHAR, name VARCHAR, service_zone VARCHAR)")
c.execute("INSERT INTO lz.main.zones SELECT LocationID, Borough, Zone, service_zone FROM read_csv('{TLC}/misc/taxi_zone_lookup.csv')")
c.execute("CREATE TABLE lz.main.trips (pickup_ts TIMESTAMP, dropoff_ts TIMESTAMP, zone INTEGER, dropoff_zone INTEGER, passengers INTEGER, distance DOUBLE, fare DOUBLE, tip DOUBLE, total DOUBLE)")
c.execute("INSERT INTO lz.main.trips SELECT tpep_pickup_datetime, tpep_dropoff_datetime, PULocationID, DOLocationID, passenger_count, trip_distance, fare_amount, tip_amount, total_amount FROM read_parquet([{files}]) WHERE tpep_pickup_datetime >= TIMESTAMP '2025-01-01'")
print(c.execute("SELECT count(*) FROM lz.main.trips").fetchone()[0])
"""
r = subprocess.run([DUCK, "-c", code], capture_output=True, text=True)
if r.returncode:
    sys.exit("DuckDB failed: " + r.stderr[-500:])
print(f"nyc.main.trips: {int(r.stdout.strip().splitlines()[-1]):,} trips, and nyc.main.zones, in acme / Main")
