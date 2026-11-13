#!/usr/bin/env bash
# Re-makes deploy/dev/realm-loamz-dev.json (the throwaway local realm) from the same template the live server uses, so local
# sign-in behaves like the real one. Needs python3 with jinja2 and the loamz-deploy repository next to this one.
set -euo pipefail
cd "$(dirname "$0")/../.."
python3 - <<'PY'
import json, jinja2, os
t = jinja2.Template(open(os.environ.get("DEPLOY_REPO", "../loamz-deploy") + "/roles/keycloak/templates/realm-loamz.json.j2").read())
out = t.render(dev=True, scheme="http", domain="localhost:5173", data_domain="localhost:5180", smtp_host="")
realm = json.loads(out)  # fail loudly if the template made invalid JSON

# Development only: the first admin, the directory roles, and single sign-on for OpenBao and RustFS (deploy/dev/realm_dev.py).
import sys
sys.path.insert(0, "deploy/dev")
import realm_dev
realm_dev.add_dev(realm)

open("deploy/dev/realm-loamz-dev.json", "w").write(json.dumps(realm, indent=2) + "\n")
print("wrote deploy/dev/realm-loamz-dev.json")
PY
