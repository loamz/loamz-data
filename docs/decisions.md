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
| 2026-11-12 | The organization controls access to workspaces (owners, admins, like an Azure owner); people sign in once with Keycloak | one place decides who may use which project |
| 2026-11-12 | Service principals belong to a workspace | an app (BI, ML, a pipeline) works in one project only |
| 2026-11-12 | Each catalog has its own governance: metadata (tables, versions), grants and audit; Postgres is only where it is kept, keyed by catalog | a catalog is self-contained; the organization keeps only its own records |
| 2026-11-12 | A catalog belongs to exactly one workspace and is never shared; BI and ML are service principals, not workspaces | simple isolation; access is granted to principals, not by sharing catalogs |
| 2026-11-12 | Compute belongs to the workspace and may join across its catalogs; each catalog says which compute may read it | joins work; each catalog keeps control |
| 2026-11-12 | Each catalog has its own S3-compatible bucket (a default if none is set) | bring your own bucket; data stays where its owner wants |
| 2026-11-12 | Loamz Agents is a separate app that connects through the public API, like a connector, acting for a member or a service principal | the platform does not depend on the agents |
| 2026-11-12 | Local and default storage is RustFS; credentials come from STS `AssumeRole` (R2 keeps its own API) | one way for every S3-compatible store; MinIO is no longer maintained |
| 2026-11-12 | Agents read data only through the catalog's query path (Arrow Flight); their sandbox mounts only the thread's workspace folder | policy applied at planning, only answers move, no storage keys in sandboxes |
