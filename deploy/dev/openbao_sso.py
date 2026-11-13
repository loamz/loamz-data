#!/usr/bin/env python3
"""Lets people sign in to the local OpenBao with Keycloak (development): the OIDC login method, one role, and a policy. Whoever holds
the Keycloak client role `admin` of `loamz-openbao` (the first admin does) gets full access. Safe to run again.

  deploy/dev/openbao_sso.py          (OpenBao at http://127.0.0.1:8200, root token loamz-dev-only; Keycloak at 127.0.0.1:8082 from inside too)
"""
import json, os, sys, urllib.error, urllib.request

BAO = os.environ.get("OPENBAO_ADDR", "http://127.0.0.1:8200")
TOKEN = os.environ.get("OPENBAO_TOKEN", "loamz-dev-only")


def call(method, path, body=None, ok=(200, 204)):
    req = urllib.request.Request(f"{BAO}/v1/{path}", data=json.dumps(body).encode() if body is not None else None, headers={"X-Vault-Token": TOKEN}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status
    except urllib.error.HTTPError as e:
        if e.code in ok:
            return e.code
        sys.exit(f"{method} {path} -> {e.code}: {e.read()[:200]!r}")


# 400 on the first call means it is already enabled.
call("POST", "sys/auth/oidc", {"type": "oidc", "listing_visibility": "unauth"}, ok=(200, 204, 400))  # 400: already enabled
call("POST", "sys/auth/oidc/tune", {"listing_visibility": "unauth"})
call("POST", "auth/oidc/config", {
    # OpenBao shares Keycloak's network (deploy/dev/compose.yml), so the address in its tokens works from inside too.
    "oidc_discovery_url": "http://127.0.0.1:8082/realms/loamz",
    "oidc_client_id": "loamz-openbao", "oidc_client_secret": "openbao-dev-only", "default_role": "default",
})
call("PUT", "sys/policies/acl/loamz-admin", {"policy": 'path "*" { capabilities = ["create", "read", "update", "patch", "delete", "list", "sudo"] }'})
call("POST", "auth/oidc/role/default", {
    "role_type": "oidc", "user_claim": "preferred_username", "oidc_scopes": ["profile", "email"],
    "bound_claims": {"roles": ["admin"]}, "token_policies": ["loamz-admin"], "token_ttl": "8h",
    "allowed_redirect_uris": ["http://127.0.0.1:8200/ui/vault/auth/oidc/oidc/callback", "http://localhost:8200/ui/vault/auth/oidc/oidc/callback", "http://localhost:8250/oidc/callback"],
})

# Break-glass: when Loamz Data is down, an owner holding the Keycloak role `break-glass` signs in (role `break-glass`, one hour) and can
# read the bucket's credential and nothing else. Every use is in the audit log (deploy/dev/openbao.hcl: the container's output).
call("PUT", "sys/policies/acl/break-glass", {"policy": 'path "secret/data/loamz-data/bucket" { capabilities = ["read"] }'})
call("POST", "auth/oidc/role/break-glass", {
    "role_type": "oidc", "user_claim": "preferred_username", "oidc_scopes": ["profile", "email"],
    "bound_claims": {"roles": ["break-glass"]}, "token_policies": ["break-glass"], "token_ttl": "1h", "token_max_ttl": "1h",
    "allowed_redirect_uris": ["http://127.0.0.1:8200/ui/vault/auth/oidc/oidc/callback", "http://localhost:8200/ui/vault/auth/oidc/oidc/callback", "http://localhost:8250/oidc/callback"],
})
print("OpenBao: sign in with Keycloak is on, and break-glass access is set up")
