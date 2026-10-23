# Enterprise readiness

An honest status against what enterprise buyers ask for. "Built" means it exists and is tested in this repository;
nothing here is a certification.

| Area | Status | Notes |
| --- | --- | --- |
| Single sign-on | Built (via the identity provider) | OIDC; SAML, LDAP and social login through Keycloak brokering |
| User and group provisioning | Built | SCIM 2.0 at `/scim/v2` (users, groups, filters, patch), tenant-scoped; see `scim.md` |
| Programmatic credentials | Built | Personal access tokens: hashed at rest, expiring (max one year), revocable, rotatable, audited, never administrators |
| Legal hold | Built | Tenant, user or table holds block erasure (423), snapshot expiry and audit purging; placement and release are audited |
| Multi-tenancy | Built | Tenant from the token, one naming module, 404 for foreign resources, per-tenant catalogs and volumes |
| Fine-grained access control | Built | Owners, inherited grants, row filters, column masks, definer-rights views |
| Audit log | Built | Hash-chained, verifiable, NDJSON and CEF export for SIEMs |
| Encryption in transit and at rest | Built | TLS, mutual TLS inside the cell; provider-managed keys by default, customer-managed keys on AWS, Google Cloud and Azure (`encryption.md`) |
| Data residency | Built by design | A cell runs in the customer's own cloud account and region |
| Privacy requests | Built | Erasure and export per person or tenant |
| High availability | Built | Stateless tiers: replicas, disruption budgets, spreading. Data controller: PostgreSQL-backed election with automatic takeover (`data.controller.ha`); managed PostgreSQL multi-AZ |
| Backup and restore | Built | Managed snapshots, PITR and versioning; restore order and objectives in `disaster-recovery.md`; weekly automated restore drill verifies row counts and the audit chain |
| Observability | Built | Prometheus `/metrics` with alert rules, OpenTelemetry traces, structured logs, probes; queue-depth and storage metrics to follow |
| Supply chain | Built | Signed images with SBOM and provenance, trusted publishing, CodeQL, dependency review, vulnerability audit |
| API stability | Built | `/api/v1`, RFC 9457 errors, cursor pagination; SDKs follow one naming standard |

## Gaps, in the order to close them
1. **Compliance**: SOC 2 Type II and ISO 27001 programs; a third-party penetration test; a published trust page.
2. **Support**: a status page, severity definitions and response targets.
