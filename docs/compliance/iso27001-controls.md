# ISO/IEC 27001:2022 Annex A, by theme

**Product** = software control; **Repo** = repository process; **Org** = the operating organization.

| Theme | Relevant controls | Owner | Where |
| --- | --- | --- | --- |
| A.5 Organizational | Policies, roles, supplier relationships, incident management, legal requirements, privacy | Org; Repo for incident and vulnerability templates | `incident-response.md`, `vulnerability-management.md` |
| A.6 People | Screening, awareness, terms of employment | Org | |
| A.7 Physical | Premises, equipment | Org; cloud provider for data centres | |
| A.8.2 to A.8.5 Access | Privileged access, restriction, secure authentication | Product | OIDC, grants, tenant admin separation, tokens that are never administrators |
| A.8.7 Malware | Sandboxed execution, non-root runtime | Product | |
| A.8.8 Technical vulnerabilities | Weekly audit, CodeQL, dependency updates | Repo | `security.yml`, `.github/dependabot.yml` |
| A.8.9 Configuration | Chart and Terraform as code, reviewed in pull requests | Repo, Product | `deploy/` |
| A.8.10 Information deletion | Erasure, retention, legal hold | Product | |
| A.8.11 Data masking | Column masks and row filters | Product | |
| A.8.13 Backup | Managed backups, versioning, restore drill | Product, Repo | `disaster-recovery.md` |
| A.8.14 Redundancy | Replicas, standby controller, multi-AZ | Product | `operations.md` |
| A.8.15 Logging | Hash-chained audit log, structured logs, traces | Product | |
| A.8.16 Monitoring | Metrics, alerts, probes | Product | |
| A.8.20 to A.8.22 Network | Network policies, TLS, mutual TLS, segregation of tenants | Product | |
| A.8.24 Cryptography | TLS, customer-managed keys, hashed tokens at rest | Product | `encryption.md` |
| A.8.25 to A.8.29 Secure development | Code review, static analysis, tests in CI, contract tests, security testing plan | Repo | `penetration-testing.md` |
| A.8.31 Separation of environments | Cells are separate installations; CI uses throwaway services | Product, Repo | |
| A.8.32 Change management | Pull requests, required checks, signed releases | Repo | |

The management-system requirements (clauses 4 to 10: scope, leadership, risk treatment, internal audit, management
review) and the Statement of Applicability belong to the organization seeking certification.
