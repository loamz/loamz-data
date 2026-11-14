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

## The plan to the first beta (13 Nov)

**Beta means:** nearly every feature a customer needs, not a minimum. A customer signs in, points us at **their own bucket and their own
machines (or Modal account)**, and pays for those themselves: we run governance only. They bring data in, query it fast, connect their
tools, and trust that nobody else can see it. It runs in production with backups and docs, for the owner and fewer than three invited
customers. Billing is by hand and invitation only (it lives in the private Loamz
Cloud, behind `Entitlements` and `UsageSink`).

| # | Milestone | Done when | State |
|---|---|---|---|
| M1 | Merge what exists | isolation tests, SDK and CLI, security (CI, SBOM, threat model) and the docs site are in `main`, `make check` green | done |
| M2 | Fast | `bench/run.py 3` shows the machine column under 0.5 s with the same answers | done (14 Nov): 0.19 to 0.29 s warm in gVisor; Arrow results and file pruning still to do |
| M3 | Data in | a CSV or Parquet file becomes a table with `loamz import` (straight to storage), or from the console, a volume file or an https address, on compute | done (14 Nov): create, append and replace proven in gVisor on RustFS; S3 source not built |
| M4 | Governed | storage roots, row filters and column masks, erase with a provable audit chain | done; storage grants and one `Principal` type still to do |
| M5 | Production | deployed from `loamz-deploy`, backups and WAL archiving running, restore check passes | in progress: roles written, OpenBao unseal and root revoke proven, agent install proven on a clean machine; the end-to-end deploy proof is not done |
| M6 | Ready to show | docs site live, console screens for every feature, end-to-end browser tests, a hostile security review | in progress: import dialog and docs pages written; a first hostile review round is running |

