#!/usr/bin/env python3
"""Spike for the agent sandbox (proposal 0006): can ordinary Python readers use the runner's storage broker as their S3 endpoint?

What it checks, against a real RustFS (deploy/dev) and the real broker (server/runner/examples/broker_spike.rs):
  1. Polars, DuckDB and pyarrow read Parquet through a loopback port in front of the broker's Unix socket (path-style, plain http,
     placeholder keys), exactly as a sandbox would.
  2. Range reads: a one-column or a filtered read fetches a fraction of the file (bytes the broker took from storage).
  3. Cache: what a second read costs, for a small file (served whole from memory) and a large one (read by ranges).
  4. Isolation: nothing outside the leased folder is served; writes and deletes are refused; the readers hold no real credential.

  python3 bench/broker_readers.py            needs: the dev stack up, `cargo build -p loamz-runner --examples`, and a Python with
                                             polars, pyarrow and duckdb (set SPIKE_PY, or it makes ~/.cache/loamz/spike-venv)
"""
import json, os, socket, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
S3 = "http://127.0.0.1:9000"
KEY, SECRET, BUCKET = "loamz-dev", "loamz-dev-only", "loamz-data"
FOLDER = f"spike-{int(time.time())}"


def venv_python():
    py = os.environ.get("SPIKE_PY")
    if py:
        return py
    home = os.path.expanduser("~/.cache/loamz/spike-venv")
    py = home + "/bin/python"
    if not os.path.exists(py):
        subprocess.run(["uv", "venv", home], check=True)
        subprocess.run(["uv", "pip", "install", "--python", py, "polars", "pyarrow", "duckdb"], check=True)
    return py


# ---- the reader side runs in a child process, so each reader is measured on its own ----
READERS = r'''
import json, sys, time
port, bucket, folder, big, small = sys.argv[1:6]
import polars as pl, pyarrow.dataset as ds, pyarrow.fs as pafs, duckdb
opts = {"aws_endpoint_url": f"http://127.0.0.1:{port}", "aws_access_key_id": "placeholder", "aws_secret_access_key": "placeholder",
        "aws_region": "auto", "aws_allow_http": "true"}
con = duckdb.connect()
con.execute("INSTALL httpfs; LOAD httpfs")
con.execute(f"CREATE SECRET (TYPE s3, KEY_ID 'placeholder', SECRET 'placeholder', ENDPOINT '127.0.0.1:{port}', URL_STYLE 'path', USE_SSL false, REGION 'auto')")
fs = pafs.S3FileSystem(endpoint_override=f"127.0.0.1:{port}", scheme="http", access_key="placeholder", secret_key="placeholder", region="auto")
which, name, kind = sys.argv[6:9]
path = f"{bucket}/{folder}/{name}"
t = time.time()
if which == "polars":
    lf = pl.scan_parquet(f"s3://{path}", storage_options=opts)
    out = {"full": lambda: lf.select(pl.len()).collect().item(),
           "column": lambda: lf.select(pl.col("val").sum()).collect().item(),
           "filter": lambda: lf.filter(pl.col("id") < 1000).select(pl.col("val").max()).collect().item()}[kind]()
elif which == "duckdb":
    q = {"full": "SELECT count(*) FROM read_parquet('s3://%s')",
         "column": "SELECT sum(val) FROM read_parquet('s3://%s')",
         "filter": "SELECT max(val) FROM read_parquet('s3://%s') WHERE id < 1000"}[kind]
    out = con.execute(q % path).fetchone()[0]
else:
    d = ds.dataset(path, filesystem=fs, format="parquet")
    out = {"full": lambda: d.count_rows(),
           "column": lambda: d.to_table(columns=["val"]).num_rows,
           "filter": lambda: d.to_table(columns=["val"], filter=ds.field("id") < 1000).num_rows}[kind]()
print(json.dumps({"seconds": round(time.time() - t, 3), "result": str(out)}))
'''


def _pump(a, b):
    try:
        while (data := a.recv(65536)):
            b.sendall(data)
    except OSError:
        pass
    finally:
        try:
            b.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def bridge(path):
    """What the sandbox does: a port on 127.0.0.1 in front of the broker's Unix socket."""
    srv = socket.socket(); srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); srv.bind(("127.0.0.1", 0)); srv.listen(64)

    def serve():
        while True:
            c, _ = srv.accept()
            u = socket.socket(socket.AF_UNIX)
            try:
                u.connect(path)
            except OSError:
                c.close(); continue
            threading.Thread(target=_pump, args=(c, u), daemon=True).start()
            threading.Thread(target=_pump, args=(u, c), daemon=True).start()
    threading.Thread(target=serve, daemon=True).start()
    return srv.getsockname()[1]


