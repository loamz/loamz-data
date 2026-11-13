# Decisions

One line each: date · decision · why. Newest last. Change one by adding a line that replaces it.

| Date | Decision | Why |
|---|---|---|
| 2026-10-29 | Privileges are Unity Catalog's; Reader, Contributor, Owner are bundles of grants | familiar to data teams; custom roles later without redesign |
| 2026-10-29 | An organization is a Keycloak organization alias; a person's own space is `personal:<subject>` | one identity system for every product |
| 2026-10-29 | Account admins live in Data; Keycloak's `account-admin` only creates an organization's Data account | admins change without touching Keycloak |
| 2026-10-29 | An agent acting for a person gets the intersection of both rights | an agent never sees more than its person |
| 2026-10-29 | Audit keeps ids, never content; one hash chain per organization; survives erase | proves who did what without keeping personal data |
| 2026-10-29 | Each workspace is its own site (`w-<id>.<base>`); the server does the Keycloak token exchange | Keycloak cannot allow wildcard origins |
| 2026-10-29 | Compute belongs to the organization; a run is not retried | a job may have side effects |
| 2026-10-31 | Tables are Iceberg only; this server is their REST catalog; DuckDB reads and writes them | open format, one table model |
| 2026-10-31 | Data bytes live in R2, one prefix per securable; machines get read-only credentials for minutes | the bucket key never leaves the server |
| 2026-10-31 | Serverless compute takes SQL only, over data classified `normal` | it runs outside our machines |
| 2026-11-02 | Data depends on no Agents code; `layering.rs` checks it | Data must work alone |
| 2026-11-12 | Writes only through Loamz; outside tools may read through Iceberg REST | keeps our metadata index complete and our internals free |
| 2026-11-12 | Parquet at rest, DuckDB engine, Arrow on the wire | each format wins its tier's bottleneck |
| 2026-11-12 | One Rust worker with embedded DuckDB, on machines and on Modal | one engine, no Python on the query path |
| 2026-11-12 | File metadata indexed in Postgres in the same transaction as the table pointer | plan a query with one indexed read, never out of step |
| 2026-11-12 | gVisor kept; warm workers serve one tenant and trust scope only | speed without crossing tenants |
| 2026-11-12 | No extra storage engines (Postgres + byte store only) | two developers; add one only on benchmark evidence |
| 2026-11-12 | An organization holds workspaces; a workspace is a project | one place per project: its people, data and compute |
| 2026-11-12 | The organization controls access to workspaces (owners, admins, like an Azure owner); people sign in once with Keycloak | one place decides who can use which project |
| 2026-11-12 | Service principals belong to a workspace | an app (BI, ML, a pipeline) works in one project only |
| 2026-11-12 | Each catalog has its own governance: metadata (tables, versions), grants and audit; Postgres is only where it is kept, keyed by catalog | a catalog is self-contained; the organization keeps only its own records |
| 2026-11-12 | A catalog belongs to exactly one workspace and is never shared; BI and ML are service principals, not workspaces | simple isolation; access is granted to principals, not by sharing catalogs |
| 2026-11-12 | Compute belongs to the workspace and may join across its catalogs; each catalog says which compute may read it | joins work; each catalog keeps control |
| 2026-11-12 | Each catalog has its own S3-compatible bucket (a default if none is set) | bring your own bucket; data stays where its owner wants |
| 2026-11-12 | Loamz Agents is a separate app that connects through the public API, like a connector, acting for a member or a service principal | the platform does not depend on the agents |
| 2026-11-12 | Local and default storage is RustFS; credentials come from STS `AssumeRole` (R2 keeps its own API) | one way for every S3-compatible store; MinIO is no longer maintained |
| 2026-11-12 | Agents read data only through the catalog's query path (Arrow Flight); their sandbox mounts only the thread's workspace folder | policy applied at planning, only answers move, no storage keys in sandboxes |
| 2026-11-12 | The core is Apache-2.0 | permissive, as Unity Catalog and Iceberg are; adoption first |
| 2026-11-13 | One abstraction, the principal: a member or a service principal. Grants, runs and apps all act as a principal | one code path for who is acting; apps add no new kind of identity |
| 2026-11-13 | The catalog governs MLflow assets as well as tables and files: ML (experiments, models) and AI (traces, evals, prompts); the standard `mlflow` client works against Loamz | one governance for data, ML and GenAI; no separate ML system |
| 2026-11-13 | Storage settings belong to the workspace: credentials (keys or a role), locations (bucket or prefix) and one root per catalog (Unity Catalog also allows a schema root; we don't). Buckets themselves can be shared. Replaces "each catalog has its own bucket" | the bytes can be common, the access is per project |
| 2026-11-13 | Unity Catalog's storage rules: managed or external objects, no two locations or objects overlap, storage grants (`READ FILES`, `WRITE FILES`, `CREATE EXTERNAL`, `CREATE MANAGED STORAGE`) | proven model; paths can never leak into each other |
| 2026-11-13 | Each workspace starts with default storage (RustFS), no setup | getting started needs no storage settings |
| 2026-11-13 | Bytes are reached with vended credentials (STS for RustFS and S3, R2's own API); Iceberg remote signing only for stores without STS or strict data | DuckDB needs vended credentials; signing never hands out keys |
| 2026-11-13 | Governance is one layer over who, compute, data and storage (grants on principals, compute, catalog objects, credentials and locations). Compute runs as a principal, asks governance, then reads and writes storage directly with short-lived scoped credentials | one place decides everything; bytes never pass through governance |
| 2026-11-13 | Secrets live in OpenBao. The catalog keeps references; the server holds secrets in memory only | open source (MPL 2.0, Linux Foundation), free, no enterprise tier; runs apart from our server |
| 2026-11-13 | Data is readable without the Loamz server: each commit writes Iceberg's version hint; Postgres is backed up continuously; owners have audited break-glass access | customers own their data, even when we are down |
| 2026-11-13 | One Loamz Cloud for every Loamz product (accounts, sign-up, plans, billing, usage). Data, Agents and Chat are open products with the same three seams | one customer, one account, one invoice; usage crosses products; billing is built once |
| 2026-11-13 | One open codebase runs as Loamz Cloud, self-hosted or hybrid. Cloud-only features live outside the data server, and the core never needs the cloud to serve authorized work | self-hosters get a whole product; no forked cloud to maintain |
| 2026-11-13 | Three seams: `Entitlements`, `UsageSink`, `OrgSignup`, each with a default that needs no Loamz account. Self-hosted limits are set by the admin with safety caps, never unlimited | a self-hosted install has limits, not plans |
| 2026-11-13 | Usage events are written in the same transaction as the change (an outbox) and delivered at least once with unique ids; billing is never called while work runs | a lost event is lost money; a cloud outage must not stop work |
| 2026-11-13 | Loamz Cloud keeps many organizations in one Postgres: one `orgs` row each, `org_id` on every table. A separate database for an organization only on request | simple to run; separation when someone asks for it |
| 2026-11-13 | Loamz Cloud is paid only, with no free tier until there is funding for it; without paying, self-host (invite-only until billing exists) | free compute is what abusers look for, and we would pay for it; self-hosting costs us nothing |
| 2026-11-13 | Default storage is one RustFS bucket per installation with a prefix per workspace; service principals sign in with tokens we issue; only organization admins create workspaces | simplest start; roots keep paths apart; one token kind also serves scripts |
| 2026-11-13 | The product's website and these docs are private for now, under the Loamz platform at `data.loamz.com` and `data.loamz.com/docs` | nothing public until the beta |
| 2026-11-13 | One way to keep secrets: OpenBao's HTTP API, in development too (a container in `compose.yml`, like Keycloak). No file backend. The server's own login to OpenBao is the one bootstrap secret | one code path; no secret in a file the server reads |
| 2026-11-13 | One sign-in for people: Keycloak. The console, OpenBao's UI and RustFS's console are all Keycloak clients; a person's access to a tool is a client role (`admin` on `loamz-openbao`, `consoleAdmin` on `loamz-rustfs`, `platform-admin` on `loamz-data`). Machines never use a person's login: the server uses an AppRole and access keys | one identity, one place to add or remove a person; the tools' audit logs show real names |
| 2026-11-13 | The cloud creates an organization once; each product creates its row with the same id. Self-hosted, `OrgSignup` is admin-only | each product still works alone |
| 2026-11-13 | Hybrid (our control plane, their bucket and machines) is under consideration; access to their bucket is a role in their account that trusts our server | follows from compute connecting out and reading storage directly; we hold no long-lived key |
