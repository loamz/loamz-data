# Loamz Data: implementation plan

Status: draft for the owner's review, 2026-10-29. It builds the design in `charter.md` and applies what we learned from
Unity Catalog and from the tests on the Oracle server. Sizes are rough (S = days, M = 1 to 2 weeks, L = several weeks) for
one owner working with an AI pair. Nothing here is a promise of dates.

## Language: Rust first (decided, owner)

Why: Loamz runs on small edge machines, and measured on the live server (2026-10-29) the Rust chat server serves the
live chat in 9 MB of memory, while the Python agents server takes 148 MB and Keycloak (Java) 650 MB. The modern data
stack's core is also Rust (Arrow, DataFusion, Lance, `object_store`, OpenDAL, Polars, delta-rs, iceberg-rust), so Rust
reaches it directly with no language bridge, and the same crates and code are shared with Chat. One static binary is
easy to deploy and to run on an edge box.

- **Rust:** the Data server (control plane and data plane), the lake client, the sandbox runner, and the `loamz-rs` client.
  Built on axum, sqlx, `object_store` or OpenDAL, `arrow`, and Lance or DataFusion where they fit. Same layering as Chat
  (domain, ports, adapters).
- **Python stays where it is the guest or the glue:** the code an agent writes inside a sandbox (DuckDB, Polars, pandas),
  the Agents server (LLM libraries), and a Python SDK generated from the API spec.
- **The existing Python engine and server are the executable reference,** not a base to extend. They are deleted once the Rust
  server passes the same black-box contract suite. No backward compatibility is kept.
- **Tests:** the goal is a small, language-neutral **contract suite** run against the HTTP API, not a large test count.
  The same suite judges the Python reference and the Rust server.
- **Budgets, checked from the first week:** idle memory of the Data server under 30 MB, start in under 200 ms, one
  static binary. A budget that cannot be met is a design alarm.
- **Costs we accept:** slower iteration and long builds (Chat's build folder is 43 GB). Mitigate with a shared target
  folder, building in CI and not on the laptop, and a small feature set.

## Phase 0: decide, de-risk, walking skeleton (M)

Deliverables: the charter committed; a decision log; the API written as an OpenAPI spec first; a conformance-test
skeleton; CI that runs tests on every push (there is none today).

Short spikes, each ending in a written result:
1. **Lance** on local disk, SeaweedFS, B2 and R2: atomic commit, erase then compact then scan for the bytes, vector search.
2. **sqlite-vec and pgvector:** same results on both, and exact search as the fallback.
3. **DuckDB lock-down:** restrict file access to chosen folders, no extension loading, memory cap, pinned extensions.
4. **Remote mounts** against a dedicated B2 and R2 test bucket (rclone, SeaweedFS remote, JuiceFS, geesefs, Mountpoint for S3,
   and DuckDB straight over S3 as the no-mount baseline). Then the same on Azure (Premium Files NFS and blobfuse2).
5. **Walking skeleton in Rust:** an axum server with a catalog on SQLite, one grant check, an audit entry and a health
   endpoint, measured against the footprint budget. It also fixes the project layout.
6. **Query engine for the governed door:** DataFusion (pure Rust, extensible with policy-aware tables) versus embedded DuckDB
   (more complete, heavier build). The sandbox guest keeps DuckDB and Polars in Python either way.

Exit: every spike has a result file, and the open items in the charter are closed or consciously deferred.

## Phase 1: governance core (L)

Rust. A catalog database (SQLite for one machine, Postgres at scale) with: tenants, catalogs, schemas and objects
(tables, volumes, views); principals (people and service principals); grants with Owner, Contributor and Reader as named
bundles; an append-only audit log; quotas and metering; durable lifecycle tasks (drop and erase run as leased, retried tasks);
a migration tool; a metrics and readiness strip; an outbound events feed. Identity from Keycloak: OIDC for people, client
credentials for service principals, token exchange when an agent acts for a person (both identities recorded, the
intersection allowed). **One authorization function** that every endpoint calls.

Exit tests: the full role matrix; tenant isolation; erase killed mid-way resumes and finishes; audit survives erase;
two endpoints that authorise the same action cannot disagree.

## Phase 2: storage, native cells only (L)

LAKE_CLIENT as a library, governed when used through the server. Relation on SQLite and Postgres. Blob on local disk and
S3-compatible stores. **Volume = a POSIX path that the infrastructure mounts** (local folder, NFS, blobfuse2, SeaweedFS);
the runner binds a subfolder. Snapshots, `erase(subject)`, `export(subject)`, quotas, restore drill.

Exit: the contract suite (put, query, snapshot, erase, export, restore, tenant isolation, quota) passes on every backend we
claim. **First consumer:** Loamz Agents' files move to Data volumes on the live server.

## Phase 3: compute and sandboxes (L)

- **Runner** (Rust, a process supervisor over `runsc`) with swappable backends: gVisor (this server, proven), bubblewrap (dev and trusted internal), microVM later on a
  KVM host. Limits are written in the sandbox's own OCI config, and the runner verifies they are in force before running code.
  No network, read-only inputs, one writable workspace, hard kill on timeout.
- **Inputs are governed mounts:** snapshots of what the person may read, plus their session volume. Uploads become volumes
  with an expiry. Nothing is read through a mounted path on the host; outputs are collected through the API.
- **Query:** embedded DuckDB and Polars in the sandbox; a governed query door for transactional tables.
- **Outputs become Artifacts** with inherited classification and recorded lineage. A short environment brief for the model
  (what is mounted, file previews, libraries, limits) is generated from the catalog.
- **Jobs and Pipelines:** a queue with time, memory and token limits; ingest (document reader, chunking, embeddings).

Exit: a person uploads an Excel file in an agent session, the agent analyses it in a sandbox, and a chart appears as an
Artifact; Audit shows who did what. Plus a hostile-code suite (escape attempts, memory and process caps, frozen mount).

## Phase 4: AI-native object kinds (M)

VectorIndex (sqlite-vec and pgvector first, Lance when size demands; records its embedding model), Graph (edge tables with
traversal), Memory (subject-tagged with a lifetime), Views as securables (the v1 access mechanism: grant a view, not the
table). Vector search obeys the permissions of the rows behind it. Agents' memory and documents move to Data.

## Phase 5: standards, scale and reach (L)

A UC-compatible catalog adapter (read first: list, then temporary credentials), Lance and Parquet on object stores,
SeaweedFS and Azure volumes, a Kubernetes runner, a microVM tier, SDKs generated from the spec (Python and Rust), updated
`data.loamz.com`.

## Not doing yet

External locations and federation, sharing, row filters, column masks and attribute-based rules, materialized views,
custom roles, multi-region, and writing Delta or Iceberg tables.

## Risks to watch

1. Sandbox escape or resource exhaustion (mitigate: gVisor, config-level limits, tiers, a hostile-code suite).
2. Erase correctness across backends, backups and derived data (mitigate: durable tasks, conformance tests, per-subject keys later).
3. Scope: Data is wider than a catalog (keep Governance and Compute/Storage strictly layered so they could split).
4. Lance and mount behaviour over remote stores is unproven (mitigate: the Phase 0 spikes decide).
5. Rust build size and iteration speed (mitigate: CI builds, shared target folder, few features, the walking skeleton first).

## Milestones you can see

1. Catalog and roles working with an audit trail (end of Phase 1).
2. Agents' files stored in Data on the live server (end of Phase 2).
3. Excel in, chart out, inside a gVisor sandbox (end of Phase 3).
