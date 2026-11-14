#!/usr/bin/env python3
"""A real query and a real job, end to end: a person's real Keycloak token, a table of a few rows, a compute agent running in gVisor.
Needs the dev stack (deploy/dev; RustFS included), `cargo build --release -p loamz-data-server -p loamz-runner`, ~/.cache/loamz/{runsc,rootfs}
(scripts/fetch-runsc.sh, server/runner/rootfs/build.sh). Uses only the standard library, and a throwaway folder.

  scripts/compute-smoke.py            (prints each step and what came back)

  REMOTE=ubuntu@host KEY=~/.ssh/key scripts/compute-smoke.py
      Runs the agent on that machine instead (as root, so gVisor holds each run to its memory and CPU limits). It connects out to
      this server through an ssh reverse tunnel. Needs ~/loamz-compute/{loamz-runner,runsc,rootfs-duckdb} there (see CLAUDE.md).
"""
import json, os, signal, subprocess, sys, tempfile, time, urllib.error, urllib.request

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
API = "http://127.0.0.1:18040"
work = tempfile.mkdtemp(prefix="loamz-smoke-")
procs = []
# RustFS and OpenBao from deploy/dev/compose.yml (throwaway development values): the bucket key goes into the vault, as the server expects.
S3 = "http://127.0.0.1:9000"
BAO = "http://127.0.0.1:8200"
os.makedirs(work + "/bao", exist_ok=True)
open(work + "/bao/token", "w").write("loamz-dev-only")
urllib.request.urlopen(urllib.request.Request(BAO + "/v1/secret/data/loamz-data/bucket", data=json.dumps({"data": {"key_id": "loamz-dev", "secret": "loamz-dev-only"}}).encode(),
                                              headers={"X-Vault-Token": "loamz-dev-only"}, method="POST")).read()
DUCK = os.path.expanduser("~/.cache/loamz/modal-venv/bin/python")


def token(who):
    return subprocess.check_output([sys.executable, os.path.join(ROOT, "deploy/dev/token.py"), who], text=True).strip()


def call(method, path, tok, body=None, raw=None, ok=(200, 201, 202, 204)):
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    req = urllib.request.Request(API + path, data=data, method=method, headers={"Authorization": "Bearer " + tok, **({"Content-Type": "application/json"} if body is not None else {})})
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            raw_out = r.read()
            return r.status, (json.loads(raw_out) if raw_out and r.headers.get("Content-Type", "").startswith("application/json") else raw_out)
    except urllib.error.HTTPError as e:
        out = e.read()
        if e.code in ok:
            return e.code, out
        sys.exit(f"{method} {path} -> {e.code}: {out[:300]!r}")


def step(msg):
    print(f"  {msg}", flush=True)


def stop():
    for p in procs:
        p.send_signal(signal.SIGTERM)
    for p in procs:
        try:
            p.wait(5)
        except subprocess.TimeoutExpired:
            p.kill()


