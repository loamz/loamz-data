# Enterprise readiness

An honest status against what enterprise buyers ask for. "Built" means it exists and is tested in this repository;
nothing here is a certification.

| Area | Status | Notes |
| --- | --- | --- |
| Single sign-on | Built (via the identity provider) | OIDC; SAML, LDAP and social login through Keycloak brokering |
| Multi-tenancy | Built | Tenant from the token, one naming module, 404 for foreign resources, per-tenant catalogs and volumes |
| Fine-grained access control | Built | Owners, inherited grants, row filters, column masks, definer-rights views |
| Audit log | Built | Hash-chained, verifiable, NDJSON and CEF export for SIEMs |
| Encryption in transit and at rest | Built | TLS, mutual TLS inside the cell, provider-managed keys in Terraform |
| Data residency | Built by design | A cell runs in the customer's own cloud account and region |
| Privacy requests | Built | Erasure and export per person or tenant |
| High availability | Partly | Stateless tiers: replicas, disruption budgets, spreading. Controller: single active |
| Backup and restore | Documented | Managed snapshots and versioning; restore order in `operations.md`; no automated drill |
| Observability | Partly | Traces, logs, probes; no Prometheus `/metrics` endpoint yet |
| Supply chain | Built | Signed images with SBOM and provenance, trusted publishing, CodeQL, dependency review, vulnerability audit |
| API stability | Built | `/api/v1`, RFC 9457 errors, cursor pagination; SDKs follow one naming standard |

## Gaps, in the order to close them
1. **Prometheus metrics** on both servers (request rate, latency, in-flight, queue depth) and ready-made alerts.
2. **SCIM** user and group provisioning, so enterprise identity systems manage membership.
3. **Personal access tokens** and service-account keys with expiry, rotation and audit (service accounts exist).
4. **Legal hold** that suspends retention and erasure for named securables.
5. **Controller high availability** with PostgreSQL-backed controller state and standbys.
6. **Customer-managed keys** surfaced in the product and documented per cloud.
7. **Automated restore drills** and a published recovery-time objective.
8. **Compliance**: SOC 2 Type II and ISO 27001 programs; a third-party penetration test; a published trust page.
9. **Support**: a status page, severity definitions and response targets.