**New for the beta (13 Nov decisions):** a store per storage location (the customer's own bucket: S3, R2, MinIO), a write-only API for
their storage keys into OpenBao, external objects and storage grants, an installer for a customer's machine (runner, gVisor, worker),
and a guide for pointing a Modal account at us.

**Built since 13 Nov:** a store per storage location (a catalog on a customer's bucket is read and written with that location's own credential, proven on RustFS), write-only credential custody in OpenBao, the write path on compute (import from a volume file or an https address, replace, compaction and snapshot expiry, all through two narrow doors in gVisor), job schedules and failure alerts, a statement's whole result as an Arrow file read by a signed address, a Parquet result saved in a person's volume with an idempotency key, the console's import dialog, and workspace catalog metadata and visibility-filtered search. **Written but not yet merged:** abuse limits and organization suspension, the agent installer, and the docs pages for schedules, maintenance and storage. **Next:** agent sandboxes that read data through the storage broker (proposal 0006), the BI connection, and the AI phase once its scope is confirmed.

**Confirmed by the owner (14 Nov):** the AI phase is in the beta in full: functions as agent tools, agent sandboxes, MLflow for ML (experiments, models) and for GenAI (traces, evaluations, prompts), and Loamz Agents connected to all of it. Designed in [0008](../proposals/0008-ai-phase.md); still to confirm: which of BI connectors, network isolation and traces beyond the AI phase.

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
| Row filters and column masks, enforced by rewriting the statement on the server | governance | ✅ | | | | | |
| Lineage | governance | | | | | | ❓ |
| Erase a person's data; the audit chain stays verifiable (`/audit/verify`) | governance | ✅ | | | | | |
| Personal data: column tags, retention, erasure across table snapshots | governance | | | | | | ❓ |
| **Data** | | | | | | | |
| Catalogs, schemas, tables, views, volumes | data | ✅ | | | | | |
| Iceberg REST catalog: read, write, commit | data | ✅ | | | | | |
| Bring data in from a file: `loamz import` (CSV, Parquet), written straight to storage | data | ✅ | | | | | |
| Bring data in from the console, from a volume file or an https address (create, append, replace) | data | ✅ | | | | | |
| Bring data in from an S3 location, database connectors | data | | | | | | 📅 |
| File index at commit, pruning before reads | data | | | 📅 | | | |
| Compaction and snapshot expiry, on demand or on a schedule | data | ✅ | | | | | |
| Sort on write | data | | | 📅 | | | |
| Functions (SQL, Python) as governed objects and agent tools, run in sandboxes as the caller (0008) | data | | | | 📅 | | |
| MLflow ML: experiments, runs, models, through a governing gateway in front of the upstream server (0008) | data | | | | 📅 | | |
| MLflow GenAI: traces, evaluations, prompts; Loamz Agents sends its traces and loads its prompts here (0008) | data | | | | 📅 | | |
| Materialized views | data | | | | | | ❓ |
| Search across workspace catalogs by name, description, tag and owner; return only visible securables | data | ✅ | | | | | |
| Data quality checks | data | | | | | | ❓ |
| **Compute** | | | | | | | |
| Jobs on machines, each run in gVisor | compute | ✅ | | | | | |
| Serverless SQL | compute | ✅ | | | | | |
| Active-run cap per organization | compute | ✅ | | | | | |
| Benchmark of the query path (`bench/run.py`) | compute | ✅ | | | | | |
| Warm engine (Rust, DuckDB, all cores) on machines: 0.19 to 0.29 s warm in gVisor | compute | ✅ | | | | | |
| The same warm engine on serverless (Modal) | compute | | | 📅 | | | |
| A statement's whole result as an Arrow file read by a signed address, or as Parquet saved in the person's volume | compute | ✅ | | | | | |
| Arrow results streamed while the statement runs | compute | | | 📅 | | | |
| Ranged reads instead of downloading tables, through the storage broker with a shared cache (measured: a column or a filter moves a fraction of a file) | compute | ✅ | | | | | |
| Credential cache: the same table read twice asks the store once | compute | ✅ | | | | | |
| Notifications instead of polling (a waiting request and the agent are woken when work is queued or a run ends) | compute | ✅ | | | | | |
| Sandboxes for agents: a thread's results and files in the person's volume, read through the storage broker (proposal 0006) | compute | | | | 🛠️ | | |
| Run cap per workspace (below the organization's), set by account admins | compute | ✅ | | | | | |
| Sandbox lifecycle (create, idle, stop, delete) | compute | | | | | 📅 | |
| Job schedules (cron and interval) and failure alerts to a webhook | compute | ✅ | | | | | |
| Triggers (on a table change) | compute | | | | | | ❓ |
| **Storage** | | | | | | | |
| S3-compatible store; short-lived credentials scoped to one folder (R2) | storage | ✅ | | | | | |
| Credentials and locations per workspace; paths never overlap | storage | ✅ | | | | | |
| A catalog on a registered location (its root is the location's folder) | storage | ✅ | | | | | |
| A store per location: a customer's own bucket (RustFS proven; MinIO and R2 gated on conformance tests; AWS needs our own AWS identity) | storage | ✅ | | | | | |
| Write-only storage credentials in OpenBao: rotate, check, delete | storage | ✅ | | | | | |
| External objects and storage grants | storage | | | | | | 📅 |
| Credentials through STS (RustFS; S3 untested) | storage | ✅ | | | | | |
| Default storage on RustFS, nothing to set up (development) | storage | ✅ | | | | | |
| Defined behavior when access is revoked | storage | | | | | 📅 | |
| **Secrets and recovery** | | | | | | | |
| Every secret in OpenBao; the server keeps references (dev: OpenBao in compose) | secrets | ✅ | | | | | |
| Production OpenBao: three-share unseal and root token revoked (proven on a test host); audit device, snapshots and a tested restore still to prove | secrets | | | | | 🛠️ | |
| One sign-in for operator tools: OpenBao and RustFS through Keycloak (development) | identity | ✅ | | | | | |
| The same in production: the realm template and each tool's settings (written, being tested) | identity | | | | | 🛠️ | |
| Tables readable without our server (version hint) | recovery | ✅ | | | | | |
| Backups of the catalog and sign-in (`scripts/backup.sh`) and a restore check (`scripts/restore-check.sh`) | recovery | ✅ | | | | | |
| Break-glass: an owner reads the bucket credential from OpenBao, audited (development) | recovery | ✅ | | | | | |
| Continuous (WAL) archiving and the same in production (daily backup and weekly restore check written, WAL next) | recovery | | | | | 🛠️ | |
| **Platform** | | | | | | | |
| Web console | platform | ✅ | | | | | |
| Versioned public API, OpenAPI | platform | ✅ | | | | | |
| Metrics: `/metrics` (request counts and times, queued and running runs), behind a vault token | platform | ✅ | | | | | |
| Traces | platform | | | | | 📅 | |
| Outbound traffic denied by default | platform | | | | | 📅 | |
| Production deployment (`*.data.loamz.com`) | platform | | | | | 🛠️ | |
| Limits an organization's admin sets (`Entitlements`: who may use SQL, jobs, compute) | platform | ✅ | | | | | |
| Durable usage events (outbox) and cost allocation (`UsageSink`) | platform | | | | | | 📅 |
| Self-hosted install: guide, one-organization default, upgrades | platform | | | | | | 📅 |
| Loamz Cloud: sign-up, plans, billing, one private service for every product | platform | | | | | | 📅 |
| Hybrid: our control plane, the customer's bucket and machines | platform | | | | | | ❓ |
| Sharing data across organizations | platform | | | | | | ❓ |
| Access tokens, a command-line tool and a Python SDK | platform | ✅ | | | | | |
| SQL connection for BI tools (Arrow Flight SQL, JDBC) | platform | | | | | ❓ | |
| Abuse protection: invite-only start, API rate limits, request-size limits, suspend an organization | platform | | | | | 🛠️ | |
| Prepaid or spend cap | platform | | | | | | ❓ |
| Safe upgrades: forward-only migrations, neighbouring versions work together | platform | | | | | ❓ | |
| Dependency checks in CI, releases with an SBOM, a threat model (signing is not done) | platform | ✅ | | | | | |
| User documentation site (private, at `data.loamz.com/docs`; built, not yet published) | platform | ✅ | | | | | |
| Cost budgets and alerts per workspace | platform | | | | | | ❓ |
| Compliance (SOC 2) and data residency by region | platform | | | | | | ❓ |
| Audited support access, status page, incident process | platform | | | | | | ❓ |
| Terraform provider | platform | | | | | | ❓ |
| Free tier of Loamz Cloud, when funded | platform | | | | | | ❓ |
