# Compliance

What this repository gives an organization that wants to pass a SOC 2 or ISO 27001 audit, and what it cannot.

**A certification attests to an organization's practices over time; software cannot hold one.** The product implements
technical controls and this repository keeps evidence of them. The organization running it (a vendor operating a managed
service, or a customer operating a cell) supplies the rest: policies, people, risk assessments, vendor reviews, and the
auditor.

| Document | Purpose |
| --- | --- |
| [soc2-controls.md](soc2-controls.md) | The SOC 2 Trust Services Criteria mapped to product controls, repository process, and what the organization owns |
| [iso27001-controls.md](iso27001-controls.md) | The same for ISO/IEC 27001:2022 Annex A themes |
| [incident-response.md](incident-response.md) | Severity levels, roles, steps and a post-incident review template |
| [vulnerability-management.md](vulnerability-management.md) | How vulnerabilities are found, ranked and fixed, with time limits |
| [penetration-testing.md](penetration-testing.md) | Scope and rules of engagement for a third-party test, and what we test ourselves |
| [trust-center.md](trust-center.md) | The content of a public trust page, drawn only from what is true today |

Operational and security guides live in [../guides](../guides): [security](../guides/security.md),
[operations](../guides/operations.md), [disaster recovery](../guides/disaster-recovery.md),
[encryption](../guides/encryption.md), [enterprise readiness](../guides/enterprise-readiness.md).

## Evidence that is produced automatically
- Every change goes through a pull request with required checks (`ci-ok`): tests, lint, import contracts, deploy checks.
- CodeQL, dependency review, a weekly vulnerability audit of both lock files, and OpenSSF Scorecard (`security.yml`).
- Signed container images with SBOM and provenance; packages published by trusted publishing.
- A weekly restore drill with timings (`restore-drill.yml`).
- A hash-chained audit log with an integrity check endpoint, exportable to a SIEM.
