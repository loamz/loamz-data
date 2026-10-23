# SOC 2 Trust Services Criteria

Legend: **Product**: implemented in the software. **Repo**: enforced by this repository's process. **Org**: the operating
organization must provide it (policy, people, evidence); nothing here substitutes for it.

## Security (Common Criteria)
| Criteria | Control | Owner | Where |
| --- | --- | --- | --- |
| CC1 Control environment | Code of conduct, contribution rules, code owners | Repo; Org for governance, roles, background checks | `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `.github/CODEOWNERS` |
| CC2 Communication | Security policy and reporting channel, `security.txt`, documented architecture and operations | Repo; Org for customer notices | `SECURITY.md`, `/.well-known/security.txt`, `docs/` |
| CC3 Risk assessment | Threat-relevant automated analysis on every change | Repo; Org for the risk register | CodeQL, dependency review (`security.yml`) |
| CC4 Monitoring | Weekly vulnerability audit and scorecard; request metrics and alerts; restore drill | Repo, Product | `security.yml`, `/metrics` with alert rules, `restore-drill.yml` |
| CC5 Control activities | Import contracts, typed APIs, versioned contract tests | Repo | `.importlinter`, `specs/*/v1`, contract drift tests |
| CC6.1 Logical access | OIDC authentication; owners, grants, row filters and column masks; tenant isolation; delegated tokens limited to scopes | Product | `security.md` |
| CC6.2 Provisioning | SCIM user and group provisioning; deactivation disables sign-in | Product; Org for joiner/mover/leaver process | `scim.md` |
| CC6.3 Authorization changes | Grants and holds are audited; access tokens expire, rotate and revoke | Product | audit log |
| CC6.6 Boundary protection | Network policies, TLS, mutual TLS inside the cell, no public databases | Product (chart, Terraform) | `deploy/` |
| CC6.7 Data in transit | TLS 1.2+ (1.3 default internally) | Product | `security.md` |
| CC6.8 Malicious software | Non-root containers, read-only root filesystem, sandboxed code execution | Product | chart, nsjail ADR |
| CC7.1 Detection | Weekly and per-change vulnerability scanning | Repo | `security.yml` |
| CC7.2 Monitoring of anomalies | Hash-chained audit, SIEM export, metrics, traces | Product; Org for the SIEM and on-call | `operations.md` |
| CC7.3 to CC7.5 Incidents | Response procedure, severities, post-incident review | Repo template; Org executes | `incident-response.md` |
| CC8.1 Change management | Pull requests, required checks, conventional commits, release automation, signed artifacts | Repo; Org for approvals and branch protection settings | `CONTRIBUTING.md`, `release.yml` |
| CC9 Risk mitigation, vendors | Dependency license and vulnerability gates; pinned lock files | Repo; Org for vendor reviews | `security.yml` |

## Availability
| Criteria | Control | Owner |
| --- | --- | --- |
| A1.1 Capacity | Quotas, rate limits, autoscaling, resource requests and limits | Product |
| A1.2 Recovery infrastructure | Replicas, disruption budgets, elected controller, multi-AZ database option, backups, versioning | Product (chart, Terraform) |
| A1.3 Recovery testing | Weekly restore drill; full-cell exercises twice a year | Repo (drill); Org (exercise) |

## Confidentiality
| Criteria | Control | Owner |
| --- | --- | --- |
| C1.1 Identify and protect | Per-tenant catalogs and volumes, encryption at rest with optional customer-managed keys | Product |
| C1.2 Disposal | Erasure requests per person or tenant, retention maintenance, legal hold override | Product |

## Processing integrity
| Criteria | Control | Owner |
| --- | --- | --- |
| PI1 | Typed, versioned APIs, RFC 9457 errors, idempotent writes, compare-and-swap on files, audit of changes | Product |

## Privacy
| Criteria | Control | Owner |
| --- | --- | --- |
| P1 to P8 | Export and erasure by subject and tenant; data stays in the customer's cloud region; audit of access | Product. Notices, consent, DPAs and sub-processor lists are the Org's |

## What an auditor will ask for that is not here
Written information-security and acceptable-use policies; risk register and annual review; employee onboarding,
training and offboarding records; vendor inventory and reviews; business-continuity plan; evidence that the controls
operated over the audit period (screenshots, tickets, access reviews); management's system description.
