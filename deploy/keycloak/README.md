# Keycloak realm for a Loamz cell

`realm-loamz.json` is an importable realm template for [ADR 0007](../../docs/adr/0007-keycloak-identity-and-agent-delegation.md):
one realm `loamz` per cell, tenants are Keycloak **Organizations**, one organization is active per token.

Validated against `quay.io/keycloak/keycloak:26.8.1` (`start-dev --import-realm`, ready in about 20 s): the import
is clean (no warnings), and tokens for a user in two organizations, a cell-admin service account and a device-grant
request were checked against `loamz-identity` using OIDC discovery.

## Import

```sh
export AGENTS_CONSOLE_CLIENT_SECRET=...          # all three are required, see "Secrets"
export DATA_WORKSPACE_CLIENT_SECRET=...
export CELL_ADMIN_CLIENT_SECRET=...
docker run --rm -p 127.0.0.1:8080:8080 \
  -e KC_BOOTSTRAP_ADMIN_USERNAME -e KC_BOOTSTRAP_ADMIN_PASSWORD \
  -e AGENTS_CONSOLE_CLIENT_SECRET -e DATA_WORKSPACE_CLIENT_SECRET -e CELL_ADMIN_CLIENT_SECRET \
  -v "$PWD/realm-loamz.json:/opt/keycloak/data/import/realm-loamz.json:ro" \
  quay.io/keycloak/keycloak:26.8.1 start-dev --import-realm
```

Production uses `start` with TLS/hostname settings and the same file. `--import-realm` skips a realm that already
exists: it seeds a new cell, it does not reconcile. For a running cell use the Terraform in [`terraform/`](terraform/).

### Secrets

No secret is in the file. Keycloak substitutes `${NAME}` from the process environment while importing:

