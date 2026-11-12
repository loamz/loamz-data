# Loamz Data

Loamz Data is the governed data service for Loamz. The current server and runner are Rust. Data is independent of Chat and Agents.

The implementation includes organization and workspace access, a catalog, managed Iceberg tables, object storage, audit, SQL queries, and sandboxed Python jobs. It is under development and is **not yet production ready**. Rules: [constitution](docs/constitution.md); current work: [docs/specs](docs/specs/).

## Components

- `server/server`: governed HTTP API and Iceberg REST catalog; its contract is `specs/data/v1/openapi.yaml`.
- `server/lake`: metadata database and file/object storage.
- `server/runner`: outbound compute agent and gVisor sandbox runner.
- `web`: React workbench.
- `deploy/dev`: development PostgreSQL and Keycloak.

## Local checks

Install the Rust toolchain in `rust-toolchain.toml`, Node 22, pnpm, and `uv`, then run:

```sh
cd web && pnpm install --frozen-lockfile && cd ..
make check
```

`make check` covers formatting, Clippy, SQLite tests, and frontend types/tests. PostgreSQL integration, a real sandbox, R2 and browser tests require separate services and configuration; they do not run as part of the default check.

For local identity and PostgreSQL, start `docker compose -f deploy/dev/compose.yml up -d` after preparing the development realm as described in `deploy/dev`. The web development server and Data API share an origin through Vite's proxy. Deployment to a public service requires additional infrastructure and validation.
