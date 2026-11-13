#!/usr/bin/env python3
"""The baseline of the query path (proposal 0001, task 1): the same five queries on one Iceberg table, three ways in.

  direct   DuckDB reads the table straight from the bucket with the bucket's own key: the best case, no governance, no catalog.
  catalog  DuckDB attaches the Loamz Data REST catalog: every plan pays the catalog's round trips, and the data is read with vended
           credentials limited to the table's folder.
  machine  a SQL statement through the API, run on a compute machine in a gVisor sandbox (what a person in the SQL editor gets today).

Each query runs three times per way: the first is cold (a new connection, a new sandbox), the best of the rest is warm.
Needs the dev stack (deploy/dev/compose.yml), the release builds, the sandbox pieces, and DuckDB 2.0 (~/.cache/loamz/modal-venv).
The data is the real NYC yellow taxi trips (the TLC's public Parquet files, about 60 MB and 3 million trips a month), so it has real
skew, nulls and strings. Nothing here is a promise: it is the number every change to the query path has to beat.

  bench/run.py [months]         default 3 (January to March 2025); results go to bench/results/<date>.json and are printed as a table
"""
import json, os, signal, statistics, subprocess, sys, tempfile, time, urllib.error, urllib.request

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
MONTHS = int(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].isdigit() else 3
TLC = "https://d37ci6vzurychx.cloudfront.net"
API = "http://127.0.0.1:18060"
S3, BAO = "http://127.0.0.1:9000", "http://127.0.0.1:8200"
DUCK = os.path.expanduser("~/.cache/loamz/modal-venv/bin/python")
work = tempfile.mkdtemp(prefix="loamz-bench-")
procs = []

QUERIES = {
    "point": "SELECT * FROM {t} WHERE id = {mid}",
    "filtered_agg": "SELECT avg(fare), count(*) FROM {t} WHERE zone = 132 AND pickup_ts >= TIMESTAMP '2025-02-05'",
    "group_by": "SELECT zone, count(*) AS trips, sum(fare) AS fare FROM {t} GROUP BY zone ORDER BY fare DESC LIMIT 10",
    "join": "SELECT z.borough, count(*) AS trips, avg(t.tip) AS tip FROM {t} t JOIN {z} z ON t.zone = z.zone GROUP BY z.borough ORDER BY trips DESC",
    "full_scan": "SELECT count(*), sum(fare), max(distance), avg(passengers) FROM {t}",
}


def token(who):
    return subprocess.check_output([sys.executable, os.path.join(ROOT, "deploy/dev/token.py"), who], text=True).strip()


def call(method, path, tok, body=None, ok=(200, 201, 202, 204)):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode() if body is not None else None, method=method,
                                 headers={"Authorization": "Bearer " + tok, **({"Content-Type": "application/json"} if body is not None else {})})
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            raw = r.read()
            return r.status, (json.loads(raw) if raw and r.headers.get("Content-Type", "").startswith("application/json") else raw)
    except urllib.error.HTTPError as e:
        out = e.read()
        if e.code in ok:
            return e.code, out
        sys.exit(f"{method} {path} -> {e.code}: {out[:300]!r}")


def duck(code, env=None):
    r = subprocess.run([DUCK, "-c", code], capture_output=True, text=True, env={**os.environ, **(env or {})})
    if r.returncode:
        sys.exit("DuckDB failed: " + r.stderr[-600:])
    return r.stdout


def stop():
    for p in procs:
        p.send_signal(signal.SIGTERM)
    for p in procs:
        try:
            p.wait(5)
        except subprocess.TimeoutExpired:
            p.kill()


