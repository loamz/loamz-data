#!/usr/bin/env python3
"""Seeds the local Keycloak with an organization, people and groups, and writes how to sign in as them to .dev/accounts.json
(mode 600; passwords are made here and never printed). Running it again keeps what exists.

  deploy/dev/seed.py                      (Keycloak at http://127.0.0.1:8082, admin / admin-dev-only)

Made: organization "acme" (alias acme) with ana (holds the Keycloak role account-admin), bob and cyd; group /finance/analysts
with cyd in it; and one person with no organization, pia. Development only.
"""
import json, os, secrets, stat, sys, urllib.error, urllib.parse, urllib.request

BASE = os.environ.get("KEYCLOAK_URL", "http://127.0.0.1:8082")
REALM = "loamz"
OUT = os.path.join(os.path.dirname(__file__), "..", "..", ".dev", "accounts.json")


def call(method, path, data=None, form=None, token=None, ok=(200, 201, 204, 409)):
    headers, body = {}, None
    if token:
        headers["Authorization"] = "Bearer " + token
    if form is not None:
        body = urllib.parse.urlencode(form).encode()
        headers["Content-Type"] = "application/x-www-form-urlencoded"
    elif data is not None:
        body = json.dumps(data).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(BASE + path, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req) as r:
            raw = r.read()
            return r.status, (json.loads(raw) if raw and r.headers.get("Content-Type", "").startswith("application/json") else raw), r.headers
    except urllib.error.HTTPError as e:
        if e.code in ok:
            return e.code, None, e.headers
        sys.exit(f"{method} {path} -> {e.code}: {e.read()[:300]!r}")


def main():
    _, t, _ = call("POST", "/realms/master/protocol/openid-connect/token", form={"grant_type": "password", "client_id": "admin-cli", "username": "admin", "password": "admin-dev-only"})
    admin = t["access_token"]
    api = lambda m, p, d=None, **k: call(m, f"/admin/realms/{REALM}{p}", data=d, token=admin, **k)

    accounts = json.load(open(OUT)) if os.path.exists(OUT) else {}
    people = {"ana": True, "bob": False, "cyd": False, "pia": False}
    for name in people:
        if name not in accounts:
            accounts[name] = {"password": secrets.token_urlsafe(18)}
    uid = {}
    for name in people:
        api("POST", "/users", {"username": name, "email": f"{name}@acme.test", "firstName": name.capitalize(), "lastName": "Test", "enabled": True, "emailVerified": True,
                               "credentials": [{"type": "password", "value": accounts[name]["password"], "temporary": False}]})
        _, found, _ = api("GET", f"/users?username={name}&exact=true")
        uid[name] = found[0]["id"]
        accounts[name]["id"] = uid[name]
        # Always set it, so the password on file is the one that works (the user may have been made by an earlier run).
        api("PUT", f"/users/{uid[name]}/reset-password", {"type": "password", "value": accounts[name]["password"], "temporary": False})

    # The organization, and who belongs to it.
    api("POST", "/organizations", {"name": "acme", "alias": "acme", "domains": [{"name": "acme.test"}]})
    _, orgs, _ = api("GET", "/organizations?search=acme")
    org = next(o for o in orgs if o["alias"] == "acme")["id"]
    for name in ("ana", "bob", "cyd"):
        api("POST", f"/organizations/{org}/members", name and uid[name])
    accounts["organization"] = {"id": org, "alias": "acme"}

    # The Keycloak role that lets someone create an organization's Data account.
    _, clients, _ = api("GET", "/clients?clientId=loamz-data")
    cid = clients[0]["id"]
    _, role, _ = api("GET", f"/clients/{cid}/roles/account-admin")
    api("POST", f"/users/{uid['ana']}/role-mappings/clients/{cid}", [role])

    # A group and a subgroup, with cyd in it.
    api("POST", "/groups", {"name": "finance"})
    _, groups, _ = api("GET", "/groups?search=finance&exact=true")
    parent = groups[0]["id"]
    api("POST", f"/groups/{parent}/children", {"name": "analysts"})
    _, kids, _ = api("GET", f"/groups/{parent}/children")
    api("PUT", f"/users/{uid["cyd"]}/groups/{next(k for k in kids if k['name'] == 'analysts')['id']}", None)

    # The realm locks an account after a few wrong passwords; a development run should not stay locked out.
    api("DELETE", "/attack-detection/brute-force/users")

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w") as f:
        json.dump(accounts, f, indent=2)
    os.chmod(OUT, stat.S_IRUSR | stat.S_IWUSR)
    print(f"seeded {', '.join(people)} and the organization acme; details in .dev/accounts.json")


main()
