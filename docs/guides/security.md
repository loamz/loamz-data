# Security model

What the platform enforces, where, and how to check it.

## Identity and access
- **Authentication.** OIDC. Both servers verify RS256 JWTs against the issuer's JWKS (`loamz.server.identity`):
  signature, issuer, audience, expiry. Keycloak is the reference identity provider (realm `loamz`, shipped in
  `deploy/keycloak`); any OIDC provider that issues the same claims works. Enterprise SSO (SAML, LDAP, social) is
  brokered by the identity provider, not by Loamz.
- **Tenants.** The tenant (`organization` claim) comes from the token, never from a request body. One naming module,
  `loamz.core.tenancy`, derives every tenant-scoped name (catalog, volumes, groups, file keys), and refuses keys that
  climb out of a tenant.
- **Authorization.** Catalog securables have owners and grants with inherited privileges; `USE` is required on every
  ancestor. Tables can carry row filters and column masks. Views run with their owner's rights.
- **Agents and delegation.** Agents act with short-lived delegated tokens limited to declared scopes. The agent host
  holds no storage credentials: bytes move on URLs the data service signs, and the data service signs per-request
  tokens for the agent host's file calls (Ed25519, one `jti` each, verified against a published JWKS).
- **Disclosure.** Another tenant's or user's private resource is `404`, never `403`.

## Data protection
- **In transit.** TLS to the edge. Between the data controller and workers the cell uses mutual TLS with a private CA
  (Helm generates it; `LOAMZ_DATA_TRUSTED_*` settings pin peers). Plain HTTP between internal hosts is an explicit
  opt-in.
- **At rest.** The Terraform modules create encrypted databases (`storage_encrypted`), encrypted buckets with
  versioning, and deletion protection (`db_deletion_protection`). Keys are the cloud provider's; bring-your-own-key is
  configured in those modules' variables, not in the application.
- **Secrets.** Never in images or the repository. Helm reads them from one Kubernetes secret (or `existingSecret`);
  Compose generates them with `gen-env.sh`.
- **Privacy.** Erasure requests and exports per person or per tenant (`/api/v1/erasure-requests`, exports), and
  retention maintenance on tables.

## Audit
- Every governed change writes an audit event: who (principal and acting identity), what, which securable, outcome.
- Events are **hash-chained**. `GET /api/v1/audit-events:verify` detects an event altered or removed from the middle
  of the log.
- `GET /api/v1/audit-events:export?format=ndjson|cef` is resumable (`after` and the `X-Next-After` header) for log
  collectors and SIEMs. Only admins can read the log.

## Runtime hardening
- Containers run as a non-root user with a read-only root filesystem and the service-account token not mounted.
- The Helm chart ships NetworkPolicies, resource requests and limits, and probes.
- Code interpreters run in nsjail sandboxes without network, reaching data only through a Unix socket with a per-run
  token.

## Supply chain
- Container images are built from tags, pushed to GHCR with an SBOM, provenance and a keyless signature
  (`release-images.yml`). Verify with `cosign verify` against this repository's workflow identity.
- Python packages publish through PyPI trusted publishing (no long-lived tokens).
- CI runs CodeQL, dependency review (blocking high severity and copyleft network licenses), a weekly vulnerability audit
  of both locks (`pip-audit`) and OpenSSF Scorecard. Dependencies are updated monthly by Dependabot.

## Reporting
See [SECURITY.md](../../SECURITY.md).
