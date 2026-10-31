#!/usr/bin/env bash
# Governance and Data run here. The job runs on a compute machine that only ever talks to the Data API.
#
#   REMOTE=ubuntu@host KEY=~/.ssh/key REMOTE_ENV='LOAMZ_RUNSC=... LOAMZ_ROOTFS=...' scripts/compute-demo.sh   (remote)
#   LOAMZ_RUNSC=... LOAMZ_ROOTFS=... LOAMZ_ROOTLESS=1 LOAMZ_PLATFORM=ptrace scripts/compute-demo.sh          (this machine)
#
# Remote: the compute machine reaches Data through a reverse tunnel (ssh -R) to this machine's loopback, so Data is
# never on a public port. Later, with Data on Cloudflare, only DATA_URL changes.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=target/release/loamz-data
WORK=$(mktemp -d)
trap 'kill "${PID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
PORT=${PORT:-18030}; API=http://127.0.0.1:$PORT; TUNNEL_PORT=${TUNNEL_PORT:-18030}

cargo run -q --release --example dev_token -- "$WORK/keys" ana acme >"$WORK/tok"; T=$(cat "$WORK/tok")
export LOAMZ_DATA_LISTEN=127.0.0.1:$PORT LOAMZ_DATA_DATABASE_URL="sqlite:$WORK/d.db?mode=rwc" LOAMZ_DATA_VOLUMES_DIR=${DEMO_VOLUMES:-$WORK/volumes}
export LOAMZ_DATA_OIDC_ISSUER=http://dev.local/realm LOAMZ_DATA_JWKS_FILE=$WORK/keys/jwks.json LOAMZ_DATA_ADMINS=ana
"$BIN" serve >"$WORK/server.log" 2>&1 & PID=$!
until curl -fs "$API/readyz" >/dev/null 2>&1; do :; done

req() { curl -s -o "$WORK/out" -w '%{http_code}' -H "Authorization: Bearer $T" "${@:2}" "$API$1"; }
id() { sed -E 's/.*"id":"([^"]+)".*/\1/' "$WORK/out"; }
req /api/v1/securables -X POST -d '{"kind":"catalog","name":"shop"}' >/dev/null; C=$(id)
req /api/v1/securables -X POST -d "{\"kind\":\"schema\",\"parent_id\":\"$C\",\"name\":\"main\"}" >/dev/null; S=$(id)
req /api/v1/securables -X POST -d "{\"kind\":\"volume\",\"parent_id\":\"$S\",\"name\":\"sales\"}" >/dev/null; IN=$(id)
req /api/v1/securables -X POST -d "{\"kind\":\"volume\",\"parent_id\":\"$S\",\"name\":\"results\"}" >/dev/null; OUT=$(id)
printf 'order,region,amount\n1,north,120\n2,south,80\n3,north,200\n4,east,50\n5,south,300\n' >"$WORK/orders.csv"
echo "upload orders.csv: $(req "/api/v1/volumes/$IN/files/orders.csv" -X PUT --data-binary @"$WORK/orders.csv")"

python3 - "$WORK/job.json" "$T" "$IN" "$OUT" "http://127.0.0.1:$TUNNEL_PORT" <<'PY'
import json, sys
path, token, vin, vout, url = sys.argv[1:]
script = '''
import csv, json, os
rows = list(csv.DictReader(open("/inputs/sales/orders.csv")))
by = {}
for r in rows: by[r["region"]] = by.get(r["region"], 0) + int(r["amount"])
open("/workspace/summary.json", "w").write(json.dumps(by, sort_keys=True))
print("rows:", len(rows), "cpus visible:", os.cpu_count())
try:
    import socket; socket.create_connection(("1.1.1.1", 53), timeout=3); print("NETWORK REACHED")
except OSError: print("no network")
'''
import os
if os.environ.get("JOB_SCRIPT"): script = open(os.environ["JOB_SCRIPT"]).read()
json.dump({"data_url": url, "token": token, "inputs": [{"volume": vin, "name": "sales"}], "output_volume": vout,
           "script": script, "memory_mb": 2048, "cpu_millis": 1000, "timeout_s": 120}, open(path, "w"))
PY

echo "== running the job"
if [ -n "${REMOTE:-}" ]; then
  ssh -i "$KEY" -o BatchMode=yes -R "127.0.0.1:$TUNNEL_PORT:127.0.0.1:$PORT" "$REMOTE" \
    "sudo env $REMOTE_ENV ${REMOTE_RUNNER:-/home/ubuntu/loamz-compute/loamz-runner} run-job" <"$WORK/job.json" >"$WORK/outcome.json"
else
  target/release/loamz-runner run-job <"$WORK/job.json" >"$WORK/outcome.json"
fi
python3 -c "import json;o=json.load(open('$WORK/outcome.json'));print(json.dumps({k:o[k] for k in ('exit_code','timed_out','stdout','stderr','downloaded','uploaded','memory_mb','cpu_millis')},indent=1))"

echo "== what arrived in the results volume (read back through governance)"
req "/api/v1/volumes/$OUT/files" >/dev/null; cat "$WORK/out"; echo
F=$(sed -E 's/.*"path":"([^"]+summary.json)".*/\1/' "$WORK/out"); req "/api/v1/volumes/$OUT/files/$F" >/dev/null; echo "summary.json: $(cat "$WORK/out")"
echo "== audit trail (who did what; the runner's calls are the caller's)"
req "/api/v1/audit?limit=8" >/dev/null; python3 -c "import json;[print(' ',e['actor'],e['action'],e['target'][:60],e['outcome']) for e in json.load(open('$WORK/out'))['items']]"
