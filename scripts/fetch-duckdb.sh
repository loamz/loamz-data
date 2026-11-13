#!/usr/bin/env bash
# Downloads DuckDB's own prebuilt shared library (the same release as the Python DuckDB in the sandbox) so the SQL worker links to it
# instead of compiling DuckDB from C++ source (ten to fifteen minutes and a lot of memory). Idempotent. Output: ~/.cache/loamz/duckdb
set -euo pipefail
version=${DUCKDB_VERSION:-2.0.0}
dest=${DUCKDB_LIB_DIR:-$HOME/.cache/loamz/duckdb}
case "$(uname -m)" in x86_64) arch=amd64 ;; aarch64|arm64) arch=arm64 ;; *) echo "unsupported machine $(uname -m)" >&2; exit 1 ;; esac
if [ -f "$dest/libduckdb.so" ] && [ "$(cat "$dest/VERSION" 2>/dev/null)" = "$version" ]; then exit 0; fi
mkdir -p "$dest"
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
curl -fsSL -o "$tmp" "https://github.com/duckdb/duckdb/releases/download/v$version/duckdb-shared-libs-linux-$arch.tar.gz"
tar -xzf "$tmp" -C "$dest"
echo "$version" >"$dest/VERSION"
echo "DuckDB $version library in $dest"
