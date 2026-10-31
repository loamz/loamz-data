#!/usr/bin/env bash
# The footprint budget: idle memory under 30 MB, ready in under 200 ms, one binary. Fails if one is missed.
# The server checks its sign-in service when it starts, so the local Keycloak must be up (docker compose -f deploy/dev/compose.yml up -d).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q --release -p loamz-data-server
WORK=$(mktemp -d)
trap 'kill "${PID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
PORT=${PORT:-18021}
LOAMZ_DATA_LISTEN=127.0.0.1:$PORT LOAMZ_DATA_DATABASE_URL="sqlite:$WORK/d.db?mode=rwc" LOAMZ_DATA_STORES_DIR=$WORK/v \
LOAMZ_DATA_OIDC_ISSUER=http://127.0.0.1:8082/realms/loamz target/release/loamz-data serve >/dev/null 2>&1 &
PID=$!
start=$(date +%s%N)
until curl -fs "http://127.0.0.1:$PORT/readyz" >/dev/null 2>&1; do :; done
ms=$(( ($(date +%s%N) - start) / 1000000 ))
sleep 1
mb=$(( $(awk '/VmRSS/ {print $2}' /proc/$PID/status) / 1024 ))
size=$(( $(stat -c %s target/release/loamz-data) / 1024 / 1024 ))
echo "ready in ${ms} ms (budget 200), idle memory ${mb} MB (budget 30), binary ${size} MB"
[ "$ms" -lt 200 ] && [ "$mb" -lt 30 ]
