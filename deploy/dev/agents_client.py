#!/usr/bin/env python3
"""Makes the agents server's own client in the running local Keycloak, the way the realm template does, so token exchange can be
tried without re-importing the realm: client loamz-agents (confidential, standard token exchange on, audience loamz-data), and
the test client names loamz-agents in its tokens' audience. The client's secret goes to .dev/agents-client-secret (mode 600) and is
never printed. Running it again keeps what exists. Development only.

  deploy/dev/agents_client.py
"""
import json, os, secrets, sys, urllib.error, urllib.parse, urllib.request

BASE = os.environ.get("KEYCLOAK_URL", "http://127.0.0.1:8082")
REALM = "loamz"
OUT = os.path.join(os.path.dirname(__file__), "..", "..", ".dev", "agents-client-secret")


def call(method, path, token, data=None, form=None, ok=(200, 201, 204, 409)):
    headers, body = {"Authorization": "Bearer " + token} if token else {}, None
    if form is not None:
        body, headers["Content-Type"] = urllib.parse.urlencode(form).encode(), "application/x-www-form-urlencoded"
    elif data is not None:
        body, headers["Content-Type"] = json.dumps(data).encode(), "application/json"
    try:
        with urllib.request.urlopen(urllib.request.Request(BASE + path, data=body, headers=headers, method=method)) as r:
            raw = r.read()
            return json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        if e.code in ok:
            return None
        sys.exit(f"{method} {path} -> {e.code}: {e.read()[:300]!r}")


def audience(name):
    return {"name": f"{name} audience", "protocol": "openid-connect", "protocolMapper": "oidc-audience-mapper", "consentRequired": False,
            "config": {"included.client.audience": name, "id.token.claim": "false", "access.token.claim": "true", "introspection.token.claim": "true"}}


def main():
    admin = call("POST", "/realms/master/protocol/openid-connect/token", None,
                 form={"grant_type": "password", "client_id": "admin-cli", "username": "admin", "password": "admin-dev-only"})["access_token"]
    api = lambda m, p, d=None, **k: call(m, f"/admin/realms/{REALM}{p}", admin, data=d, **k)
    secret = open(OUT).read().strip() if os.path.exists(OUT) else secrets.token_urlsafe(32)
    api("POST", "/clients", {
        "clientId": "loamz-agents", "name": "Loamz Agents", "enabled": True, "publicClient": False, "secret": secret,
        "standardFlowEnabled": False, "directAccessGrantsEnabled": False, "serviceAccountsEnabled": True,
        "attributes": {"standard.token.exchange.enabled": "true"}, "protocolMappers": [audience("loamz-data")],
        "defaultClientScopes": ["basic", "roles", "organization"], "optionalClientScopes": [],
    })
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    fd = os.open(OUT, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    os.write(fd, secret.encode())
    os.close(fd)
    test = next((c for c in api("GET", "/clients?clientId=loamz-data-test") or []), None)
    if test:
        api("POST", f"/clients/{test['id']}/protocol-mappers/models", audience("loamz-agents"))
    print("the loamz-agents client is in place; its secret is in .dev/agents-client-secret")


main()
