#!/usr/bin/env bash
# Closes the serverless SQL endpoint's running containers at once (they also stop by themselves after a minute idle).
# The Modal key is read from ~/.config/loamz/modal.env; nothing is printed of it. Needs the tools venv (see CLAUDE.md).
set -euo pipefail
modal=${MODAL:-$HOME/.cache/loamz/modal-venv/bin/modal}
set -a; . "$HOME/.config/loamz/modal.env"; set +a
export MODAL_TOKEN_ID MODAL_TOKEN_SECRET=$MODAL_SECRET_KEY
"$modal" container list --json | python3 -c "
import json, subprocess, sys
for c in json.load(sys.stdin):
    cid = c.get('Container ID')
    if cid:
        subprocess.run(['$modal', 'container', 'stop', cid], check=True)
        print('stopped', cid)
"
