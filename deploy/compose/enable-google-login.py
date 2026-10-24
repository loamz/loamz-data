"""Turn on "Sign in with Google" in a laptop cell's Keycloak, using the OAuth client in `.env` (GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET).

Create the OAuth client (type "Web application") in Google Cloud Console and add this redirect URI to it:

    <KEYCLOAK_PUBLIC_URL>/realms/loamz/broker/google/endpoint

Then:

    uv run --package loamz-server python deploy/compose/enable-google-login.py deploy/compose/.env

Safe to run again. The provider is shown to everyone on the sign-in page. A person who signs in with Google still needs to
belong to an organization (tenant) to use the apps: add them with add-demo-member.py after their first sign-in. For a laptop cell only.
"""

import sys
from pathlib import Path

import httpx

ENV = dict(
    line.split("=", 1)
    for line in Path(sys.argv[1]).read_text().splitlines()
    if "=" in line and not line.startswith("#")
)
if not ENV.get("GOOGLE_CLIENT_ID") or not ENV.get("GOOGLE_CLIENT_SECRET"):
    raise SystemExit("Set GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET in the .env first")
PUBLIC = ENV.get("KEYCLOAK_PUBLIC_URL", "http://keycloak.localhost:8081")
KC = f"http://localhost:{ENV.get('PORT_KEYCLOAK', '8081')}"
HOST = {"Host": PUBLIC.split("://", 1)[1]}
http = httpx.Client(timeout=30)

token = http.post(
    f"{KC}/realms/master/protocol/openid-connect/token",
    headers=HOST,
    data={
        "grant_type": "password",
        "client_id": "admin-cli",
        "username": ENV["KEYCLOAK_ADMIN_USER"],
        "password": ENV["KEYCLOAK_ADMIN_PASSWORD"],
    },
).json()["access_token"]
admin = {**HOST, "Authorization": f"Bearer {token}"}
api = f"{KC}/admin/realms/loamz"

provider = {
    "alias": "google",
    "displayName": "Google",
    "providerId": "google",
    "enabled": True,
    "trustEmail": True,
    "firstBrokerLoginFlowAlias": "first broker login",
    "config": {
        "clientId": ENV["GOOGLE_CLIENT_ID"],
        "clientSecret": ENV["GOOGLE_CLIENT_SECRET"],
        "defaultScope": "openid email profile",
        "syncMode": "IMPORT",
    },
}
exists = http.get(f"{api}/identity-provider/instances/google", headers=admin).status_code == 200
r = (
    http.put(f"{api}/identity-provider/instances/google", headers=admin, json=provider)
    if exists
    else http.post(f"{api}/identity-provider/instances", headers=admin, json=provider)
)
r.raise_for_status()
print("Google provider", "updated" if exists else "created")

print(f"Add this redirect URI to the Google OAuth client:\n  {PUBLIC}/realms/loamz/broker/google/endpoint")
