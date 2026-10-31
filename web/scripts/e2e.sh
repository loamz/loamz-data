#!/usr/bin/env bash
# The web app against a real, empty Data server and the real local Keycloak, driven in a browser by real sign-ins.
# Needs: the dev stack up and seeded (docker compose -f deploy/dev/compose.yml up -d; deploy/dev/seed.py), the release server built
# (cargo build --release -p loamz-data-server), pnpm install, and Python Playwright with Chromium (PLAYWRIGHT_PY, default python3).
set -euo pipefail
cd "$(dirname "$0")/../.."
WORK=$(mktemp -d)
trap 'kill "${SERVER:-0}" "${VITE:-0}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
LOAMZ_DATA_LISTEN=127.0.0.1:8020 LOAMZ_DATA_DATABASE_URL="sqlite:$WORK/data.db?mode=rwc" LOAMZ_DATA_STORES_DIR=$WORK/stores \
  LOAMZ_DATA_OIDC_ISSUER=http://127.0.0.1:8082/realms/loamz target/release/loamz-data serve >"$WORK/server.log" 2>&1 & SERVER=$!
(cd web && exec pnpm exec vite --host localhost --port 5180 >"$WORK/vite.log" 2>&1) & VITE=$!
until curl -fs http://127.0.0.1:8020/readyz >/dev/null 2>&1 && curl -fs http://localhost:5180/ >/dev/null 2>&1; do :; done
${PLAYWRIGHT_PY:-python3} web/e2e/flow.py