try:
    env = {**os.environ, "LOAMZ_DATA_LISTEN": "127.0.0.1:18040", "LOAMZ_DATA_DATABASE_URL": f"sqlite:{work}/d.db?mode=rwc",
           "LOAMZ_DATA_BUCKET": "loamz-data", "LOAMZ_DATA_BUCKET_ENDPOINT": S3, "LOAMZ_DATA_BUCKET_KEY_ID_REF": "loamz-data/bucket#key_id", "LOAMZ_DATA_BUCKET_SECRET_REF": "loamz-data/bucket#secret", "LOAMZ_DATA_BUCKET_STS_ENDPOINT": S3,
           "LOAMZ_DATA_FLIGHT_LISTEN": "127.0.0.1:18041", "LOAMZ_DATA_STORAGE_ALLOW_PRIVATE": "true", "LOAMZ_DATA_STORAGE_ALLOW_HTTP_DEV": "true", "LOAMZ_DATA_OPENBAO_ADDR": BAO, "LOAMZ_DATA_OPENBAO_TOKEN_FILE": work + "/bao/token", "LOAMZ_DATA_OIDC_ISSUER": "http://127.0.0.1:8082/realms/loamz"}
    procs.append(subprocess.Popen([os.path.join(ROOT, "target/release/loamz-data"), "serve"], env=env, stdout=open(f"{work}/server.log", "w"), stderr=subprocess.STDOUT))
    for _ in range(100):
        try:
            urllib.request.urlopen(API + "/readyz", timeout=1)
            break
        except Exception:  # noqa: BLE001
            time.sleep(0.1)
    ana = token("ana")
    _, me = call("GET", "/api/v1/me", ana)
    org = next(o["id"] for o in me["orgs"] if o["alias"] == "acme")
    call("POST", f"/api/v1/orgs/{org}/account", ana, {}, ok=(201, 409))
    _, w = call("POST", f"/api/v1/orgs/{org}/workspaces", ana, {"name": "Smoke"}, ok=(201, 409))
    ws = next(x["id"] for x in call("GET", f"/api/v1/orgs/{org}/workspaces", ana)[1]["items"] if x["name"] == "Smoke")
    S = f"/api/v1/workspaces/{ws}"
    WSID = ws
    mk = lambda kind, parent, name: call("POST", f"{S}/securables", ana, {"kind": kind, "parent_id": parent, "name": name})[1]["id"]
    cat = mk("catalog", "", "shop"); sch = mk("schema", cat, "main")
    out = mk("volume", sch, "results")
    seed = subprocess.run([DUCK, os.path.join(ROOT, "scripts/iceberg_seed.py"), API + S + "/iceberg", "shop", "main.orders", "order_id BIGINT, region VARCHAR, amount BIGINT"],
                          input=json.dumps([[1, "north", 120], [2, "south", 80], [3, "north", 200], [4, "east", 50], [5, "south", 300]]), text=True, capture_output=True,
                          env={**os.environ, "LZ_TOKEN": ana})
    if seed.returncode:
        sys.exit("seeding failed: " + seed.stderr[-400:])
    orders = next(i["id"] for i in call("GET", f"{S}/securables?parent_id={sch}", ana)[1]["items"] if i["name"] == "orders")
    step("an Iceberg table of 5 rows was made and filled through the catalog, as any DuckDB client would (its files are in the RustFS bucket)")

    _, t = call("POST", f"{S}/compute/targets", ana, {"name": "this-machine", "max_memory_mb": 2048, "max_cpu_millis": 1000})
    secret = f"{work}/agent.secret"
    open(secret, "w").write(t["secret"]); os.chmod(secret, 0o600)
    remote, key = os.environ.get("REMOTE"), os.path.expanduser(os.environ.get("KEY", "~/.ssh/id_ed25519"))
    if remote:
        home = "/home/ubuntu/loamz-compute"
        subprocess.run(["ssh", "-i", key, "-o", "BatchMode=yes", remote, f"umask 077; cat > {home}/agent.secret"], input=t["secret"].encode(), check=True)
        agent_cmd = ["ssh", "-i", key, "-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes", "-R", "127.0.0.1:18040:127.0.0.1:18040", remote,
                     f"sudo env LOAMZ_DATA_URL=http://127.0.0.1:18040 LOAMZ_AGENT_SECRET_FILE={home}/agent.secret LOAMZ_RUNSC={home}/runsc "
                     f"LOAMZ_ROOTFS={home}/rootfs-duckdb LOAMZ_WORK=/var/tmp/loamz-compute {home}/loamz-runner agent"]
        procs.append(subprocess.Popen(agent_cmd, stdout=open(f"{work}/agent.log", "w"), stderr=subprocess.STDOUT))
    else:
        cache = os.path.expanduser("~/.cache/loamz")
        aenv = {**os.environ, "LOAMZ_DATA_URL": API, "LOAMZ_AGENT_SECRET_FILE": secret, "LOAMZ_RUNSC": f"{cache}/runsc", "LOAMZ_ROOTFS": f"{cache}/rootfs",
                "LOAMZ_WORK": f"{work}/agent", "LOAMZ_PLATFORM": os.environ.get("LOAMZ_PLATFORM", "ptrace")}
        procs.append(subprocess.Popen([os.path.join(ROOT, "target/release/loamz-runner"), "agent"], env=aenv, stdout=open(f"{work}/agent.log", "w"), stderr=subprocess.STDOUT))
    for _ in range(150):
        time.sleep(0.2)
        if call("GET", f"{S}/compute/targets", ana)[1]["items"][0]["online"]:
            break
    step("the agent connected out and the compute shows as online")

    t0 = time.time()
    code, r = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT region, count(*) AS orders, sum(amount) AS total FROM shop.main.orders GROUP BY region ORDER BY total DESC", "wait_s": 60})
    step(f"the query came back {code} {r['state']} in {time.time() - t0:.1f} s (a cold sandbox, one process, one query)")
    if r["state"] != "succeeded":
        print("   ", json.dumps({k: r[k] for k in ("state", "error", "stderr", "stdout")}))
        print(open(f"{work}/agent.log").read()[-1500:])
        sys.exit(1)
    print("   columns:", [c["name"] for c in r["result"]["columns"]]); [print("   ", row) for row in r["result"]["rows"]]

    code, r2 = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT * FROM read_csv('/etc/passwd')", "wait_s": 5}, ok=(400,))
    step(f"a statement that reads a file is refused before it reaches compute ({code}): {json.loads(r2)['detail'][:70]}")
    _, r3 = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT nope FROM shop.main.orders", "wait_s": 60})
    step(f"a mistake in the SQL is an answer, not a crash: {r3['state']}: {r3['error'][:80]}")

    job = "import duckdb, json\nc=duckdb.connect(config={'extension_directory':'/opt/duckdb-extensions','autoinstall_known_extensions':'false','autoload_known_extensions':'false'})\nc.execute('LOAD iceberg')\nv=json.load(open('/inputs/orders/loamz-table.json'))['metadata'].split('/')[-1].removesuffix('.metadata.json')\nrows,total=c.sql(f\"select count(*), sum(amount) from iceberg_scan('/inputs/orders', version='{v}', allow_moved_paths=true)\").fetchall()[0]\nopen('/workspace/summary.json','w').write(json.dumps({'rows':rows,'total':int(total)}))\nprint('rows', rows)\ntry:\n    import socket; socket.create_connection(('1.1.1.1',53),timeout=2); print('NETWORK')\nexcept OSError: print('no network')\n"
    _, j = call("POST", f"{S}/runs", ana, {"script": job, "inputs": [orders], "output": out, "timeout_s": 90})
    for _ in range(300):
        time.sleep(0.5)
        run = call("GET", f"{S}/runs/{j['id']}", ana)[1]
        if run["state"] not in ("queued", "running"):
            break
    step(f"the job ended {run['state']}; it printed {run['stdout'].split()}")
    files = call("GET", f"{S}/volumes/{out}/files", ana)[1]["items"]
    summary = call("GET", f"{S}/volumes/{out}/files/{files[0]['path']}", ana)[1]
    step(f"and its file reached the results volume through the API: {files[0]['path'].split('/')[-1]} = {summary.decode()}")
    # ---- the write path (proposal 0005): a file is imported as a table through the two doors ----
    def wait_run(run_id, seconds=150):
        for _ in range(seconds * 2):
            time.sleep(0.5)
            r = call("GET", f"{S}/runs/{run_id}", ana)[1]
            if r["state"] not in ("queued", "running"):
                return r
        sys.exit("the run did not end")

    call("PUT", f"{S}/volumes/{out}/files/people.csv", ana, raw=b"id,name,age\n1,Ana,31\n2,Bo,45\n3,Cy,27\n")
    _, imp = call("POST", f"{S}/imports", ana, {"target": "shop.main.people", "volume": out, "path": "people.csv"})
    done = wait_run(imp["id"])
    if done["state"] != "succeeded":
        print(json.dumps({k: done[k] for k in ("state", "error", "stderr", "stdout")}))
        print(open(f"{work}/agent.log").read()[-2000:])
        sys.exit("the import failed")
    people = next(i["id"] for i in call("GET", f"{S}/securables?parent_id={sch}", ana)[1]["items"] if i["name"] == "people")
    step(f"a CSV in a volume was imported as shop.main.people through the doors: {done['stdout'].strip()}")
    _, cnt = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT count(*) AS n, sum(age) AS ages FROM shop.main.people", "wait_s": 60})
    step(f"and a statement reads it back: {cnt['result']['rows']}")
    assert cnt["result"]["rows"] == [[3, "103"]], cnt
    call("PUT", f"{S}/volumes/{out}/files/more.csv", ana, raw=b"id,name,age\n4,Di,52\n")
    _, imp2 = call("POST", f"{S}/imports", ana, {"target": "shop.main.people", "volume": out, "path": "more.csv", "mode": "append"})
    appended = wait_run(imp2["id"])
    if appended["state"] != "succeeded":
        sys.exit("the append failed: " + json.dumps({k: appended[k] for k in ("state", "error", "stderr", "stdout")})[:1500])
    _, cnt2 = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT count(*) FROM shop.main.people", "wait_s": 60})
    step(f"an append added a row: {cnt2['result']['rows']}")
    assert cnt2["result"]["rows"] == [[4]], cnt2

    _, compact_run = call(
        "POST",
        f"{S}/tables/{people}/maintenance",
        ana,
        {"action": "compact"},
    )
    compacted = wait_run(compact_run["id"])
    if compacted["state"] != "succeeded":
        sys.exit("compaction failed: " + json.dumps({k: compacted[k] for k in ("state", "error", "stderr")})[:6000])
    step(f"compaction rewrote the table without changing its rows: {compacted['stdout'].strip()}")

    time.sleep(2)
    call("PUT", f"{S}/volumes/{out}/files/last.csv", ana, raw=b"id,name,age\n5,Eli,38\n")
    _, imp3 = call("POST", f"{S}/imports", ana, {"target": "shop.main.people", "volume": out, "path": "last.csv", "mode": "append"})
    appended_again = wait_run(imp3["id"])
    if appended_again["state"] != "succeeded":
        sys.exit("the second append failed: " + json.dumps({k: appended_again[k] for k in ("state", "error", "stderr")})[:1500])
    _, expire_run = call(
        "POST",
        f"{S}/tables/{people}/maintenance",
        ana,
        {"action": "expire", "older_than": 1},
    )
    expired = wait_run(expire_run["id"])
    if expired["state"] != "succeeded":
        sys.exit("snapshot expiry failed: " + json.dumps({k: expired[k] for k in ("state", "error", "stderr")})[:6000])
    _, after_expiry = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT count(*) FROM shop.main.people", "wait_s": 60})
    assert after_expiry["result"]["rows"] == [[5]], after_expiry
    step(f"expiry kept the live snapshot readable: {expired['stdout'].strip()} rows={after_expiry['result']['rows'][0][0]}")

    # Replace swaps the rows; an address on the machine's own network is refused before anything is fetched.
    call("PUT", f"{S}/volumes/{out}/files/fresh.csv", ana, raw=b"id,name,age\n9,Zed,60\n8,Yan,20\n")
    _, imp3 = call("POST", f"{S}/imports", ana, {"target": "shop.main.people", "volume": out, "path": "fresh.csv", "mode": "replace"})
    replaced = wait_run(imp3["id"])
    if replaced["state"] != "succeeded":
        sys.exit("the replace failed: " + json.dumps({k: replaced[k] for k in ("state", "error", "stderr", "stdout")})[:1500])
    _, cnt3 = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT count(*), sum(age) FROM shop.main.people", "wait_s": 60})
    step(f"a replace swapped the rows: {cnt3['result']['rows']}")
    assert cnt3["result"]["rows"] == [[2, "80"]], cnt3
    _, bad = call("POST", f"{S}/imports", ana, {"target": "shop.main.fetched", "url": "https://127.0.0.1/x.csv"})
    refused = wait_run(bad["id"])
    step(f"an address on the machine's own network was refused: {refused['state']}, {refused['error'] or refused['stderr']!r}"[:200])
    assert refused["state"] == "failed" and "x.csv" not in json.dumps(refused), refused
    _, http = call("POST", f"{S}/imports", ana, {"target": "shop.main.fetched", "url": "http://example.com/x.csv"}, ok=(400,))

    # A whole result as Arrow: the machine puts the file beside the data (in the bucket), and the server only signs a short-lived address.
    many = ", ".join(f"shop.main.orders t{i}" for i in range(6))
    t1 = time.time()
    _, big = call("POST", f"{S}/sql/statements", ana, {"sql": f"SELECT t0.order_id, t0.region, t0.amount FROM {many}", "export": True, "wait_s": 90})
    if big["state"] != "succeeded":
        print(open(f"{work}/agent.log").read()[-1500:])
        sys.exit("the exported statement failed: " + json.dumps({k: big[k] for k in ("state", "error", "stderr")})[:800])
    kept = big["result"]["export"]
    _, link = call("GET", f"{S}/runs/{big['id']}/result", ana)
    assert link["format"] == "application/vnd.apache.arrow.stream" and link["bytes"] == kept["bytes"], link
    # Read straight from the bucket with the signed address alone: no credential of ours is sent.
    import pyarrow.ipc as ipc
    with urllib.request.urlopen(link["url"], timeout=60) as r:
        table = ipc.open_stream(r).read_all()
    assert table.num_rows == kept["rows"] == 5 ** 6 and table.column_names == ["order_id", "region", "amount"], (table.num_rows, kept)
    step(f"a whole result ({table.num_rows} rows, {kept['bytes']} bytes) was kept beside the data and read as an Arrow stream straight from the bucket in {time.time() - t1:.1f} s")
    code, _ = call("GET", f"{S}/runs/{big['id']}/result", "not-a-token", ok=(401,))
    step(f"and the address is only given to someone with a token ({code})")

    # A whole result saved as Parquet in the person's volume, by the machine, with a key that makes sending it again harmless.
    home = mk("volume", sch, "agent_files")
    body = {"sql": f"SELECT t0.order_id, t0.region, t0.amount FROM {many}", "save": {"volume": home, "thread": "t1"}, "idempotency_key": "turn-1", "wait_s": 90}
    t2 = time.time()
    _, saved = call("POST", f"{S}/sql/statements", ana, body)
    if saved["state"] != "succeeded":
        print(open(f"{work}/agent.log").read()[-1500:])
        sys.exit("saving the result failed: " + json.dumps({k: saved[k] for k in ("state", "error", "stderr")})[:800])
    kept = saved["result"]["export"]
    assert kept["format"] == "parquet" and kept["volume"] == home and kept["path"] == f"threads/t1/results/{saved['id']}.parquet" and kept["rows"] == 5 ** 6, kept
    assert len(saved["result"]["rows"]) == 20, "the preview"
    _, again = call("POST", f"{S}/sql/statements", ana, body)
    assert again["id"] == saved["id"], "the same key is the same run"
    import io, pyarrow.parquet as pq
    _, raw = call("GET", f"{S}/volumes/{home}/files/{kept['path']}", ana)
    table = pq.read_table(io.BytesIO(raw))
    assert table.num_rows == 5 ** 6 and table.schema.names == ["order_id", "region", "amount"], table.schema
    step(f"a whole result ({kept['rows']} rows, {kept['bytes']} bytes) was saved as Parquet in the person's volume at {kept['path']} in {time.time() - t2:.1f} s; sending it again returned the same run")

    # Code in the sandbox reads the saved result in place, through the machine's storage broker: nothing is downloaded before it starts.
    reader = (
        "import json, urllib.request, urllib.error, os\nimport loamz, polars as pl\n"
        f"p = loamz.path('agent_files', '{kept['path']}')\n"
        "n = pl.scan_parquet(p).select(pl.len()).collect().item()\n"
        "few = pl.scan_parquet(p).filter(pl.col('order_id') < 3).select(pl.len()).collect().item()\n"
        "m = loamz.duckdb().execute(f\"SELECT count(*) FROM read_parquet('{p}')\").fetchone()[0]\n"
        "local = loamz.fetch('agent_files', '" + kept['path'] + "')\n"
        "# the sandbox may only read: a write and another volume's folder are refused, and no real key is in reach\n"
        "req = urllib.request.Request('http://' + loamz.endpoint() + '/loamz-data/' + p.split('/', 3)[3] + '.x', data=b'x', method='PUT')\n"
        "try:\n    urllib.request.urlopen(req); put = 200\nexcept urllib.error.HTTPError as e:\n    put = e.code\n"
        "try:\n    urllib.request.urlopen('http://' + loamz.endpoint() + '/loamz-data/elsewhere/x'); other = 200\nexcept urllib.error.HTTPError as e:\n    other = e.code\n"
        "print(json.dumps({'rows': n, 'filtered': few, 'duckdb': m, 'fetched_bytes': os.path.getsize(local), 'put': put, 'other': other, 'keys': [v for k, v in os.environ.items() if 'loamz-dev' in v]}))\n"
    )
    for attempt in (1, 2):
        _, rj = call("POST", f"{S}/runs", ana, {"script": reader, "reads": [home], "output": out, "timeout_s": 120})
        rr = wait_run(rj["id"])
        if rr["state"] != "succeeded":
            print(open(f"{work}/agent.log").read()[-1500:])
            sys.exit("the job that reads the volume in place failed: " + rr["error"] + " | " + rr["stderr"][-900:])
        seen = json.loads(rr["stdout"].strip().splitlines()[-1])
        assert seen["rows"] == seen["duckdb"] == 5 ** 6 and seen["filtered"] == 2 * 5 ** 5 and seen["put"] == 403 and seen["other"] == 403 and seen["keys"] == [], seen
        step(f"a job read the saved result in place through the broker (Polars, DuckDB and a one-file fetch), run {attempt}: {seen['rows']} rows, a write is refused ({seen['put']}), another folder is refused ({seen['other']}), no key in its environment")
    served = [l for l in open(f"{work}/agent.log").read().splitlines() if "storage broker for a job" in l]
    step("broker traffic per run: " + " | ".join(l.split("served_bytes")[1].split(" storage")[0].strip()[:60] for l in served[-2:]))

    # A job commits what it leaves in /workspace/out to the person's volume, under this run's own path, each file create-only. A link
    # in the folder and a name that is not plain are refused and reported, and a run that did not commit everything is failed.
    writer = ("import loamz, os, polars as pl\n"
              "pl.DataFrame({'x': [1, 2, 3]}).write_csv(loamz.out('summary.csv'))\n"
              "open(loamz.out('charts/one.txt'), 'w').write('chart')\n")
    _, c1 = call("POST", f"{S}/runs", ana, {"script": writer, "commit": {"volume": home, "thread": "t1"}, "timeout_s": 120})
    done1 = wait_run(c1["id"])
    if done1["state"] != "succeeded":
        print(open(f"{work}/agent.log").read()[-1500:])
        sys.exit("the commit job failed: " + done1["error"] + " | " + done1["stderr"][-600:])
    got1 = json.loads(done1["result"]) if isinstance(done1["result"], str) else done1["result"]
    paths = sorted(f["path"] for f in got1["committed"])
    assert paths == [f"threads/t1/runs/{c1['id']}/charts/one.txt", f"threads/t1/runs/{c1['id']}/summary.csv"] and not got1["failed"], got1
    _, back = call("GET", f"{S}/volumes/{home}/files/{paths[1]}", ana)
    assert back.decode().split() == ["x", "1", "2", "3"], back
    step(f"a job committed its outputs to the thread: {[p.split('/', 4)[-1] for p in paths]}; the file read back through the API")
    sneaky = ("import loamz, os\nopen(loamz.out('ok.txt'), 'w').write('fine')\n"
              "os.symlink('/etc/passwd', '/workspace/out/leak')\nopen('/workspace/out/bad;name', 'w').write('x')\n")
    _, c2 = call("POST", f"{S}/runs", ana, {"script": sneaky, "commit": {"volume": home, "thread": "t1"}, "timeout_s": 120})
    done2 = wait_run(c2["id"])
    got2 = json.loads(done2["result"]) if isinstance(done2["result"], str) else done2["result"]
    names = sorted(f["path"].rsplit("/", 1)[-1] for f in got2["committed"])
    assert done2["state"] == "failed" and names == ["ok.txt"] and len(got2["failed"]) == 2, (done2["state"], got2)
    _, listing = call("GET", f"{S}/volumes/{home}/files", ana)
    assert not any(f"{c2['id']}/leak" in f["path"] or "bad" in f["path"] for f in listing["items"]), "a refused file must not be in the volume"
    step(f"a link and an odd name were refused and reported, the good file stayed, and the run failed: {done2['error'][:110]}")
    _, closed = call("DELETE", f"{S}/volumes/{home}/threads/t1", ana)
    _, left = call("GET", f"{S}/volumes/{home}/files", ana)
    assert not any(f["path"].startswith("threads/t1/") for f in left["items"]), left
    code, _ = call("POST", f"{S}/runs", ana, {"script": writer, "commit": {"volume": home, "thread": "t1"}, "timeout_s": 60}, ok=(409,))
    step(f"deleting the thread removed its files, and new work on it is refused for now ({code})")

    # The BI connection: an Arrow Flight SQL client (ADBC, as a BI tool's driver would be) signs in with the workspace id and a token,
    # runs a statement, lists what it may see, and is refused with a wrong token. The rows are the same as the HTTP API's.
    flight = subprocess.run(
        [os.path.expanduser("~/.cache/loamz/spike-venv/bin/python"), "-c", (
            "import sys, json\nimport adbc_driver_flightsql.dbapi as fl\n"
            "ws, tok, good = sys.argv[1:4]\n"
            "con = fl.connect('grpc://127.0.0.1:18041', db_kwargs={'username': ws, 'password': tok})\n"
            "cur = con.cursor()\n"
            "cur.execute('SELECT region, count(*) AS n, sum(amount) AS total FROM shop.main.orders GROUP BY region ORDER BY region')\n"
            "t = cur.fetch_arrow_table()\n"
            "objs = con.adbc_get_objects(depth='tables').read_all().to_pylist()\n"
            "names = sorted(f\"{c['catalog_name']}.{s['db_schema_name']}.{x['table_name']}\" for c in objs for s in (c['catalog_db_schemas'] or []) for x in (s['db_schema_tables'] or []))\n"
            "bad = None\n"
            "try:\n    fl.connect('grpc://127.0.0.1:18041', db_kwargs={'username': ws, 'password': 'not-a-token'})\n    c2 = fl.connect('grpc://127.0.0.1:18041', db_kwargs={'username': ws, 'password': 'not-a-token'}); c2.cursor().execute('SELECT 1')\nexcept Exception as e:\n    bad = type(e).__name__\n"
            "print(json.dumps({'rows': t.to_pylist(), 'types': [str(f.type) for f in t.schema], 'tables': names, 'bad': bad}, default=str))\n"),
         WSID, ana, "x"], capture_output=True, text=True)
    if flight.returncode:
        sys.exit("the BI connection failed: " + flight.stderr[-800:])
    seen = json.loads(flight.stdout.strip().splitlines()[-1])
    assert [r["region"] for r in seen["rows"]] == ["east", "north", "south"] and [r["n"] for r in seen["rows"]] == [1, 2, 2], seen
    assert "shop.main.orders" in seen["tables"] and "shop.main.people" in seen["tables"], seen["tables"]
    assert seen["bad"], "a wrong token must be refused"
    step(f"a BI client (ADBC over Arrow Flight SQL) signed in, ran a statement ({len(seen['rows'])} groups, types {seen['types']}), listed {len(seen['tables'])} tables it may see, and a wrong token was refused ({seen['bad']})")

    # A catalog on a customer's storage location: its bytes are read and written with the location's own credential, in its own
    # folder of the bucket (here the same RustFS, a folder of its own), and nothing of it is in the installation's default storage.
    _, cred = call("POST", f"{S}/storage/credentials", ana, {"name": "rustfs", "provider": "rustfs", "endpoint": S3,
                                                              "secret": {"kind": "sts", "access_key_id": "loamz-dev", "secret_access_key": "loamz-dev-only"}})
    folder = f"smoke-{int(time.time())}"
    _, loc = call("POST", f"{S}/storage/locations", ana, {"name": "cust", "credential_id": cred["id"], "bucket": "loamz-data", "prefix": folder})
    ccat = call("POST", f"{S}/securables", ana, {"kind": "catalog", "name": "cust", "location": "cust"})[1]["id"]
    csch = mk("schema", ccat, "main")
    cvol = mk("volume", csch, "inbox")
    call("PUT", f"{S}/volumes/{cvol}/files/p.csv", ana, raw=b"id,name\n1,Ana\n2,Bo\n3,Cy\n4,Di\n")
    _, imp4 = call("POST", f"{S}/imports", ana, {"target": "cust.main.people", "volume": cvol, "path": "p.csv"})
    onloc = wait_run(imp4["id"])
    if onloc["state"] != "succeeded":
        print(open(f"{work}/agent.log").read()[-1500:])
        sys.exit("the import into a table on a customer location failed: " + json.dumps({k: onloc[k] for k in ("state", "error", "stderr", "stdout")})[:1500])
    _, cnt4 = call("POST", f"{S}/sql/statements", ana, {"sql": "SELECT count(*) FROM cust.main.people", "wait_s": 60})
    assert cnt4["state"] == "succeeded" and cnt4["result"]["rows"] == [[4]], cnt4
    step(f"a table on the customer's location was written through its credential and read back: {cnt4['result']['rows']}")
    listing = subprocess.run([DUCK, "-c", "import duckdb; c=duckdb.connect(); c.execute('INSTALL httpfs'); c.execute('LOAD httpfs'); "
                              f"c.execute(\"CREATE SECRET (TYPE s3, KEY_ID 'loamz-dev', SECRET 'loamz-dev-only', ENDPOINT '127.0.0.1:9000', URL_STYLE 'path', USE_SSL false)\"); "
                              f"print(c.execute(\"SELECT count(*) FROM glob('s3://loamz-data/{folder}/**')\").fetchone()[0])"], capture_output=True, text=True)
    step(f"and its files are under the location's own folder {folder}/ in the bucket: {listing.stdout.strip() or listing.stderr[-160:]} files")

    # The doors refuse what the run did not declare, from inside the sandbox.
    probe = (
        "import sys, json, urllib.request, urllib.error\nsys.path.insert(0, '/inputs/job')\nimport loamz_doors\n"
        f"cat, s3 = loamz_doors.start('{ws}')\nseen = {{}}\n"
        "def code(url, method='GET', data=None):\n"
        "    try:\n        return urllib.request.urlopen(urllib.request.Request(url, data=data, method=method), timeout=20).status\n"
        "    except urllib.error.HTTPError as e:\n        return e.code\n    except Exception as e:\n        return type(e).__name__\n"
        f"seen['securables'] = code(f'http://127.0.0.1:{{cat.split(\":\")[2].split(\"/\")[0]}}/api/v1/workspaces/{ws}/securables')\n"
        "seen['other bucket'] = code(f'http://{s3}/other-bucket/x')\n"
        "seen['elsewhere in the bucket'] = code(f'http://{s3}/loamz-data/nowhere/x')\n"
        "seen['delete'] = code(f'http://{s3}/loamz-data/nowhere/x', 'DELETE')\n"
        "print(json.dumps(seen))\n"
    )
    _, j3 = call("POST", f"{S}/runs", ana, {"script": probe, "table_writes": ["shop.main.people"], "timeout_s": 60})
    r3 = wait_run(j3["id"])
    seen = json.loads(r3["stdout"].strip().splitlines()[-1]) if r3["state"] == "succeeded" else None
    step(f"inside a write lease, the doors answer: {seen}")
    assert seen and all(v == 403 for v in seen.values()), (r3["state"], r3["error"], r3["stderr"][-400:], seen)

    if remote:
        # As root, gVisor holds a run to its limits. 1 CPU of two, and 2 GB of the machine's twelve.
        busy = "import os, time\nt0=time.time()\nkids=[]\nfor _ in range(2):\n    p=os.fork()\n    if p==0:\n        end=t0+5\n        while time.time()<end: pass\n        os._exit(0)\n    kids.append(p)\n[os.waitpid(k,0) for k in kids]\nwall=time.time()-t0\nc=os.times()\nprint('cpus worth', round((c.children_user+c.children_system)/wall,2), 'of', os.cpu_count(), 'visible')\n"
        eat = "b=bytearray(1536*1024*1024); b[::4096]=b'x'*len(b[::4096]); print('1.5 GB ok', flush=True)\nc=bytearray(2560*1024*1024); c[::4096]=b'x'*len(c[::4096]); print('2.5 GB ok (should not get here)')\n"
        for name, script in (("cpu", busy), ("memory", eat)):
            _, j2 = call("POST", f"{S}/runs", ana, {"script": script, "inputs": [], "output": out, "memory_mb": 2048, "cpu_millis": 1000, "timeout_s": 120})
            for _ in range(600):
                time.sleep(0.5)
                run2 = call("GET", f"{S}/runs/{j2['id']}", ana)[1]
                if run2["state"] not in ("queued", "running"):
                    break
            step(f"{name} limit job: {run2['state']}; printed {run2['stdout'].split()}; {run2['error']}")
    print("smoke ok")
finally:
    try:
        # Takes the smoke catalog away, and with it every file it left in the bucket.
        call("DELETE", f"{S}/securables/{cat}", ana)
        time.sleep(4)
    except NameError:
        pass
    stop()
    if os.environ.get("REMOTE"):
        subprocess.run(["ssh", "-i", os.path.expanduser(os.environ.get("KEY", "~/.ssh/id_ed25519")), "-o", "BatchMode=yes", os.environ["REMOTE"], "rm -f /home/ubuntu/loamz-compute/agent.secret; sudo pkill -f '[l]oamz-compute/loamz-runner agent'; true"], check=False)
    log = open(f"{work}/agent.log").read() if os.path.exists(f"{work}/agent.log") else ""
    if "--keep" not in sys.argv and "ok" in log + "ok":
        pass
