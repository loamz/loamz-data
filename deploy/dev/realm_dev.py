"""What the local Keycloak realm has on top of the one the live server uses (development only): the first admin, the roles that let
the server create organizations, and single sign-on for the tools around Loamz Data (OpenBao and the RustFS console).

Every tool is one Keycloak client. A person gets in when they hold that client's role `admin`; the first admin holds all of them.
`add_dev(realm)` is safe to run again."""

# Secrets of the throwaway development clients.
OPENBAO_SECRET = "openbao-dev-only"
RUSTFS_SECRET = "rustfs-dev-only"


def _once(items, key, value):
    return any(i.get(key) == value for i in items)


def _sso_client(client_id, name, secret, redirects, claim, claim_name):
    return {
        "clientId": client_id, "name": name, "enabled": True, "publicClient": False, "secret": secret,
        "standardFlowEnabled": True, "directAccessGrantsEnabled": False,
        "redirectUris": redirects, "webOrigins": ["+"],
        "defaultClientScopes": ["basic", "profile", "email", "roles"], "optionalClientScopes": [],
        "protocolMappers": [{
            "name": f"{claim_name} from the client's roles", "protocol": "openid-connect",
            "protocolMapper": "oidc-usermodel-client-role-mapper", "consentRequired": False,
            "config": {"usermodel.clientRoleMapping.clientId": client_id, "claim.name": claim, "jsonType.label": "String",
                       "multivalued": "true", "id.token.claim": "true", "access.token.claim": "true", "userinfo.token.claim": "true"},
        }],
    }


def add_dev(realm):
    roles = realm["roles"].setdefault("client", {})
    if not _once(roles.setdefault("loamz-data", []), "name", "platform-admin"):
        roles["loamz-data"].append({"name": "platform-admin", "description": "The installation's admin: creates organizations and adds people to them."})
    roles.setdefault("loamz-openbao", [{"name": "admin", "description": "May sign in to OpenBao with full access (development)."}])
    roles.setdefault("loamz-rustfs", [{"name": "consoleAdmin", "description": "May sign in to the RustFS console with full access (development)."}])

    if not _once(realm["clients"], "clientId", "loamz-openbao"):
        realm["clients"].append(_sso_client(
            "loamz-openbao", "OpenBao (development)", OPENBAO_SECRET,
            ["http://127.0.0.1:8200/ui/vault/auth/oidc/oidc/callback", "http://localhost:8200/ui/vault/auth/oidc/oidc/callback", "http://localhost:8250/oidc/callback"],
            "roles", "roles"))
    if not _once(realm["clients"], "clientId", "loamz-rustfs"):
        # RustFS takes the policy a person gets from a claim; `consoleAdmin` is its built-in full-access policy.
        realm["clients"].append(_sso_client(
            "loamz-rustfs", "RustFS console (development)", RUSTFS_SECRET,
            ["http://127.0.0.1:9001/*", "http://localhost:9001/*", "http://127.0.0.1:9000/*"],
            "policy", "policy"))

    users = realm["users"]
    admin = next((u for u in users if u.get("username") == "admin"), None)
    if admin is None:
        admin = {"username": "admin", "email": "admin@loamz.local", "firstName": "Loamz", "lastName": "Admin", "enabled": True, "emailVerified": True,
                 "credentials": [{"type": "password", "value": "admin", "temporary": True}], "clientRoles": {}}
        users.append(admin)
    admin.setdefault("clientRoles", {}).update({"loamz-data": ["platform-admin"], "loamz-openbao": ["admin"], "loamz-rustfs": ["consoleAdmin"]})
    for user in users:
        if user.get("serviceAccountClientId") == "loamz-data-directory":
            have = user["clientRoles"].setdefault("realm-management", [])
            have += [r for r in ("manage-users", "manage-realm") if r not in have]
    for client in realm["clients"]:
        if client["clientId"] == "loamz-data-directory":
            client["description"] = "Lets Data list people and groups and create organizations and people for its admin pages. A service account."
    return realm
