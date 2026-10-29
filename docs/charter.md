# Loamz Data: charter

Status: draft for the owner's review, 2026-10-29. Decisions marked **decided** came from the owner; the rest are proposals.

## What it is

The governed data platform. It stores data of every shape (tables, vectors, graphs, files), lets people, agents and apps
query and compute on it, and enforces who may do what. Chat and Agents are customers of Data. Data never calls them.

## What it owns (the source of truth)

Datasets and their catalog entries, the bytes behind them, access rules, quotas, the audit record, and the lifecycle
(retention, versions, erase, export). Nothing else.

## What it never knows

Conversations, people's messages, agents, schedules, how a chat is delivered. If a feature needs one of those, it belongs
in Chat or Agents.

## Shape (decided)

```
┌─ LOAMZ DATA ─────────────────────────────────────────────────────────────────────────┐
│                                                                                      │
│  ┌─ GOVERNANCE  (the only door; every call enters here) ──────────────────────────┐  │
│  │                                                                                │  │
│  │  ┌──────────┐  ┌───────┐  ┌───────┐  ┌───────┐    Catalog entry: the USER sets │  │
│  │  │ Catalogs │  │  IAM  │  │ Quota │  │  API  │    classification, placement,   │  │
│  │  └──────────┘  └───────┘  └───────┘  └───────┘    consistency, retention class │  │
│  │                                                                                │  │
│  │  ┌──────────┐  ┌────────────────┐  ┌─────────┐    Governance only checks,      │  │
│  │  │LifeCycle │  │ AccessPolicies │  │  Audit  │    limits and records.          │  │
│  │  └──────────┘  └────────────────┘  └─────────┘                                 │  │
│  │                                                                                │  │
│  │  API = the single entry. It holds no business logic.                           │  │
│  │                                                                                │  │
│  │                                                                                │  │
│  └────────────────────────────────────────────────────────────────────────────────┘  │
│                     │  governed handles                         │                    │
│                     ▼                                           ▼                    │
│  ┌─ COMPUTE ──────────────────────────┐    ┌─ STORAGE ────────────────────────────┐  │
│  │                                    │    │                                      │  │
│  │  ┌──────────────────────────────┐  │    │  ┌────────────────────────────────┐  │  │
│  │  │         Query Engine         │  │    │  │ LAKE_CLIENT  (also a library)  │  │  │
│  │  └──────────────────────────────┘  │    │  └────────────────────────────────┘  │  │
│  │                                    │    │                  │                   │  │
│  │  ┌──────────────────────────────┐  │    │      ┌───────────┴─────────┐         │  │
│  │  │             Jobs             │  │    │      ▼                     ▼         │  │
│  │  └──────────────────────────────┘  │    │ ┌─────────┐          ┌───────────┐   │  │
│  │                                    │    │ │LakeBase │          │ LakeHouse │   │  │
│  │  ┌──────────────────────────────┐  │    │ └─────────┘          └───────────┘   │  │
│  │  │          Pipelines           │  │    │      │             ┌───────┴───┐     │  │
│  │  └──────────────────────────────┘  │    │      ▼             ▼           ▼     │  │
│  │                                    │    │ ┌─────────┐  ┌───────────┐┌─────────┐│  │
│  │  ┌──────────────────────────────┐  │    │ │Postgres │  │ObjectStore││FileStore││  │
│  │  │          Sandboxes           │  │    │ └─────────┘  └───────────┘└─────────┘│  │
│  │  └──────────────────────────────┘  │    │      │             │           │     │  │
│  │                                    │    │      │         ┌───────┐   ┌───────┐ │  │
│  │                                    │    │      │         │  S3   │   │SQLite │ │  │
│  │  never opens storage itself:       │    │      │         └───────┘   └───────┘ │  │
│  │  gets a scoped handle per job      │    │      ▼             ▼           ▼     │  │
│  │                                    │    │ ┌──────────────────────────────────┐ │  │
│  │                                    │    │ │ Relation · Vector · Graph · Blob │ │  │
│  │                                    │    │ │     same API on any backend      │ │  │
│  │                                    │    │ └──────────────────────────────────┘ │  │
│  └────────────────────────────────────┘    └──────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

- Governance is the only door, and the API is its single entry (no business logic in it). Compute gets a scoped, short-lived
  handle per job and never opens storage itself.
- The user chooses classification, placement and consistency on the catalog entry. Governance checks them, enforces
  them and records them. It does not choose.

## How compute reaches storage

Compute never holds the raw LAKE_CLIENT or any storage credential. Governance creates a **governed client** for each job
after IAM and AccessPolicies allow it: the same client, wrapped, limited to named datasets and operations, short-lived,
and recording its use for Audit.

- **Trusted compute** (the query engine and pipelines, inside the Data server) gets an in-process governed client.
- **Untrusted compute** (sandboxes running code an agent wrote) gets no storage credential at all. It reads and writes
  through the API or a bulk-stream ticket, so every access is checked and logged.

## What each Governance box means

- **API:** the single entry. It authenticates and routes. It holds no business logic.
- **IAM:** roles and role assignments. A principal (a person, an agent, a job) is given a role at a scope (tenant,
  catalog, schema, object). Built-in roles (decided), the same three everyone expects: **Owner** (everything below, plus assign roles and change
  policies), **Contributor** (create, change and delete data and catalog entries; cannot assign roles) and **Reader**
  (read only, still subject to AccessPolicies). Custom roles are a later feature; nothing like "Steward" is built in,
  and approval workflows belong to apps on top, not to Data. This is where a role is added or removed. Who the principal *is* comes from the shared sign-in layer; Data only reads the token.
- **AccessPolicies:** rules on the data itself, checked after IAM allows the action: row filters, column masks, rules by
  classification (sealed, personal), deny rules, sharing limits. IAM says what kind of thing you may do; policies say
  which rows and columns and under what conditions.
- **Catalogs:** names (tenant, catalog, schema, then table, volume, model or function), owners, schemas, and the entry
  fields the user sets: classification, placement, consistency, retention class.
- **Quota:** limits on storage and compute, per person and per tenant.
- **LifeCycle:** what happens to data over time: retention, versions and snapshot cleanup, archive, `erase(subject)`,
  `export(subject)`.
- **Audit:** an append-only record of who did what and with what result. It must survive an erase (it proves the erase
  happened without keeping the person's content). Lineage (what was derived from what) is stored as a graph.

Order on every call: authenticate, IAM, AccessPolicies, Quota, run, Audit. A scoped handle is minted only after IAM and
AccessPolicies both allow it.

## The storage contract

1. **Universal surface (decided).** Every backend can offer Relation, Vector, Graph and Blob. The user chooses where a
   dataset lives. Vector and Graph are layers over Relation; accelerators (pgvector, sqlite-vec, Lance indexes, AGE) are
   optional and never change what a query means (exact search stays available).
2. **Two guarantees, named.** `transaction`: multi-statement, multi-table, constraints (Postgres, SQLite).
   `snapshot`: atomic commit per dataset, consistent reads, one writer per dataset (file and object formats). A request
   for `transaction` on a placement that cannot give it is refused, never silently weakened.
3. **One writer per dataset.** The Data server serialises writes. In embedded use a lock does.
4. **Sensitivity classes:** normal, personal, sealed. Sealed data is opaque bytes: no scanning, OCR or embedding.
5. **Erase and export are real.** `erase(subject)` finds everything tagged with the subject across every backend,
   including derived data and old versions, and a test proves the bytes are gone. `export(subject)` is the same walk.
6. **Derived data is marked derived** (embeddings, indexes, OCR text, thumbnails) and can be rebuilt. It is never the
   only copy.
7. **Formats are protocols we use, not things we build** (Lance, Parquet over a byte store).

## LAKE_CLIENT: two ways to use it (decided: B)

- **Embedded:** a library any product may use for its own private data. The caller is trusted; there is no central policy.
- **Governed:** through the Data server. Anything shared, or that another product or person may read, goes this way.

Rule: private product state may use the library; shared or governed data must use the server.

## Dependency budget

Two kinds of infrastructure: a SQL database (SQLite or Postgres) and a byte store (local disk or S3). Everything else is
our code or one embedded library behind an interface (Lance is the candidate). Data can run with no database server
(files only); Postgres is a first-class option for people who need multi-writer transactions.

## How it grows on its own

Volume and shapes of data, more formats and connectors, compute scale. Contract changes get a new version (`/api/v1`,
`/api/v2`); nothing is edited in place. Release, scale and storage are independent of Agents and Chat.

## Decisions after reading Unity Catalog (owner, 2026-10-29)

Unity Catalog is a catalog and governance layer only. Loamz Data is wider on purpose: AI analysis needs strict sandboxes
that run code next to the data, and that is best managed in the platform. Keep the inside layered so governance and
compute/storage could still be split into separate services later.

- **Managed locations only.** Data owns every byte it stores. No external locations, no "bring your own bucket" for now.
  Storage credentials live on the server and never reach a sandbox.
- **Grants underneath roles.** IAM stores grants (privilege on an object for a principal). Owner, Contributor and Reader
  are named bundles of grants. Service principals (jobs, agents) are ordinary principals. Custom roles come later with
  no redesign.
- **One definition per permission rule,** shared by every endpoint, so two doors to the same action cannot drift.
- **Erase and drop are durable tasks.** A task record in the catalog database, a worker that leases and retries it,
  a verification step, and an Audit entry when it is done. Never a best-effort call.
- **Audit is not observability.** Audit records who did what to which data (append-only, survives erase).
  Observability is operator health: metrics, logs, traces, readiness. It is a thin cross-cutting strip, holds no
  personal data, and is separate from Audit.
- **Views are an object kind** (a governed saved query). In v1 they are also the access-restriction mechanism: grant a
  view, not the table. Row filters, column masks and attribute-based rules are enterprise features and come later.
- **AI-native object kinds,** beyond tables, volumes, functions and models: VectorIndex (records its embedding model and
  dimension), Graph, Memory (per agent or person, tagged by subject, with a lifetime) and Artifact (what a sandbox made).
- **Classification is inherited.** Whatever a sandbox produces carries the strictest classification of what it read, and
  its lineage is recorded. Vector search obeys the same permissions as the rows behind it.
- **Sandboxes are the heart.** No network by default, fixed time and memory, no storage credential, reads and writes only
  through governed calls. They get their own design document next.
- **Credentials and Keycloak.** One identity provider. People sign in with OIDC. Service principals use client
  credentials. When an agent acts for a person the call carries both (token exchange), Data allows only what both are
  allowed, and Audit records both.
- **UC-compatible API is an adapter, not the foundation.** Our own spec comes first. A UC-compatible catalog surface may
  be added so existing tools can connect, but it must never limit vectors, graphs, agent storage or sandboxes.
- **Observability** (above), **database migrations** for the catalog, and **change events out** are part of the first build.

## Open items

- Audit as its own box (owner chose to keep it), and lineage stored as a graph in our own Graph capability.
- Lance, sqlite-vec and AGE are proposals: prove each with a short spike (install on aarch64, concurrent writers on local
  disk, SeaweedFS, B2 and Oracle, erase then compact then scan the files, SQL over the format, export to Parquet).
- Today `loamz-engine` is the seed of LAKE_CLIENT (Postgres and SQLite databases, a file store, vectors, memory,
  documents). It should be regrouped into capabilities by backends.
- Data has never run in production. First consumer to try: Agents' files and documents.
- Conformance suite per backend: put, query, snapshot, erase, export, restore, tenant isolation, quota.