def main():
    py = venv_python()
    tmp = tempfile.mkdtemp(prefix="loamz-spike-")
    # 1. data: a large file of many row groups (~100 MB) and a small one (~10 MB), written locally then uploaded to RustFS
    gen = (f"import duckdb; c=duckdb.connect(); "
           f"c.execute(\"COPY (SELECT i AS id, (i%100) AS grp, random() AS val, md5(i::VARCHAR)||md5((i*7)::VARCHAR) AS txt FROM range({{n}}) t(i)) "
           f"TO '{{p}}' (FORMAT parquet, ROW_GROUP_SIZE 100000)\")")
    for name, n in (("big.parquet", 3_000_000), ("small.parquet", 300_000)):
        subprocess.run([py, "-c", gen.format(n=n, p=f"{tmp}/{name}")], check=True)
    up = (f"import pyarrow.fs as f, sys; fs=f.S3FileSystem(endpoint_override='127.0.0.1:9000', scheme='http', access_key='{KEY}', secret_key='{SECRET}', region='auto'); "
          f"[ (lambda n: f.copy_files('{tmp}/'+n, '{BUCKET}/{FOLDER}/'+n, destination_filesystem=fs))(n) for n in ('big.parquet','small.parquet') ]")
    subprocess.run([py, "-c", up], check=True)
    sizes = {n: os.path.getsize(f"{tmp}/{n}") for n in ("big.parquet", "small.parquet")}
    print("data:", {k: f"{v / 1e6:.1f} MB" for k, v in sizes.items()})

    example = os.path.join(ROOT, "target/debug/examples/broker_spike")
    results = []

    def run_broker():
        d = tempfile.mkdtemp(prefix="loamz-broker-")
        p = subprocess.Popen([example, d, S3, BUCKET, KEY, SECRET, FOLDER], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(100):
            if os.path.exists(d + "/socket") and os.path.exists(d + "/stats.json"):
                break
            time.sleep(0.1)
        return p, d

    def stats(d):
        time.sleep(0.3)
        return json.load(open(d + "/stats.json"))

    def read(port, which, name, kind):
        r = subprocess.run([py, "-c", READERS, str(port), BUCKET, FOLDER, "big.parquet", "small.parquet", which, name, kind], capture_output=True, text=True)
        if r.returncode:
            return {"error": r.stderr.strip().splitlines()[-1][:200] if r.stderr.strip() else "failed"}
        return json.loads(r.stdout.strip().splitlines()[-1])

    for name in ("big.parquet", "small.parquet"):
        for which in ("polars", "duckdb", "pyarrow"):
            for kind in ("full", "column", "filter"):
                p, d = run_broker()                      # a new broker: cold cache
                try:
                    port = bridge(d + "/socket")
                    s0 = stats(d); a = read(port, which, name, kind); s1 = stats(d)
                    b = read(port, which, name, kind); s2 = stats(d)  # the same read again: warm
                finally:
                    p.terminate(); p.wait()
                row = {"file": name, "reader": which, "read": kind,
                       "cold_s": a.get("seconds"), "cold_mb": round((s1["bytes"] - s0["bytes"]) / 1e6, 2),
                       "warm_s": b.get("seconds"), "warm_mb": round((s2["bytes"] - s1["bytes"]) / 1e6, 2),
                       "warm_cache_hits": s2["hits"] - s1["hits"], "error": a.get("error") or b.get("error")}
                results.append(row)
                print(json.dumps(row))

    # local-disk baseline for the same three reads (polars)
    base = subprocess.run([py, "-c",
        f"import polars as pl,time; lf=pl.scan_parquet('{tmp}/big.parquet'); t=time.time(); lf.select(pl.len()).collect(); a=time.time()-t; "
        f"t=time.time(); lf.select(pl.col('val').sum()).collect(); b=time.time()-t; t=time.time(); lf.filter(pl.col('id')<1000).select(pl.col('val').max()).collect(); c=time.time()-t; "
        f"print(round(a,3),round(b,3),round(c,3))"], capture_output=True, text=True).stdout.split()
    print("local disk, polars, big file: full/column/filter seconds =", base)

    # 4. isolation, through the bridge, with anything signed
    p, d = run_broker()
    try:
        port = bridge(d + "/socket")
        def call(method, key, body=None):
            req = urllib.request.Request(f"http://127.0.0.1:{port}/{BUCKET}/{key}", method=method, data=body)
            try:
                with urllib.request.urlopen(req, timeout=10) as r:
                    return r.status
            except urllib.error.HTTPError as e:
                return e.code
        checked = {
            "leased file": call("GET", f"{FOLDER}/small.parquet"),
            "another folder": call("GET", "some-other-folder/x"),
            "the bucket root": call("GET", ""),
            "write": call("PUT", f"{FOLDER}/new.bin", b"x"),
            "delete": call("DELETE", f"{FOLDER}/small.parquet"),
            "path trick": call("GET", f"{FOLDER}/../loamz-data/x"),
        }
        print("isolation (status codes):", checked)
    finally:
        p.terminate(); p.wait()
    ok = checked["leased file"] == 200 and all(v >= 400 for k, v in checked.items() if k != "leased file")
    print("isolation ok" if ok else "ISOLATION FAILED")
    return 0 if ok and not any(r["error"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
