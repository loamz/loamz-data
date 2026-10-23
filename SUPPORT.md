# Support

## Open source
Use GitHub: **Discussions** for questions, **Issues** for reproducible bugs and feature requests. Maintainers answer on a
best-effort basis. Never post secrets, tokens or customer data; for a vulnerability follow [SECURITY.md](SECURITY.md).

## Commercial support
Where an organization offers support for Loamz, these are the severity definitions and response targets it should
adopt. They are targets for that organization's agreement, not promises made by this repository.

| Severity | Definition | First response | Updates |
| --- | --- | --- | --- |
| 1 Critical | Production down or data at risk; no workaround | 1 hour, 24x7 | Every 2 hours until mitigated |
| 2 High | Major function impaired; a workaround exists | 4 business hours | Daily |
| 3 Normal | Minor impairment or a question about use | 1 business day | Every 3 business days |
| 4 Low | Documentation, enhancement requests | 3 business days | On change |

A good request names the version (`loamz-server --version`, image tag), the topology (Compose or Helm, mode), what was
expected and what happened, the request id from the `X-Request-Id` response header, and relevant logs with secrets removed.
Severity 1 incidents follow [docs/compliance/incident-response.md](docs/compliance/incident-response.md).

## Status
A cell reports its own health at `/health`, `/readyz` and `/metrics`. An operator offering a managed service should
publish a status page and link it from its trust page.
