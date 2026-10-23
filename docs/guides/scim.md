# SCIM provisioning

Loamz Data serves SCIM 2.0 at `/scim/v2`, so an enterprise identity provider (Okta, Microsoft Entra ID, OneLogin,
JumpCloud and others) can create, update and remove a tenant's people and groups automatically.

## Set up
1. A tenant administrator signs in and creates a personal access token (`POST /api/v1/access-tokens`, a name and an
   expiry up to a year). The token is shown once.
2. In the identity provider, add a SCIM application with base URL `https://<data host>/scim/v2`, authentication "HTTP
   bearer token", and that token. Rotate it with `POST /api/v1/access-tokens/{id}:rotate`; rotation revokes the old one.
3. The server needs the identity provider's admin API: `LOAMZ_DATA_DIRECTORY_URL`, `LOAMZ_DATA_DIRECTORY_REALM`,
   `LOAMZ_DATA_DIRECTORY_CLIENT_ID` (default `loamz-directory`, a service account that may manage users, groups and organizations of the realm and nothing else) and `LOAMZ_DATA_DIRECTORY_CLIENT_SECRET`. The Compose
   file and the Helm chart (`scim.enabled`) set these for the bundled Keycloak. Without a directory URL, SCIM answers
   `503`.

## What it does
| Resource | Operations |
| --- | --- |
| `Users` | list (with `startIndex`, `count`, filters `userName`, `emails.value`, `externalId` with `eq`), get, create, replace, patch, delete |
| `Groups` | list, get, create, replace, patch (add, remove or replace members, rename), delete |
| Discovery | `ServiceProviderConfig`, `ResourceTypes` |

- **Scope.** Only the calling administrator's tenant (Keycloak organization) is visible or changeable. Another tenant's
  user or group answers `404`.
- **Deactivate.** `active: false` disables the person's sign-in.
- **Remove.** `DELETE /Users/{id}` removes the person from this tenant and keeps their identity, because they may belong
  to other tenants.
- **Groups.** A tenant's groups are realm groups named `<tenant alias>:<display name>`. Display names cannot contain `:`.
- **Errors.** SCIM error documents (`urn:ietf:params:scim:api:messages:2.0:Error`) with `scimType` `uniqueness` for
  duplicates and `invalidValue` for bad input.
- **Not supported:** bulk operations, sorting, ETags, password changes, filters other than `eq`.
