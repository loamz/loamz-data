#!/usr/bin/env bash
# A Data server with sample data, for working on the web app. Prints the address and a token to paste.
#   scripts/dev-ui.sh          (then, in web/: pnpm dev, and open the address it prints with #token=<the token>)
# Needs `cargo build --release -p loamz-data-server --example dev_token` first. Everything lives in a temporary folder.
set -euo pipefail
cd "$(dirname "$0")/.."
WORK=${WORK:-$(mktemp -d)}
PORT=${PORT:-8020}; API=http://127.0.0.1:$PORT
cargo run -q --release --example dev_token -- "$WORK/keys" ana acme >"$WORK/ana.tok"
cargo run -q --release --example dev_token -- "$WORK/keys" bob acme >"$WORK/bob.tok"
ANA=$(cat "$WORK/ana.tok"); BOB=$(cat "$WORK/bob.tok")
export LOAMZ_DATA_LISTEN=127.0.0.1:$PORT LOAMZ_DATA_DATABASE_URL="sqlite:$WORK/d.db?mode=rwc" LOAMZ_DATA_VOLUMES_DIR=$WORK/volumes
export LOAMZ_DATA_OIDC_ISSUER=http://dev.local/realm LOAMZ_DATA_JWKS_FILE=$WORK/keys/jwks.json LOAMZ_DATA_ADMINS=ana
target/release/loamz-data serve >"$WORK/server.log" 2>&1 &
echo $! >"$WORK/pid"
until curl -fs "$API/readyz" >/dev/null 2>&1; do :; done

req() { curl -s -o "$WORK/out" -H "Authorization: Bearer $1" -H 'Content-Type: application/json' "${@:3}" "$API$2"; }
id() { sed -E 's/.*"id":"([^"]+)".*/\1/' "$WORK/out"; }
mk() { req "$ANA" /api/v1/securables -X POST -d "{\"kind\":\"$1\",\"parent_id\":\"$2\",\"name\":\"$3\",\"classification\":\"${4:-normal}\"}"; id; }
SALES=$(mk catalog "" sales); RAW=$(mk schema "$SALES" raw); CLEAN=$(mk schema "$SALES" clean)
UP=$(mk volume "$RAW" uploads); mk table "$CLEAN" orders personal >/dev/null; mk table "$CLEAN" customers personal >/dev/null; mk view "$CLEAN" weekly_totals >/dev/null
RES=$(mk volume "$CLEAN" results); HR=$(mk catalog "" people_ops); mk schema "$HR" payroll >/dev/null
printf 'order,region,amount\n1,north,120\n2,south,80\n3,north,200\n4,east,50\n5,south,300\n' >"$WORK/orders.csv"
head -c 48000 /dev/urandom >"$WORK/scan.bin"
for f in orders.csv scan.bin; do req "$ANA" "/api/v1/volumes/$UP/files/$f" -X PUT -H 'Content-Type: application/octet-stream' --data-binary @"$WORK/$f"; done
req "$ANA" "/api/v1/volumes/$UP/files/2026/q3/notes.txt" -X PUT --data-binary 'Q3 notes' >/dev/null
req "$ANA" "/api/v1/securables/$SALES/grants" -X PUT -d '{"principal":"bob","role":"reader"}'
req "$ANA" "/api/v1/securables/$RES/grants" -X PUT -d '{"principal":"agent-helper","role":"contributor"}'
req "$BOB" "/api/v1/volumes/$UP/files/x.txt" -X PUT --data-binary x >/dev/null          # a refusal, for the Activity screen
req "$BOB" "/api/v1/securables/$HR" >/dev/null                                           # and a hidden thing
echo "server  $API   (pid $(cat "$WORK/pid"), stop with: kill \$(cat $WORK/pid))"
echo "token   $ANA"
echo "bob     $BOB"
echo "work    $WORK"
