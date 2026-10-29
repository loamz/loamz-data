#!/usr/bin/env bash
# The footprint budget: idle memory under 30 MB, ready in under 200 ms, one binary. Fails if one is missed.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q --release -p loamz-data-server
cargo build -q --release -p loamz-data-server --example dev_token
WORK=$(mktemp -d)
trap 'kill "${PID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
cargo run -q --release --example dev_token -- "$WORK/keys" ana >/dev/null
PORT=${PORT:-18021}
LOAMZ_DATA_LISTEN=127.0.0.1:$PORT LOAMZ_DATA_DATABASE_URL="sqlite:$WORK/d.db?mode=rwc" LOAMZ_DATA_VOLUMES_DIR=$WORK/v \
LOAMZ_DATA_OIDC_ISSUER=http://dev.local/realm LOAMZ_DATA_JWKS_FILE=$WORK/keys/jwks.json target/release/loamz-data serve >/dev/null 2>&1 &
PID=$!
start=$(date +%s%N)
until curl -fs "http://127.0.0.1:$PORT/readyz" >/dev/null 2>&1; do :; done
ms=$(( ($(date +%s%N) - start) / 1000000 ))
sleep 1
mb=$(( $(awk '/VmRSS/ {print $2}' /proc/$PID/status) / 1024 ))
size=$(( $(stat -c %s target/release/loamz-data) / 1024 / 1024 ))
echo "ready in ${ms} ms (budget 200), idle memory ${mb} MB (budget 30), binary ${size} MB"
[ "$ms" -lt 200 ] && [ "$mb" -lt 30 ]
