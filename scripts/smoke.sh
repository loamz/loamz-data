#!/usr/bin/env bash
# The real server against the real (local) Keycloak, with real tokens for the seeded people. Needs the dev stack
# (docker compose -f deploy/dev/compose.yml up -d; deploy/dev/seed.py) and `cargo build --release -p loamz-data-server`.
# Walks the main path, and measures the footprint. Writes only under a temporary folder.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=target/release/loamz-data
WORK=$(mktemp -d)
trap 'kill "${PID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
PORT=${PORT:-18020}; API=http://127.0.0.1:$PORT
export LOAMZ_DATA_LISTEN=127.0.0.1:$PORT LOAMZ_DATA_DATABASE_URL="${SMOKE_DB:-sqlite:$WORK/data.db?mode=rwc}" LOAMZ_DATA_STORES_DIR=$WORK/stores
export LOAMZ_DATA_OIDC_ISSUER=http://127.0.0.1:8082/realms/loamz
tok() { python3 deploy/dev/token.py "$1"; }
ANA=$(tok ana); CYD=$(tok cyd); BOB=$(tok bob)

start=$(date +%s%N)
"$BIN" serve >"$WORK/server.log" 2>&1 &
PID=$!
until curl -fs "$API/readyz" >/dev/null 2>&1; do :; done
echo "start to ready: $(( ($(date +%s%N) - start) / 1000000 )) ms"

req() { curl -s -o "$WORK/out" -w '%{http_code}' -H "Authorization: Bearer $1" -H 'Content-Type: application/json' "${@:3}" "$API$2"; }
field() { python3 -c "import json,sys; print(json.load(open('$WORK/out'))$1)"; }
step() { echo "$1: $2 (want $3)"; [ "$2" = "$3" ] || { echo FAILED; cat "$WORK/out"; exit 1; }; }

step "me (ana)" "$(req "$ANA" /api/v1/me)" 200
echo "  organizations: $(field "['orgs']" | head -c 300)"
step "create the acme account" "$(req "$ANA" /api/v1/orgs/acme/account -X POST -d '{}')" 201
step "bob may not create it again" "$(req "$BOB" /api/v1/orgs/acme/account -X POST -d '{}')" 403
step "create a workspace" "$(req "$ANA" /api/v1/orgs/acme/workspaces -X POST -d '{"name":"Main"}')" 201; WS=$(field "['id']")
step "assign the group /finance/analysts" "$(req "$ANA" /api/v1/workspaces/$WS/members -X PUT -d '{"principal":"group:/finance/analysts","role":"user"}')" 204
step "cyd (in the group) opens it" "$(req "$CYD" /api/v1/workspaces/$WS)" 200
step "bob (not assigned) does not" "$(req "$BOB" /api/v1/workspaces/$WS)" 404
S=/api/v1/workspaces/$WS/securables
step "catalog" "$(req "$ANA" $S -X POST -d '{"kind":"catalog","name":"sales"}')" 201; C=$(field "['id']")
step "schema" "$(req "$ANA" $S -X POST -d "{\"kind\":\"schema\",\"parent_id\":\"$C\",\"name\":\"raw\"}")" 201; SC=$(field "['id']")
step "volume" "$(req "$ANA" $S -X POST -d "{\"kind\":\"volume\",\"parent_id\":\"$SC\",\"name\":\"uploads\"}")" 201; V=$(field "['id']")
echo "hello loamz" >"$WORK/hello.txt"
step "upload" "$(req "$ANA" /api/v1/workspaces/$WS/volumes/$V/files/hello.txt -X PUT -H 'Content-Type: application/octet-stream' --data-binary @"$WORK/hello.txt")" 201
step "cyd sees nothing yet" "$(req "$CYD" /api/v1/workspaces/$WS/volumes/$V/files)" 404
step "grant the group reader on the catalog" "$(req "$ANA" $S/$C/grants -X PUT -d '{"principal":"group:/finance/analysts","role":"reader"}')" 204
step "cyd downloads" "$(req "$CYD" /api/v1/workspaces/$WS/volumes/$V/files/hello.txt)" 200
cmp "$WORK/out" "$WORK/hello.txt" && echo "downloaded bytes match"
step "cyd may not write" "$(req "$CYD" /api/v1/workspaces/$WS/volumes/$V/files/x.txt -X PUT -H 'Content-Type: application/octet-stream' -d x)" 403
step "drop the schema" "$(req "$ANA" $S/$SC -X DELETE)" 202
sleep 1.2
[ ! -e "$WORK/stores/$V" ] && echo "volume bytes are gone from disk"
step "audit verify" "$(req "$ANA" /api/v1/orgs/acme/audit/verify)" 200
cat "$WORK/out"; echo
rss=$(awk '/VmRSS/ {print $2}' /proc/$PID/status)
echo "memory after the run: $((rss / 1024)) MB (budget 30)"
