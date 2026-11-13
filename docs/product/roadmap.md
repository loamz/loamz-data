# Roadmap

Where Loamz Data is going, in phases until the first beta; after the beta, in versions. Features may move between phases as we
learn. What each feature means: [requirements](requirements.md). How each is built: [design](../design/). Each phase is planned in a
proposal: phase 1 in [0002 foundation](../proposals/0002-foundation.md), phase 2 in [0001 fast data plane](../proposals/0001-fast-data-plane.md).

## Phases to the first beta

### Phase 1: Foundation ([0002](../proposals/0002-foundation.md))
The product's model in the code: catalogs owned by one workspace, service principals, storage set per workspace with credentials,
locations and roots, any S3-compatible store through STS, and a first login with a default admin. A fresh checkout runs with nothing
to configure, on RustFS.

### Phase 2: Fast ([0001](../proposals/0001-fast-data-plane.md))
Speed from design, measured: a benchmark first, then a warm engine with all cores, Arrow results streamed, files pruned by an index
before reading and read by range, notifications instead of polling, and tables kept in shape by compaction.

### Phase 3: AI
Data, ML and GenAI under one governance: functions as agent tools that run in our sandboxes, not in the caller; MLflow for ML
(experiments, models) and GenAI (traces, evals, prompts) through the standard client; sandboxes for agents with a query path into
the catalog; row filters and column masks.

### Phase 4: Production, then beta
Safe to run for others: secrets in OpenBao, data readable without our server, backups, network isolation, quotas and limits an
admin sets (`Entitlements`), revocation rules, metrics and traces, and the production deployment. The first beta ships at the end of this phase. Under consideration for it: bring data in, access tokens with a CLI and SDK,
a SQL connection for BI tools, job schedules and alerts, abuse protection, safe upgrades and user docs.

> **Note:** this roadmap is a plan, not a promise. Built features are marked in the Today column.

## Full roadmap

✅ built · 🛠️ in progress · 📅 planned · ❓ under consideration