results = {}
try:
    os.makedirs(work + "/bao")
    open(work + "/bao/token", "w").write("loamz-dev-only")
    urllib.request.urlopen(urllib.request.Request(BAO + "/v1/secret/data/loamz-data/bucket", method="POST", headers={"X-Vault-Token": "loamz-dev-only"},
                           data=json.dumps({"data": {"key_id": "loamz-dev", "secret": "loamz-dev-only"}}).encode())).read()
    env = {**os.environ, "LOAMZ_DATA_LISTEN": "127.0.0.1:18060", "LOAMZ_DATA_DATABASE_URL": f"sqlite:{work}/d.db?mode=rwc", "LOAMZ_DATA_BUCKET": "loamz-data",
           "LOAMZ_DATA_BUCKET_ENDPOINT": S3, "LOAMZ_DATA_BUCKET_KEY_ID_REF": "loamz-data/bucket#key_id", "LOAMZ_DATA_BUCKET_SECRET_REF": "loamz-data/bucket#secret",
           "LOAMZ_DATA_BUCKET_STS_ENDPOINT": S3, "LOAMZ_DATA_OPENBAO_ADDR": BAO, "LOAMZ_DATA_OPENBAO_TOKEN_FILE": work + "/bao/token",
           "LOAMZ_DATA_OIDC_ISSUER": "http://127.0.0.1:8082/realms/loamz"}
    procs.append(subprocess.Popen([os.path.join(ROOT, "target/release/loamz-data"), "serve"], env=env, stdout=open(f"{work}/server.log", "w"), stderr=subprocess.STDOUT))
    for _ in range(100):
        try:
            urllib.request.urlopen(API + "/readyz", timeout=1)
            break
        except Exception:  # noqa: BLE001
            time.sleep(0.1)
    ana = token("ana")
    org = next(o["id"] for o in call("GET", "/api/v1/me", ana)[1]["orgs"] if o["alias"] == "acme")
    call("POST", f"/api/v1/orgs/{org}/account", ana, {}, ok=(201, 409))
    call("POST", f"/api/v1/orgs/{org}/workspaces", ana, {"name": "Bench"}, ok=(201, 409))
    ws = next(w["id"] for w in call("GET", f"/api/v1/orgs/{org}/workspaces", ana)[1]["items"] if w["name"] == "Bench")
    S = f"/api/v1/workspaces/{ws}"
    mk = lambda kind, parent, name: call("POST", f"{S}/securables", ana, {"kind": kind, "parent_id": parent, "name": name})[1]["id"]
    cat = mk("catalog", "", "bench"); mk("schema", cat, "main")
    ICE = f"{API}{S}/iceberg"

    # The table: real NYC taxi trips, written through the catalog the way any DuckDB client would.
    t0 = time.perf_counter()
    files = ", ".join(f"'{TLC}/trip-data/yellow_tripdata_2025-{m:02d}.parquet'" for m in range(1, MONTHS + 1))
    ROWS = int(duck(f"""
import duckdb
c = duckdb.connect(); c.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
c.execute("ATTACH 'bench' AS lz (TYPE iceberg, ENDPOINT '{ICE}', TOKEN '{ana}')")
c.execute("CREATE TABLE lz.main.trips (id BIGINT, pickup_ts TIMESTAMP, zone INTEGER, passengers INTEGER, distance DOUBLE, fare DOUBLE, tip DOUBLE)")
c.execute("CREATE TABLE lz.main.zones (zone INTEGER, borough VARCHAR)")
c.execute("INSERT INTO lz.main.zones SELECT LocationID, Borough FROM read_csv('{TLC}/misc/taxi_zone_lookup.csv')")
c.execute("INSERT INTO lz.main.trips SELECT row_number() OVER (), tpep_pickup_datetime, PULocationID, passenger_count, trip_distance, fare_amount, tip_amount FROM read_parquet([{files}]) WHERE tpep_pickup_datetime >= TIMESTAMP '2025-01-01'")
print(c.execute("SELECT count(*) FROM lz.main.trips").fetchone()[0])
""").strip().splitlines()[-1])
    results["load_seconds"] = round(time.perf_counter() - t0, 1)
    print(f"  loaded {ROWS:,} real trips ({MONTHS} months) through the catalog in {results['load_seconds']} s", flush=True)
    loaded = call("GET", f"{S}/iceberg/v1/bench/namespaces/main/tables/trips", ana)[1]
    location = loaded["metadata-location"]
    results["rows"] = ROWS
    mid = ROWS * 2 // 3

    def sql(name, t, z):
        return QUERIES[name].format(t=t, z=z, mid=mid)

    # Before timing anything: every way must see every row, or the numbers mean nothing.
    for way, setup, tn in (
        ("direct", "c.execute(\"CREATE SECRET (TYPE S3, KEY_ID 'loamz-dev', SECRET 'loamz-dev-only', ENDPOINT '127.0.0.1:9000', URL_STYLE 'path', USE_SSL false, REGION 'us-east-1')\")", f"iceberg_scan('{location}')"),
        ("catalog", f"c.execute(\"ATTACH 'bench' AS lz (TYPE iceberg, ENDPOINT '{ICE}', TOKEN '{ana}')\")", "lz.main.trips"),
    ):
        got = int(duck(f"""
import duckdb
c = duckdb.connect(); c.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
{setup}
print(c.execute("SELECT count(*) FROM {tn}").fetchone()[0])
""").strip().splitlines()[-1])
        if got != ROWS:
            sys.exit(f"{way} sees {got:,} rows, not {ROWS:,}")
    print(f"  direct and catalog both see all {ROWS:,} rows", flush=True)

    # direct and catalog: one connection per query (cold first, then warm repeats on the same connection)
    for way, setup, tn, zn in (
        ("direct", f"c.execute(\"CREATE SECRET (TYPE S3, KEY_ID 'loamz-dev', SECRET 'loamz-dev-only', ENDPOINT '127.0.0.1:9000', URL_STYLE 'path', USE_SSL false, REGION 'us-east-1')\")",
         f"iceberg_scan('{location}')", None),
        ("catalog", f"c.execute(\"ATTACH 'bench' AS lz (TYPE iceberg, ENDPOINT '{ICE}', TOKEN '{ana}')\")", "lz.main.trips", "lz.main.zones"),
    ):
        results[way] = {}
        for name in QUERIES:
            if way == "direct" and name == "join":
                continue  # the zones table's location is only known through the catalog; direct reads one table
            q = sql(name, tn, zn)
            out = duck(f"""
import duckdb, time, json
c = duckdb.connect(); c.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
{setup}
times = []
for _ in range(3):
    t = time.perf_counter(); rows = c.execute({q!r}).fetchall(); times.append(round(time.perf_counter() - t, 3))
print(json.dumps({{"times": times, "rows": len(rows)}}))
""")
            r = json.loads(out.strip().splitlines()[-1])
            results[way][name] = {"cold": r["times"][0], "warm": min(r["times"][1:]), "rows_returned": r["rows"]}
            print(f"  {way:8} {name:13} cold {r['times'][0]:6.2f}s  warm {min(r['times'][1:]):6.2f}s", flush=True)

    # machine: a compute agent connects out; every statement is a new sandbox, so cold is all there is
    _, t = call("POST", f"{S}/compute/targets", ana, {"name": "bench-machine", "max_memory_mb": 4096, "max_cpu_millis": 4000})
    open(work + "/agent.secret", "w").write(t["secret"]); os.chmod(work + "/agent.secret", 0o600)
    cache = os.path.expanduser("~/.cache/loamz")
    procs.append(subprocess.Popen([os.path.join(ROOT, "target/release/loamz-runner"), "agent"], stdout=open(f"{work}/agent.log", "w"), stderr=subprocess.STDOUT,
                                  env={**os.environ, "LOAMZ_DATA_URL": API, "LOAMZ_AGENT_SECRET_FILE": work + "/agent.secret", "LOAMZ_RUNSC": f"{cache}/runsc",
                                       "LOAMZ_ROOTFS": f"{cache}/rootfs", "LOAMZ_WORK": work + "/agent", "LOAMZ_PLATFORM": os.environ.get("LOAMZ_PLATFORM", "ptrace")}))
    for _ in range(150):
        time.sleep(0.2)
        if call("GET", f"{S}/compute/targets", ana)[1]["items"][0]["online"]:
            break
    results["machine"] = {}
    _, r = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT count(*) FROM bench.main.trips", "wait_s": 300})
    if r["state"] != "succeeded" or int(r["result"]["rows"][0][0]) != ROWS:
        sys.exit(f"the machine does not see all {ROWS:,} rows: {r['state']} {r.get('error')} {r.get('result')}")
    print(f"  the machine sees all {ROWS:,} rows", flush=True)
    for name in QUERIES:
        runs = []
        for _ in range(3):
            t = time.perf_counter()
            _, r = call("POST", f"{S}/sql/statements", ana, {"sql": sql(name, "bench.main.trips", "bench.main.zones"), "wait_s": 120})
            if r["state"] != "succeeded":
                sys.exit(f"{name} on the machine: {r['state']} {r.get('error')}")
            runs.append({"total": round(time.perf_counter() - t, 3), "queued_ms": r.get("queued_ms"), "run_ms": r.get("run_ms"), "worker": (r.get("result") or {}).get("stats")})
        best = min(runs, key=lambda x: x["total"])
        results["machine"][name] = {"cold": runs[0]["total"], "warm": best["total"], "queued_ms": best["queued_ms"], "run_ms": best["run_ms"], "worker": best["worker"]}
        print(f"  machine  {name:13} first {runs[0]['total']:6.2f}s  best {best['total']:6.2f}s  (queued {best['queued_ms']} ms, ran {best['run_ms']} ms, worker {best['worker']})", flush=True)
    call("DELETE", f"{S}/securables/{cat}", ana)
    time.sleep(4)
finally:
    stop()

os.makedirs(os.path.join(os.path.dirname(__file__), "results"), exist_ok=True)
out = os.path.join(os.path.dirname(__file__), "results", time.strftime("%Y-%m-%d") + ".json")
json.dump(results, open(out, "w"), indent=2)
print("\n| query | direct cold | direct warm | catalog cold | catalog warm | machine |\n|---|--:|--:|--:|--:|--:|")
for name in QUERIES:
    d, c, m = (results.get(w, {}).get(name) for w in ("direct", "catalog", "machine"))
    f = lambda x, k: "n/a" if not x else f"{x[k]:.2f} s"
    print(f"| {name} | {f(d, 'cold')} | {f(d, 'warm')} | {f(c, 'cold')} | {f(c, 'warm')} | {f(m, 'warm')} |")
print("\nwrote", out)
