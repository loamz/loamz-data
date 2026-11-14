# Requirements

Everything Loamz Data does. Who it is for: [overview](overview.md). How it is built: [architecture](../architecture/overview.md).
Status: **Built** is in the code today; **Planned** names its proposal and task in [proposals](../proposals/).

## 1. What it does

### Organization and access
| id | requirement | status |
|---|---|---|
| O1 | People sign in once (OpenID Connect, Keycloak). An organization is a Keycloak organization. | Built |
| O2 | The organization decides who can use which workspace: owners and admins assign people. | Built |
| O3 | A new installation has a default admin who creates organizations and adds people. An organization is set up the first time its admin signs in, with a workspace to start in: nobody asks for it. | Built (0002 task 2) |

### Workspaces and principals
| id | requirement | status |
|---|---|---|
| W1 | A workspace is a project with its own members. | Built |
| W2 | Members get entitlements: `sql-access`, `compute-create`, `sandbox-access`. | Built |
| W3 | Groups are principals; a grant to a group reaches its members. | Built |
| W4 | Service principals (BI, ML apps, pipelines) belong to one workspace and sign in with tokens we issue. | Built (0002 task 4) |

### Governance
| id | requirement | status |
|---|---|---|
| G1 | Grants use Unity Catalog's privileges and ownership, and pass down the tree. | Built |
| G2 | A caller who cannot see a thing gets 404, never 403. Every refusal is audited. | Built |
| G3 | Every change and its audit entry are one transaction; each organization has a tamper-evident hash chain. | Built |
| G4 | An app gets only what both it and its principal can do. | Built |
| G5 | A catalog says which of its workspace's compute may read it (built, 0002 task 4). Grants on storage (`READ FILES`, `CREATE EXTERNAL LOCATION`) wait for roots on a location and external objects, which would enforce them (0002 task 5). | Compute policy built; storage grants planned |
| G6 | Policies: row filters and column masks, applied when a query is planned. | Built |
| G7 | Personal data: tags on columns (such as PII) that drive masks, retention, and erasing one person's data across a table's snapshots. | Under consideration |

### Data
| id | requirement | status |
|---|---|---|
| D1 | Catalogs, schemas, tables, views and volumes. | Built |
| D2 | A catalog belongs to exactly one workspace and is never shared. | Built (0002 task 4) |
| D3 | Tables are Apache Iceberg. Loamz Data is their Iceberg REST catalog; any Iceberg client reads them (tested with DuckDB). | Built |
| D4 | A write commits through the catalog: the table pointer and its audit entry move in one transaction. | Built |
| D5 | Volumes hold files of any format. | Built |
| D6 | Functions (SQL or Python), also usable as agent tools. | Planned |
| D7 | MLflow: ML (experiments, models) and GenAI (traces, evals, prompts). The standard `mlflow` client works. | Planned |
| D8 | Erase and drop are durable tasks: leased, retried, verified, audited. | Built |
| D9 | Bring data in: upload files in the console, load from S3 or a URL, connect to a database. | Under consideration |
| D10 | Search workspace catalogs by name, description, tag and owner, returning only securables the caller may see; descriptions and tags are editable with MODIFY. | Built |

### Compute
| id | requirement | status |
|---|---|---|
| C1 | Jobs run on machines that connect out; each run is in a gVisor sandbox. A workspace's admins attach its compute; compute belongs to one workspace. | Built |
| C2 | Serverless SQL on a provider endpoint; an admin starts it, it stops when idle. | Built |
| C3 | Sandboxes for agents: one per conversation thread, with its own workspace folder; data only through the query path. | Planned |
| C4 | Active runs are capped per organization; several servers respect one cap. | Built |
| C5 | SQL editor, saved queries and query history. | Built |
| C6 | A warm engine with all cores, results streamed as Arrow, files pruned before reading. | Planned (0001 tasks 2 to 5) |
| C7 | Jobs run on a schedule or after another run; a failed run can notify by email or webhook. | Under consideration |

