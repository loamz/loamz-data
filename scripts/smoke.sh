#!/usr/bin/env bash
# Starts the release binary against a throwaway folder, walks the main path with curl, and measures the footprint.
# Writes only under a temporary folder. Usage: scripts/smoke.sh  (needs `cargo build --release` done first)
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=target/release/loamz-data
WORK=$(mktemp -d)
trap 'kill "${PID:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
PORT=${PORT:-18020}
API=http://127.0.0.1:$PORT

cargo run -q --release --example dev_token -- "$WORK/keys" ana acme >"$WORK/ana.tok"
cargo run -q --release --example dev_token -- "$WORK/keys" bob acme >"$WORK/bob.tok"
ANA=$(cat "$WORK/ana.tok"); BOB=$(cat "$WORK/bob.tok")

export LOAMZ_DATA_LISTEN=127.0.0.1:$PORT LOAMZ_DATA_DATABASE_URL="${SMOKE_DB:-sqlite:$WORK/data.db?mode=rwc}"
VOL=${SMOKE_VOLUMES:-$WORK/volumes}; export LOAMZ_DATA_VOLUMES_DIR=$VOL LOAMZ_DATA_OIDC_ISSUER=http://dev.local/realm
export LOAMZ_DATA_JWKS_FILE=$WORK/keys/jwks.json LOAMZ_DATA_ADMINS=ana

start=$(date +%s%N)
"$BIN" serve >"$WORK/server.log" 2>&1 &
PID=$!
until curl -fs "$API/readyz" >/dev/null 2>&1; do :; done
echo "start to ready: $(( ($(date +%s%N) - start) / 1000000 )) ms"

req() { curl -s -o "$WORK/out" -w '%{http_code}' -H "Authorization: Bearer $1" "${@:3}" "$API$2"; }
id() { sed -E 's/.*"id":"([^"]+)".*/\1/' "$WORK/out"; }
step() { echo "$1: $2 (want $3)"; [ "$2" = "$3" ] || { echo FAILED; cat "$WORK/out"; exit 1; }; }

step "me" "$(req "$ANA" /api/v1/me)" 200
step "catalog" "$(req "$ANA" /api/v1/securables -X POST -d '{"kind":"catalog","name":"sales"}')" 201; C=$(id)
step "schema" "$(req "$ANA" /api/v1/securables -X POST -d "{\"kind\":\"schema\",\"parent_id\":\"$C\",\"name\":\"raw\"}")" 201; S=$(id)
step "volume" "$(req "$ANA" /api/v1/securables -X POST -d "{\"kind\":\"volume\",\"parent_id\":\"$S\",\"name\":\"uploads\"}")" 201; V=$(id)
step "bob sees nothing yet" "$(req "$BOB" "/api/v1/volumes/$V/files")" 404
step "grant reader" "$(req "$ANA" "/api/v1/securables/$C/grants" -X PUT -d '{"principal":"bob","role":"reader"}')" 204
echo "hello loamz" >"$WORK/hello.txt"
step "upload" "$(req "$ANA" "/api/v1/volumes/$V/files/hello.txt" -X PUT --data-binary @"$WORK/hello.txt")" 201
step "bob downloads" "$(req "$BOB" "/api/v1/volumes/$V/files/hello.txt")" 200
cmp "$WORK/out" "$WORK/hello.txt" && echo "downloaded bytes match"
step "bob may not write" "$(req "$BOB" "/api/v1/volumes/$V/files/x.txt" -X PUT -d x)" 403
step "erase bob" "$(req "$ANA" /api/v1/erase -X POST -d '{"subject":"bob"}')" 202
sleep 1.2
step "bob is gone" "$(req "$BOB" "/api/v1/securables/$C")" 404
step "drop catalog" "$(req "$ANA" "/api/v1/securables/$C" -X DELETE)" 202
sleep 1.2
[ ! -e "$VOL/$V" ] && echo "volume bytes are gone from disk"
step "audit verify" "$(req "$ANA" /api/v1/audit/verify)" 200
cat "$WORK/out"; echo
rss=$(awk '/VmRSS/ {print $2}' /proc/$PID/status)
echo "memory after the run: $((rss / 1024)) MB (budget 30)"
