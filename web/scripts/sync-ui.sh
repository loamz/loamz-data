#!/usr/bin/env bash
# Copies @loamz/conversation (the event-stream reader, the state of a turn, a plain thread view) from loamz-ui into this app.
# A stop-gap like sync-design.sh: once loamz-ui is pushed and tagged this becomes a dependency on the exact tag. Never edit web/src/conversation/.
#   web/scripts/sync-ui.sh        (loamz-ui is the folder next to this repository; override with UI_SOURCE)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
src=${UI_SOURCE:-$here/../../loamz-ui/packages/conversation/src}
[ -d "$src" ] || { echo "loamz-ui source not found: $src" >&2; exit 2; }
rm -rf "$here/src/conversation" && mkdir -p "$here/src/conversation"
cp -R "$src"/. "$here/src/conversation/"
rm -f "$here/src/conversation/"*.test.ts   # its tests run in loamz-ui
echo "conversation copied from $src"
