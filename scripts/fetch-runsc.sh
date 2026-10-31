#!/usr/bin/env bash
# Downloads gVisor's runsc for this machine's architecture, checks it against the published checksum, and keeps it in
# ~/.cache/loamz/runsc. Run once per compute machine.
set -euo pipefail
release=${RUNSC_RELEASE:-20260126.0}
case "$(uname -m)" in x86_64) arch=x86_64 ;; aarch64|arm64) arch=aarch64 ;; *) echo "unsupported machine: $(uname -m)" >&2; exit 1 ;; esac
dest=${1:-$HOME/.cache/loamz/runsc}
mkdir -p "$(dirname "$dest")"
base="https://storage.googleapis.com/gvisor/releases/release/$release/$arch"
curl -fsSL -o "$dest.part" "$base/runsc"
want=$(curl -fsSL "$base/runsc.sha512" | cut -d' ' -f1)
[ "$(sha512sum "$dest.part" | cut -d' ' -f1)" = "$want" ] || { rm -f "$dest.part"; echo "the download does not match its published checksum" >&2; exit 1; }
chmod +x "$dest.part" && mv "$dest.part" "$dest"
echo "runsc $release ($arch) ready in $dest"
