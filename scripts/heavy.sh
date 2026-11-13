#!/usr/bin/env bash
# Runs a heavy command (a build, the test suites) under one machine-wide lock, at low priority, so several people or agents working on this
# computer never compile and test at the same moment. It waits its turn.
#   scripts/heavy.sh make check
#   scripts/heavy.sh cargo test -p loamz-data-server --test compute
set -euo pipefail
mkdir -p "$HOME/.cache/loamz"
exec flock "$HOME/.cache/loamz/heavy.lock" nice -n 10 "$@"
