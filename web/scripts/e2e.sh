#!/usr/bin/env bash
# The web app against a real Data server with sample data, driven in a browser. Needs: the release server built
# (cargo build --release -p loamz-data-server --examples), pnpm install, and Python Playwright with Chromium
# (PLAYWRIGHT_PY=/path/to/python; default python3).
set -euo pipefail
cd "$(dirname "$0")/../.."
export WORK=$(mktemp -d)
cleanup() { kill "$(cat "$WORK/pid" 2>/dev/null)" "${VITE:-0}" 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT
scripts/dev-ui.sh >"$WORK/dev-ui.log" 2>&1
(cd web && exec pnpm exec vite --host 127.0.0.1 --port 5180 >"$WORK/vite.log" 2>&1) & VITE=$!
until curl -fs http://127.0.0.1:5180/ >/dev/null 2>&1; do :; done
${PLAYWRIGHT_PY:-python3} web/e2e/flow.py
