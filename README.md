<p align="center">
  <img src="web/public/favicon.svg" alt="Loamz" width="72">
</p>

<h1 align="center">Loamz Data</h1>

<p align="center">
  <b>Governed data platform for Data, AI/ML engineers.</b><br>
  Built on Open Data Format.
</p>

<p align="center">
  <a href="docs/ARCHITECTURE.md">Architecture</a> ·
  <a href="docs/life-of-a-query.md">Life of a query</a> ·
  <a href="docs/CONTRIBUTING.md">Contributing</a> ·
  <a href="docs/rfcs/">RFCs</a> ·
  <a href="specs/data/v1/openapi.yaml">API</a>
</p>

<p align="center">
  <img src="docs/assets/demo.gif" alt="Catalog and SQL editor: one week of NYC taxi trips queried" width="880">
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white">
  <img alt="Apache Iceberg" src="https://img.shields.io/badge/Apache%20Iceberg-1F6FEB">
  <img alt="Parquet" src="https://img.shields.io/badge/Parquet-50ABF1?logo=apacheparquet&logoColor=white">
  <img alt="DuckDB" src="https://img.shields.io/badge/DuckDB-FFF000?logo=duckdb&logoColor=black">
  <img alt="PostgreSQL" src="https://img.shields.io/badge/PostgreSQL-4169E1?logo=postgresql&logoColor=white">
  <img alt="Keycloak" src="https://img.shields.io/badge/Keycloak-4D4D4D?logo=keycloak&logoColor=white">
  <img alt="gVisor" src="https://img.shields.io/badge/gVisor-4285F4">
  <img alt="React" src="https://img.shields.io/badge/React-20232A?logo=react&logoColor=61DAFB">
</p>

## Features

- **Governed catalog**: catalogs, schemas, tables and volumes with Unity Catalog privileges and a tamper-evident audit.
- **Open tables**: Apache Iceberg on Parquet in your object store; any Iceberg client reads them through our catalog.
- **SQL and jobs on any compute**: serverless, or your own machines running each job in a gVisor sandbox.
- **Agents act as the person**: an agent gets only what both it and its person may do.
- **Workbench**: catalog, SQL editor, query history, compute and settings in the browser.

## Getting started

```sh
docker compose -f deploy/dev/compose.yml up -d    # Keycloak and Postgres
deploy/dev/seed.py                                 # people; passwords in .dev/accounts.json
cargo build --release -p loamz-data-server
scripts/dev.sh                                     # open http://localhost:5180 and sign in as ana
```

Storage settings (Cloudflare R2) are in [CONTRIBUTING](docs/CONTRIBUTING.md).

## Use it from DuckDB

```sql
INSTALL iceberg; LOAD iceberg;
ATTACH 'nyc' AS lz (TYPE iceberg, ENDPOINT 'http://127.0.0.1:8020/api/v1/workspaces/<id>/iceberg', TOKEN '<token>');
SELECT count(*) FROM lz.main.yellow_trips;
```

`deploy/dev/token.py ana` prints a token. You see only what you may see.

## How it works

```mermaid
flowchart LR
  U[Workbench · DuckDB · agents] --> G[Governance: identity, catalog, grants, audit]
  G -->|scoped, short-lived credentials| S[(Storage: Postgres + object store)]
  G -->|work| C[Compute: serverless or machines]
  C -->|reads only what it was given| S
```

Governance decides, storage keeps the bytes, compute runs the work. More in [ARCHITECTURE](docs/ARCHITECTURE.md).

## Development

`make check` runs formatting, lints and tests. Setup, Postgres and sandbox tests: [CONTRIBUTING](docs/CONTRIBUTING.md).

## Status

Under development, not production ready.