| feature | area | today | phase 1 | phase 2 | phase 3 | phase 4 | after beta |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|
| **Identity and access** | | | | | | | |
| Sign-in with OpenID Connect (Keycloak) | identity | ✅ | | | | | |
| Organization decides who can use which workspace | identity | ✅ | | | | | |
| Groups as principals | identity | ✅ | | | | | |
| First login: a default admin creates organizations and adds people | identity | ✅ | | | | | |
| Service principals in a workspace | identity | ✅ | | | | | |
| Federation and MFA enforced | identity | | | | | 📅 | |
| Enterprise extras: SCIM sync, SSO integrations, long audit retention | identity | | | | | | ❓ |
| **Governance** | | | | | | | |
| Grants, ownership and inheritance (Unity Catalog privileges) | governance | ✅ | | | | | |
| 404 for what you cannot see; every refusal audited | governance | ✅ | | | | | |
| Tamper-evident audit chain | governance | ✅ | | | | | |
| Apps act as a principal, never with more access | governance | ✅ | | | | | |
| A catalog belongs to one workspace | governance | ✅ | | | | | |
| Grants on compute and storage | governance | | 📅 | | | | |
| Row filters and column masks | governance | | | | 📅 | | |
| Lineage | governance | | | | | | ❓ |
| Personal data: column tags, retention, erasure across snapshots | governance | | | | | | ❓ |
| **Data** | | | | | | | |
| Catalogs, schemas, tables, views, volumes | data | ✅ | | | | | |
| Iceberg REST catalog: read, write, commit | data | ✅ | | | | | |
| Bring data in: file upload, load from S3 or a URL, database connectors | data | | | | | ❓ | |
| File index at commit, pruning before reads | data | | | 📅 | | | |
| Compaction, sort on write, snapshot expiry | data | | | 📅 | | | |
| Functions (SQL, Python) as agent tools, run in sandboxes | data | | | | 📅 | | |
| MLflow ML: experiments and models | data | | | | 📅 | | |
| MLflow GenAI: traces, evals, prompts | data | | | | 📅 | | |
| Materialized views | data | | | | | | ❓ |
| Search across catalogs by name, description, tag, owner | data | | | | | | ❓ |
| Data quality checks | data | | | | | | ❓ |
| **Compute** | | | | | | | |
| Jobs on machines, each run in gVisor | compute | ✅ | | | | | |
| Serverless SQL | compute | ✅ | | | | | |
| Active-run cap per organization | compute | ✅ | | | | | |
| Benchmark of the query path (`bench/run.py`) | compute | ✅ | | | | | |
| Warm engine (Rust, DuckDB, all cores), on machines and serverless | compute | | | 📅 | | | |
| Arrow results, streamed | compute | | | 📅 | | | |
| Ranged reads instead of downloading tables | compute | | | 📅 | | | |
| Notifications instead of polling; credential cache | compute | | | 📅 | | | |
| Sandboxes for agents: a workspace folder per thread, a query path | compute | | | | 📅 | | |
| Quotas per workspace; sandbox lifecycle | compute | | | | | 📅 | |
| Schedules, triggers and failure alerts for jobs | compute | | | | | ❓ | |
| **Storage** | | | | | | | |
| S3-compatible store; short-lived credentials scoped to one folder (R2) | storage | ✅ | | | | | |
| Credentials and locations per workspace; paths never overlap | storage | ✅ | |
| One root per catalog; managed and external objects; storage grants | storage | | 📅 | | | | |
| Credentials through STS (RustFS; S3 untested) | storage | ✅ | | | | | |
| Default storage on RustFS, nothing to set up (development) | storage | ✅ | | | | | |
| Defined behavior when access is revoked | storage | | | | | 📅 | |
| **Secrets and recovery** | | | | | | | |
| Every secret in OpenBao; the server keeps references (dev: OpenBao in compose) | secrets | ✅ | | | | | |
| Production OpenBao: HA, audit device, snapshots, a tested restore | secrets | | | | | 📅 | |
| One sign-in for operator tools: OpenBao and RustFS through Keycloak (development) | identity | ✅ | | | | | |
| The same in production: the realm template and each tool's settings | identity | | | | | 📅 | |
| Tables readable without our server (version hint) | recovery | ✅ | | | | | |
| Continuous backups of the catalog and sign-in (Keycloak); break-glass | recovery | | | | | 📅 | |
| **Platform** | | | | | | | |
| Web console | platform | ✅ | | | | | |
| Versioned public API, OpenAPI | platform | ✅ | | | | | |
| Metrics and traces | platform | | | | | 📅 | |
| Outbound traffic denied by default | platform | | | | | 📅 | |
| Production deployment (`*.data.loamz.com`) | platform | | | | | 📅 | |
| Limits an organization's admin sets (`Entitlements`), with safety caps | platform | | | | | 📅 | |
| Durable usage events (outbox) and cost allocation (`UsageSink`) | platform | | | | | | 📅 |
| Self-hosted install: guide, one-organization default, upgrades | platform | | | | | | 📅 |
| Loamz Cloud: sign-up, plans, billing, one private service for every product | platform | | | | | | 📅 |
| Hybrid: our control plane, the customer's bucket and machines | platform | | | | | | ❓ |
| Sharing data across organizations | platform | | | | | | ❓ |
| Access tokens, a command-line tool and a Python SDK | platform | | | | | ❓ | |
| SQL connection for BI tools (Arrow Flight SQL, JDBC) | platform | | | | | ❓ | |
| Abuse protection: invite-only start, prepaid or spend cap, API rate limits, suspend an organization | platform | | | | | ❓ | |
| Safe upgrades: forward-only migrations, neighbouring versions work together | platform | | | | | ❓ | |
| Dependency checks in CI, signed releases with an SBOM, a threat model | platform | | | | | ❓ | |
| User documentation site (private, at `data.loamz.com/docs`) | platform | | | | | ❓ | |
| Cost budgets and alerts per workspace | platform | | | | | | ❓ |
| Compliance (SOC 2) and data residency by region | platform | | | | | | ❓ |
| Audited support access, status page, incident process | platform | | | | | | ❓ |
| Terraform provider | platform | | | | | | ❓ |
| Free tier of Loamz Cloud, when funded | platform | | | | | | ❓ |