| Variable | Used by | Default |
| --- | --- | --- |
| `AGENTS_CONSOLE_CLIENT_SECRET` | `agents-console` client secret (the BFF's `clientSecret`) | none |
| `DATA_WORKSPACE_CLIENT_SECRET` | `data-workspace` client secret (`WEB_CLIENT_SECRET`) | none |
| `CELL_ADMIN_CLIENT_SECRET` | `cell-admin` service account | none |
| `AGENTS_CONSOLE_URL` | UI origin for redirect/logout URIs | `http://127.0.0.1:3000` |
| `DATA_WORKSPACE_URL` | data workspace origin | `http://127.0.0.1:3011` |

**An unset secret variable is not an error**: Keycloak keeps the literal text `${CELL_ADMIN_CLIENT_SECRET}` and that
becomes the secret (verified). Check that all three are set before importing, and rotate secrets after import
through the admin API or Terraform if they were ever wrong.

## What the realm contains

| Clients | Type | Purpose |
| --- | --- | --- |
| `agents-console` | confidential, code + PKCE S256 | the agents console's backend-for-frontend (`@loamz/oidc-bff`) |
| `data-workspace` | confidential, code + PKCE S256 | the data workspace's backend-for-frontend |
| `agent-loamz`, `loamz-data` | bearer-only | the resource servers: their client ids are the token **audiences**, and they own the client roles `provision-tenant` and `erase-tenant` |
| `cell-admin` | confidential, client credentials | the platform's service account for provisioning and erasure |
| `loamz-cli` | public, device grant | people at a terminal; the SDK/CLI sign-in |

- **Realm roles:** `platform_admin` (operates the cell; never given to tenant users), `tenant_admin` (administers one organization),
  `end_user`. They arrive in `realm_access.roles`; client roles in `resource_access.<client>.roles`.
- **Client scopes:** `agents-api` adds `aud=agent-loamz`, `data-api` adds `aud=loamz-data`. The delegated scopes
  `data:read files:read files:write documents:read documents:write memory:read memory:write` are optional scopes on the
  human clients (what a CLI or integration may ask for). The standard scopes (`basic` with the `sub` claim, `profile`, `email`, `roles`, `web-origins`,
  `acr`) are defined **explicitly**: a realm file that defines any client scope suppresses Keycloak's built-ins, and a missing `basic`
  scope means access tokens without `sub`.
- **`organization` scope:** its mapper emits the `organization` claim as `{"<alias>": {"id": "<org id>"}}` (`addOrganizationId`).
- `groups` scope: group names for loamz-data's metastore admin group (`data-admins`, created empty).

### One active organization per token

A user may belong to several organizations, but a token is for exactly one. Clients request `scope=openid organization:<alias>`;
the claim then holds that single organization. Observed with 26.8.1 for a member of `acme` and `beta`:

| Requested scope | `organization` claim |
| --- | --- |
| `organization:acme` | `{"acme": {"id": "..."}}` |
| `organization:*` | both organizations |
| `organization` | **absent** for a multi-organization user |
| (none) | absent |

`loamz-identity` takes the immutable organization **id** as `tenant_id` and rejects a token with zero (403
`organization-required`) or several (403 `ambiguous-organization`) organizations. The UIs therefore pass the chosen
organization to the BFF (`authParams` / `scope`), and a user with several organizations picks one before sign-in.
Platform service accounts (`service-account-*`, such as `cell-admin`) carry no organization: they act across tenants, name the tenant in the
request, and are authorized by their client roles.

### Accounts realm brokering (planned)

The commercial platform runs a second realm, `accounts`, where people hold their account. A managed cell's realm
`loamz` will list `accounts` as an OIDC identity provider (broker), so one sign-in covers the account and its
projects; organizations in the cell map to the account's workspaces. Enterprise customers add their own IdP as an
**organization identity provider** (Organizations feature), which also routes users to the right organization by
email domain. Neither is in this template: add the identity provider per cell after import (or in Terraform).

### How the platform uses `cell-admin`

The control plane obtains a client-credentials token (`cell-admin`, scopes `agents-api data-api`) and calls the cell's
provisioning and erasure endpoints. The token has `aud` of both services and the client roles `provision-tenant` /
`erase-tenant` on each; the services check the role in `resource_access.<their client id>.roles`. Creating a tenant means
creating a Keycloak organization (admin API) and then calling `provision-tenant`; erasure calls `erase-tenant` and then deletes
the organization. `cell-admin` has no realm roles and cannot sign anyone in. Keep its secret in the platform's secret store and rotate it
per cell.

## Development realm

`data/loamz-data/scripts/bootstrap.py` still generates its own realm for the local workspace
(`.workspace/infra/loamz-realm.json`: users `alice`/`bob`, clients `data-workspace`, `loamz-data-cli`,
`loamz-data-bench`) with random dev secrets, and is unchanged. It works with the new libraries as is: loamz-data runs
with `require_organization=False`, so tokens without an organization are accepted. To move the dev flow onto this template:
import `realm-loamz.json` instead, create an organization and a user, point `LOAMZ_DATA_OIDC_ISSUER` at the realm, set
`WEB_CLIENT_ID=data-workspace` and `WEB_CLIENT_SECRET=$DATA_WORKSPACE_CLIENT_SECRET`, and add users to the group `data-admins`
for administrator rights (the template carries no users and no passwords).

## Terraform

[`terraform/`](terraform/) is a skeleton with the same clients, scopes and roles for reconciling a live cell (provider
`keycloak/keycloak`). It has not been applied or validated with `terraform validate`.

## Sign-in page

The realm uses the `loamz` login theme (`themes/loamz/login`). It extends Keycloak's own `keycloak.v2` theme, so every flow
(passwords, resets, one-time codes, passkeys, social sign-in) keeps working; the theme only changes the layout, colours and
text. Light and dark follow the browser. `compose.yml` mounts it into Keycloak, and the Helm chart ships a copy
(`deploy/helm/loamz-cell/files/theme/loamz`; CI fails if the two differ).

To change it, set `KEYCLOAK_THEME_CACHE=false` and `KEYCLOAK_THEME_STATIC_MAX_AGE=-1` when starting the cell (so a reload shows
each edit), edit `themes/loamz/login`, and copy the folder into the chart.

## Sign in with Google

Create an OAuth client (type "Web application") in Google Cloud Console, add `<KEYCLOAK_PUBLIC_URL>/realms/loamz/broker/google/endpoint`
as an authorized redirect URI, put `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` in `deploy/compose/.env`, then run
`deploy/compose/enable-google-login.py` against a laptop cell. For a Helm cell, add the provider under
`keycloak.brokering.identityProviders`. A person who signs in with Google must still belong to an organization (tenant) to use
the apps.
