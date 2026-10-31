#!/usr/bin/env bash
# Copies the Loamz design system (tokens, primitives, layout) into this app. The copy in loamz-chat/web/src/design is the
# source now (the agents console that used to own it was removed). Never edit web/src/design/ by hand: edit it there and run this.
#   web/scripts/sync-design.sh        (loamz-chat is the folder next to this repository; override with DESIGN_SOURCE)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
src=${DESIGN_SOURCE:-$here/../../loamz-chat/web/src/design}
[ -d "$src" ] || { echo "design source not found: $src" >&2; exit 2; }
rm -rf "$here/src/design" && mkdir -p "$here/src/design"
cp -R "$src"/. "$here/src/design/"
echo "design system copied from $src"
