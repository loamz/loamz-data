"""End-to-end check of a local compose cell: Keycloak org + user, tenant provisioning, data API.

Run after `docker compose up -d`:  uv run --package loamz-core python deploy/compose/e2e.py deploy/compose/.env
"""

import sys
import time
from pathlib import Path

import httpx

from loamz.core.tenancy import tenant_alias

ENV = dict(
    line.split("=", 1)
    for line in Path(sys.argv[1]).read_text().splitlines()
    if "=" in line and not line.startswith("#")
)
KC = f"http://localhost:{ENV.get('PORT_KEYCLOAK', '8081')}"
REALM = f"{KC}/realms/loamz"
DATA = f"http://localhost:{ENV.get('PORT_DATA_API', '8020')}/api/v1"
# Keycloak only answers for its configured hostname; browsers and containers know it as keycloak.localhost:8081.
KC_HOST = {
    "Host": ENV.get("KEYCLOAK_PUBLIC_URL", "http://keycloak.localhost:8081").split("://", 1)[1]
}
http = httpx.Client(timeout=60)


def step(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  {detail}" if detail else ""))
    if not ok:
        sys.exit(1)


def admin_token():
    r = http.post(
        f"{KC}/realms/master/protocol/openid-connect/token",
        headers=KC_HOST,
        data={
            "grant_type": "password",
            "client_id": "admin-cli",
            "username": ENV["KEYCLOAK_ADMIN_USER"],
            "password": ENV["KEYCLOAK_ADMIN_PASSWORD"],
        },
    )
    r.raise_for_status()
    return {"Authorization": f"Bearer {r.json()['access_token']}", **KC_HOST}


admin = admin_token()
api = f"{KC}/admin/realms/loamz"

# 1. organization + user + membership
alias = f"e2e{int(time.time())}"
r = http.post(
    f"{api}/organizations",
    headers=admin,
    json={"name": alias, "alias": alias, "domains": [{"name": f"{alias}.example.com"}]},
)
step("create organization", r.status_code == 201, str(r.status_code) + r.text[:200])
org_id = r.headers["Location"].rsplit("/", 1)[1]
r = http.post(
    f"{api}/users",
    headers=admin,
    json={
        "username": f"alice-{alias}",
        "email": f"alice@{alias}.example.com",
        "emailVerified": True,
        "enabled": True,
        "firstName": "Alice",
        "lastName": "E2E",
        "credentials": [{"type": "password", "value": "e2e-Passw0rd!", "temporary": False}],
    },
)
step("create user", r.status_code == 201, str(r.status_code) + r.text[:200])
user_id = r.headers["Location"].rsplit("/", 1)[1]
r = http.post(
    f"{api}/organizations/{org_id}/members",
    headers=admin,
    content=f'"{user_id}"',
)
r = (
    r
    if r.status_code < 400
    else http.post(
        f"{api}/organizations/{org_id}/members",
        headers={**admin, "Content-Type": "application/json"},
        content=f'"{user_id}"',
    )
)
step("add member", r.status_code in (201, 204), str(r.status_code) + r.text[:200])

# test-only: let the CLI client use the password grant so a script can sign in (the realm file is unchanged)
clients = http.get(f"{api}/clients", headers=admin, params={"clientId": "loamz-cli"}).json()
cli = clients[0]
http.put(
    f"{api}/clients/{cli['id']}",
    headers=admin,
    json={**cli, "directAccessGrantsEnabled": True},
).raise_for_status()


def user_token(scope):
    r = http.post(
        f"{REALM}/protocol/openid-connect/token",
        headers=KC_HOST,
        data={
            "grant_type": "password",
            "client_id": "loamz-cli",
            "username": f"alice-{alias}",
            "password": "e2e-Passw0rd!",
            "scope": scope,
        },
    )
    step(
        f"sign in ({scope})",
        r.status_code == 200,
        r.text[:300] if r.status_code != 200 else "",
    )
    return {"Authorization": f"Bearer {r.json()['access_token']}"}


# 2. provision the tenant in loamz-data (as the platform would, with cell-admin)
r = http.post(
    f"{REALM}/protocol/openid-connect/token",
    headers=KC_HOST,
    data={
        "grant_type": "client_credentials",
        "client_id": "cell-admin",
        "client_secret": ENV["CELL_ADMIN_CLIENT_SECRET"],
    },
)
step("cell-admin token", r.status_code == 200, r.text[:200])
cell = {"Authorization": f"Bearer {r.json()['access_token']}"}

r = http.post(
    f"{DATA}/tenants",
    headers=cell,
    json={"tenant_id": org_id, "alias": tenant_alias(org_id)},
)
step("provision tenant", r.status_code in (200, 201), str(r.status_code) + r.text[:300])

# the tenant's administrator group (the platform creates it per cell); Alice administers her tenant
r = http.post(f"{api}/groups", headers=admin, json={"name": "tenant-admins"})
if r.status_code == 409:
    found = http.get(
        f"{api}/groups", headers=admin, params={"search": "tenant-admins", "exact": "true"}
    ).json()
    group_id = found[0]["id"]
else:
    group_id = r.headers["Location"].rsplit("/", 1)[1]
r = http.put(f"{api}/users/{user_id}/groups/{group_id}", headers=admin)
step("make Alice the tenant administrator", r.status_code in (200, 204), str(r.status_code))

# 3. Data API as the tenant administrator.
user = user_token(f"openid organization:{alias}")
r = http.get(f"{DATA}/search", headers=user, params={"q": "tenant_"})
step("data: search the catalog", r.status_code == 200, str(r.status_code) + r.text[:300])

# Data is never available without a signed-in identity.
r = http.get(f"{DATA}/catalogs")
step("isolation: an anonymous request is denied", r.status_code in (401, 403), str(r.status_code))

# 5b. SCIM: the tenant administrator provisions people and groups with a personal access token
r = http.post(f"{DATA}/access-tokens", headers=user, json={"name": "e2e-scim"})
step(
    "create a personal access token",
    r.status_code == 201 and r.json()["token"].startswith("lpat_"),
    str(r.status_code),
)
pat = {"Authorization": "Bearer " + r.json()["token"]}
SCIM = DATA.removesuffix("/api/v1") + "/scim/v2"
r = http.get(f"{DATA}/catalogs", headers=pat)
step("the token acts as its owner", r.status_code == 200, str(r.status_code))
r = http.post(
    f"{SCIM}/Users",
    headers=pat,
    json={
        "userName": f"scim-{alias}",
        "name": {"givenName": "Sam", "familyName": "Scim"},
        "emails": [{"value": f"sam@{alias}.example.com", "primary": True}],
    },
)
step("SCIM: create a user", r.status_code == 201, str(r.status_code) + r.text[:300])
scim_user = r.json()["id"]
r = http.get(f"{SCIM}/Users", headers=pat, params={"filter": f'userName eq "scim-{alias}"'})
step("SCIM: find the user by filter", r.json().get("totalResults") == 1, r.text[:200])
r = http.get(f"{SCIM}/Users", headers=pat)
step("SCIM: list shows the tenant's people", r.json()["totalResults"] >= 2, r.text[:200])
r = http.patch(
    f"{SCIM}/Users/{scim_user}",
    headers=pat,
    json={"Operations": [{"op": "replace", "value": {"active": False}}]},
)
step(
    "SCIM: deactivate the user", r.status_code == 200 and r.json()["active"] is False, r.text[:200]
)
r = http.post(
    f"{SCIM}/Groups",
    headers=pat,
    json={"displayName": "analysts", "members": [{"value": scim_user}]},
)
step(
    "SCIM: create a group with a member",
    r.status_code == 201 and len(r.json()["members"]) == 1,
    r.text[:300],
)
scim_group = r.json()["id"]
r = http.get(f"{SCIM}/Groups", headers=pat, params={"filter": 'displayName eq "analysts"'})
step("SCIM: find the group", r.json().get("totalResults") == 1, r.text[:200])
r = http.delete(f"{SCIM}/Groups/{scim_group}", headers=pat)
step("SCIM: delete the group", r.status_code == 204, str(r.status_code))
r = http.delete(f"{SCIM}/Users/{scim_user}", headers=pat)
step("SCIM: remove the user from the tenant", r.status_code == 204, str(r.status_code))
r = http.get(f"{SCIM}/Users/{scim_user}", headers=pat)
step("SCIM: the removed user is gone", r.status_code == 404, str(r.status_code))
tokens = http.get(f"{DATA}/access-tokens", headers=user).json()["items"]
r = http.delete(f"{DATA}/access-tokens/{tokens[0]['id']}", headers=user)
step("revoke the token", r.status_code == 204, str(r.status_code))
r = http.get(f"{DATA}/catalogs", headers=pat)
step("a revoked token is refused", r.status_code == 401, str(r.status_code))

print("ALL PASSED")
