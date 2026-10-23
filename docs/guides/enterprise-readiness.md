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

## What is left, and who does it
Everything above is built and tested in this repository. The remaining items are organizational: they need a company,
auditors and time rather than code, and the repository already holds the evidence and templates for them.

1. **Audits.** A SOC 2 Type II and an ISO 27001 assessment by an accredited firm
   ([control mappings](../compliance/soc2-controls.md), [ISO 27001 mapping](../compliance/iso27001-controls.md)).
2. **Third-party penetration test**, yearly ([scope and rules](../compliance/penetration-testing.md)).
3. **Public trust page** and a **status page** for an operator offering a managed service
   ([content](../compliance/trust-center.md), [support policy](../../SUPPORT.md)).
4. **Full-cell recovery exercise** twice a year, timed against the objectives in
   [disaster-recovery.md](disaster-recovery.md).
