# Compliance

> Historical control material, not evidence of current Rust implementation, deployment, or certification. See the [readiness baseline](../guides/enterprise-readiness.md).

These historical control mappings and templates can help plan an audit. Their claims about product controls and repository processes must be revalidated against the current Rust service and the actual operating organization. A certification attests to an organization's practices over time; this repository does not establish one.

| Document | Purpose |
| --- | --- |
| [soc2-controls.md](soc2-controls.md) | The SOC 2 Trust Services Criteria mapped to product controls, repository process, and what the organization owns |
| [iso27001-controls.md](iso27001-controls.md) | The same for ISO/IEC 27001:2022 Annex A themes |
| [incident-response.md](incident-response.md) | Severity levels, roles, steps and a post-incident review template |
| [vulnerability-management.md](vulnerability-management.md) | How vulnerabilities are found, ranked and fixed, with time limits |
| [penetration-testing.md](penetration-testing.md) | Scope and rules of engagement for a third-party test, and what we test ourselves |
| [trust-center.md](trust-center.md) | A checklist for drafting a future public trust page |

Operational and security guides live in [../guides](../guides): [security](../guides/security.md),
[operations](../guides/operations.md), [disaster recovery](../guides/disaster-recovery.md),
[encryption](../guides/encryption.md), [enterprise readiness](../guides/enterprise-readiness.md).

## Evidence to establish

Confirm branch protection and successful CI runs, security scanning, signed releases and provenance, restore exercises, and audit export before presenting them as operating controls. The current Data service has hash-chained audit records and a verification endpoint; SIEM export and production evidence are not established by that code alone.
