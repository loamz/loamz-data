<p align="center">
  <img src="web/public/favicon.svg" alt="Loamz" width="72">
</p>

<h1 align="center">Loamz Data</h1>

<p align="center">
  <b>A fast, governed data platform for analysts, data engineers and AI agents.</b><br>
  Open tables, SQL and sandboxed compute, with access control and audit built in.
</p>

<p align="center">
  <a href="docs/ARCHITECTURE.md">Architecture</a> ·
  <a href="docs/life-of-a-query.md">Life of a query</a> ·
  <a href="docs/CONTRIBUTING.md">Contributing</a> ·
  <a href="docs/rfcs/">RFCs</a> ·
  <a href="specs/data/v1/openapi.yaml">API</a>
</p>

---

## Why

Teams choose between **fast** (a local engine, no governance) and **governed** (a heavy platform, slow and costly).
AI agents make it harder: they need data, and must never see more than the person they act for. Loamz Data is fast and
governed, for traditional data work and for the AI era at the same time.

## What it does

- **Governed catalog.** Organizations, workspaces, catalogs, schemas, tables and volumes, with Unity Catalog privileges.
  What you may not see does not exist for you (404, never 403). Every action is audited in a hash chain.
- **Open tables.** Tables are Apache Iceberg on Parquet, in your object store. Loamz Data is their Iceberg REST catalog,
  so DuckDB and other Iceberg clients read and write them directly, with credentials scoped to one table for minutes.
- **SQL and jobs on any compute.** Serverless (Modal) or your own machines, which connect out and run each statement in a
  gVisor sandbox. No machine ever holds the storage key.
- **Agents on your data.** An agent acting for a person gets only what both may do, and the audit names both.
- **A workbench.** Catalog explorer, SQL editor, query history, compute and settings, in the browser.

## Try it

```sh
docker compose -f deploy/dev/compose.yml up -d        # Keycloak and Postgres
deploy/dev/seed.py                                    # people; passwords go to .dev/accounts.json
cargo build --release -p loamz-data-server
scripts/dev.sh                                        # server on :8020, workbench on http://localhost:5180
```

Sign in as `ana`. Then query a table from any DuckDB, through the governed catalog:

```sql
INSTALL iceberg; LOAD iceberg;
ATTACH 'nyc' AS lz (TYPE iceberg, ENDPOINT 'http://127.0.0.1:8020/api/v1/workspaces/<workspace id>/iceberg', TOKEN '<token>');
SELECT count(*) FROM lz.main.yellow_trips;
```

`deploy/dev/token.py ana` prints a token. Data is stored in Cloudflare R2; see [CONTRIBUTING](docs/CONTRIBUTING.md) for the settings.

## Status

Under active development, **not production ready**. Working today: sign-in, organizations and workspaces, the catalog and
grants, Iceberg tables, SQL on serverless and on sandboxed machines, jobs, audit, the workbench. Being built now: the fast
data plane ([RFC 001](docs/rfcs/001-fast-data-plane.md)). Not deployed publicly yet.

## License

Apache-2.0.
