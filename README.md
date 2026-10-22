# Loamz

A governed data platform and durable agents built on it.

| Directory | Distribution | What it is |
| --- | --- | --- |
| [`server/engine`](server/engine) | `loamz-engine` | Embeddable engines: SQL `Database` (SQLite and Postgres), vectors, memory, files, documents, and the durable agent runtime (`loamz.engine.agents`) |
| [`server/server`](server/server) | `loamz-server` | The open platform: Loamz Data (`loamz.server.data`), agent hosting (`loamz.server.agents`) and token verification (`loamz.server.identity`); one CLI, `loamz-server data|agents` |
| [`server/reader`](server/reader) | `loamz-reader` | High-throughput document reader service (PaddleOCR and layout analysis), Python 3.13 |
| [`apps/data-workspace`](apps/data-workspace) | | Next.js data workspace |
| [`apps/agents-console`](apps/agents-console) | | Next.js agent chat and operations console |
| [`sdk/js/oidc-bff`](sdk/js/oidc-bff) | `@loamz/oidc-bff` | Browser BFF session library (Keycloak authorization code with PKCE) |
| [`specs`](specs) | | OpenAPI specs (`agents/v1`, `data/v1`) that every SDK is generated or checked against |
| [`deploy`](deploy) | | Docker, Compose cell, Helm chart `loamz-cell`, Terraform, Keycloak realm |

The Python client libraries (`loamz-core`, `loamz-identity`, `loamz-data`, `loamz`) live in
[loamz/loamz-python](https://github.com/loamz/loamz-python). Until they are on PyPI this repo builds against that
repository's `main` branch (see `[tool.uv.sources]` in `pyproject.toml`).

## Quick Start

```bash
make sync     # uv (Python 3.14) and pnpm workspaces
make check    # lint, import contracts, fast unit test suite
```
