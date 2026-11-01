#!/usr/bin/env bash
# Runs Loamz Data on this computer against the local Keycloak: the server, then the web app at http://localhost:5180.
# Needs the dev stack (docker compose -f deploy/dev/compose.yml up -d; deploy/dev/seed.py) and `cargo build --release -p loamz-data-server`.
# Sign in with a person from .dev/accounts.json (made by deploy/dev/seed.py): ana is the account admin of the organization acme.
# Data bytes live in the R2 bucket loamz-data (credentials: files in ~/.config/loamz/r2/); the database is in .dev/ (not committed). Stop with Ctrl-C.
set -euo pipefail
cd "$(dirname "$0")/.."
r2=$HOME/.config/loamz/r2
export LOAMZ_DATA_LISTEN=127.0.0.1:8020 LOAMZ_DATA_DATABASE_URL="sqlite:$PWD/.dev/data.db?mode=rwc" LOAMZ_DATA_BUCKET=loamz-data LOAMZ_DATA_BUCKET_ENDPOINT="$(cat "$r2/endpoint")" LOAMZ_DATA_BUCKET_KEY_ID_FILE=$r2/key-id LOAMZ_DATA_BUCKET_SECRET_FILE=$r2/secret LOAMZ_DATA_BUCKET_ISSUER_TOKEN_FILE=$r2/api-token LOAMZ_DATA_SERVERLESS_TOKEN_FILE=$HOME/.config/loamz/serverless-token LOAMZ_DATA_AGENT_CLIENTS=loamz-agents
export LOAMZ_DATA_OIDC_ISSUER=http://127.0.0.1:8082/realms/loamz
trap 'kill 0' EXIT
target/release/loamz-data serve &
(cd web && [ -d node_modules ] || pnpm install)
(cd web && pnpm exec vite --host localhost --port 5180) &
wait
