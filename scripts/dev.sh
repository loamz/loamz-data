#!/usr/bin/env bash
# Runs Loamz Data on this computer against the local Keycloak and RustFS: the server, then the web app at http://localhost:5180.
# Needs the dev stack (docker compose -f deploy/dev/compose.yml up -d: Keycloak, Postgres, RustFS, OpenBao).
# Sign in with a person from .dev/accounts.json (made by deploy/dev/seed.py): ana is the account admin of the organization acme.
# Data bytes live in RustFS (the bucket loamz-data, made here if missing); the database is in .dev/ (not committed). Stop with Ctrl-C.
# Another S3-compatible store is only different settings: LOAMZ_DATA_BUCKET_* (see CLAUDE.md), nothing else changes.
set -euo pipefail
cd "$(dirname "$0")/.."
s3=http://127.0.0.1:9000 bao=http://127.0.0.1:8200 key=loamz-dev secret=loamz-dev-only bucket=loamz-data
down() { echo "$1 is not running: docker compose -f deploy/dev/compose.yml up -d" >&2; exit 1; }
# Idempotent: an existing bucket answers 409.
curl -s -o /dev/null -X PUT --aws-sigv4 "aws:amz:us-east-1:s3" --user "$key:$secret" "$s3/$bucket" || down RustFS
# Every secret lives in OpenBao (dev mode, root token loamz-dev-only). The server's login to it is the one file.
mkdir -p .dev
printf %s loamz-dev-only >.dev/openbao-token
put() { curl -sf -o /dev/null -X POST -H "X-Vault-Token: loamz-dev-only" -d "{\"data\": $2}" "$bao/v1/secret/data/$1" || down OpenBao; }
put loamz-data/bucket "{\"key_id\": \"$key\", \"secret\": \"$secret\"}"
# The secret of the Keycloak client the server creates organizations and people through (set in deploy/dev/compose.yml).
put loamz-data/directory '{"secret": "directory-dev-only"}'
# People sign in to OpenBao with Keycloak (dev mode forgets its settings when it restarts, so this runs every time).
deploy/dev/openbao_sso.py
export LOAMZ_DATA_LISTEN=127.0.0.1:8020 LOAMZ_DATA_DATABASE_URL="sqlite:$PWD/.dev/data.db?mode=rwc" LOAMZ_DATA_BUCKET=$bucket LOAMZ_DATA_BUCKET_ENDPOINT=$s3 LOAMZ_DATA_BUCKET_STS_ENDPOINT=$s3 LOAMZ_DATA_AGENT_CLIENTS=loamz-agents
export LOAMZ_DATA_OPENBAO_ADDR=$bao LOAMZ_DATA_OPENBAO_TOKEN_FILE=$PWD/.dev/openbao-token LOAMZ_DATA_BUCKET_KEY_ID_REF=loamz-data/bucket#key_id LOAMZ_DATA_BUCKET_SECRET_REF=loamz-data/bucket#secret LOAMZ_DATA_DIRECTORY_SECRET_REF=loamz-data/directory#secret
export LOAMZ_DATA_OIDC_ISSUER=http://127.0.0.1:8082/realms/loamz
export VITE_OIDC_AUTHORITY=http://127.0.0.1:8082/realms/loamz
[ -x target/release/loamz-data ] || cargo build --release -p loamz-data-server
trap 'kill 0' EXIT
target/release/loamz-data serve &
if [ -x target/release/loamz-runner ] && [ -f .dev/agent.secret ]; then
  CACHE=${LOAMZ_CACHE:-$HOME/.cache/loamz}
  LOAMZ_DATA_URL="http://127.0.0.1:8020" LOAMZ_AGENT_SECRET_FILE="$PWD/.dev/agent.secret" \
    LOAMZ_RUNSC="${CACHE}/runsc" LOAMZ_ROOTFS="${CACHE}/rootfs" LOAMZ_WORK="$(mktemp -d)" \
    LOAMZ_PLATFORM="${LOAMZ_PLATFORM:-ptrace}" target/release/loamz-runner agent &
fi
(cd web && [ -d node_modules ] || pnpm install)
(cd web && pnpm exec vite --host localhost --port 5180) &
wait