### Storage
| id | requirement | status |
|---|---|---|
| S1 | Bytes live in an S3-compatible object store. A run gets short-lived, read-only credentials for its tables' folders only. | Built (R2) |
| S2 | Credentials, locations and one root per catalog, set per workspace; managed and external objects; paths never overlap. | Credentials and locations built, overlaps refused; a root per catalog in the workspace's prefix built (default store only); roots on a location, managed and external objects planned (0002 task 5) |
| S3 | Any store with STS (RustFS, S3); R2 through its own API. | Built on RustFS (0002 task 1); S3 untested |
| S4 | Default storage on RustFS: nothing to set up. | Built for development (0002 task 3); per-workspace prefixes planned (task 5) |
| S5 | Every secret lives in OpenBao; the catalog keeps references only. | Built for the server's own secrets (0002 task 6); production OpenBao (HA, audit) planned |
| S6 | Data is readable without the Loamz server: a version hint per table, continuous backups of the catalog and of sign-in (Keycloak), break-glass access for owners. | Version hint, base backups with a restore check and break-glass (development) built; continuous archiving and production planned (0002 task 6) |

### Clients
| id | requirement | status |
|---|---|---|
| K1 | Web console: catalog, SQL editor, history, compute, settings. | Built |
| K2 | Versioned public API (`/api/v1`), OpenAPI document, RFC 9457 errors. | Built |
| K3 | Apps such as Loamz Agents connect like any client and act as a principal. | Built |
| K4 | Access tokens for people and service principals, for scripts and tools. | Built |
| K5 | A command-line tool and a Python SDK. | Built |
| K6 | A standard SQL connection for BI tools (Arrow Flight SQL, JDBC). | Under consideration |

### Hosting
| id | requirement | status |
|---|---|---|
| H1 | One open codebase runs as Loamz Cloud, self-hosted or hybrid; the core is complete in every mode. | Planned (after beta) |
| H2 | An organization's limits come from `Entitlements`. A self-hosted install uses limits its admin sets, with safety caps. | Planned (the active-run cap is built) |
| H3 | Usage events are written in the same transaction as the change and delivered at least once, counted once. | Planned (after beta) |
| H4 | The core never needs Loamz Cloud to serve authorized work; sign-up, plans and billing live outside it. | Planned |
| H5 | Abuse protection for a hosted service: invite-only until billing exists, prepaid credit or a spend cap (never billed after use without one), API rate limits, outbound traffic denied, per-organization compute caps, a way to suspend an organization (audited). | Under consideration |
| H7 | A free tier of Loamz Cloud, once there is funding to pay for it. Until then the cloud is paid only. | Under consideration |
| H6 | Safe upgrades: forward-only migrations, server and runners of neighbouring versions work together, an upgrade guide. | Under consideration |

### Project
| id | requirement | status |
|---|---|---|
| P1 | Dependency checks in CI, signed releases with an SBOM, a short threat model. | Built |
| P2 | A user documentation site: quickstart, API reference, tutorials. Private for now, at `data.loamz.com/docs`. | Built |

## 2. Quality

| quality | target |
|---|---|
| speed | every change on the query path is measured with the benchmark (0001 task 1); targets are set from its baseline |
| security | no machine holds the storage key; compute connects out; credentials are scoped to one object and expire in minutes |
| footprint | the server idles under 30 MB and is ready in under 200 ms (`scripts/budget.sh`) |
| portability | the same SQL on SQLite (tests) and Postgres |
| correctness | results equal plain Iceberg reads of the same snapshot |

## 3. Examples

How we know the key requirements work.

| id | given | when | then | status |
|---|---|---|---|---|
| G2 | Bob has no grant on catalog `sales` | he runs `SELECT * FROM sales.main.orders` | 404, the same answer as a missing table; the refusal is in the audit log | Built |
| G4 | Ana can read `sales`; the agent's app can read only `public` | the agent, acting for Ana, reads `sales.main.orders` | refused: it gets only what both can do | Built |
| D4 | two engines commit to `orders` from the same snapshot | both commit at once | one succeeds; the other gets 409 and retries on the new snapshot; no half-written table is ever visible | Built |
| S1 | a job reads `sales.main.orders` | it receives credentials | they open only that table's folder, read-only, until the run's time limit plus five minutes | Built |
| C4 | an organization has 32 active runs (the default cap) | it submits another | 429; a slot frees when a run ends | Built |
| S6 | the Loamz server is stopped | DuckDB reads `orders` straight from the bucket | the latest snapshot, found through the table's version hint | Built (0002 task 6) |
| H3 | the cloud usage service is down | a 20-minute run finishes | the run is unaffected; its usage event waits in the outbox and arrives once when the service returns | Planned (after beta) |

## 4. Not in scope

Our own table format, query engine or storage engine; distributed scans; streaming ingestion; a BI tool; multi-region. Each needs a
benchmark or a customer first.
