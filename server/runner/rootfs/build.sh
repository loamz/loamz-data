#!/usr/bin/env bash
# Builds the sandbox's root filesystem (Python and DuckDB) and unpacks it into a folder. Needs docker, and the machine's own
# architecture is what it builds for, so run it on each kind of compute machine.
#   server/runner/rootfs/build.sh [folder]        (default ~/.cache/loamz/rootfs)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dest=${1:-$HOME/.cache/loamz/rootfs}
duckdb=${DUCKDB_VERSION:-1.3.2}
docker build --quiet -t loamz-sandbox-rootfs --build-arg "DUCKDB_VERSION=$duckdb" "$here" >/dev/null
id=$(docker create loamz-sandbox-rootfs)
trap 'docker rm "$id" >/dev/null' EXIT
rm -rf "$dest" && mkdir -p "$dest"
# Device nodes are made by the sandbox itself, and cannot be unpacked without root.
docker export "$id" | tar -x -C "$dest" --exclude='dev/*' --no-same-owner 2>/dev/null || true
test -x "$dest/usr/local/bin/python3" || { echo "the root filesystem is incomplete" >&2; exit 1; }
echo "root filesystem ready in $dest (DuckDB $duckdb)"
