# Proposal 0002: Foundation

- **Status:** accepted, 13 Nov 2026. Phase 1 of the [roadmap](../product/roadmap.md).
- **Authors:** Ravikumar, Claude.

## Context

The documents describe the product's model: organizations, workspaces as projects, one principal abstraction, governance over who,
compute, data and storage, and storage set per workspace ([architecture](../architecture/overview.md)). Today's code differs:
catalogs belong to the organization and are bound to workspaces, there are no service principals, storage is one R2 bucket, secrets
are files, and a new checkout needs a seed script and a cloud account.

## Goals and non-goals

**Goals:** the code matches the documented model; a fresh checkout runs with nothing to configure; any S3-compatible store works;
no secret lives in a file.

**Non-goals:** speed work ([0001](0001-fast-data-plane.md)), AI assets (phase 3), production operations (phase 4).

## Design

Recorded as decisions ([decisions](../architecture/decisions.md), 2026-11-12 and 13) and described in
[governance](../design/governance.md) and [storage](../design/storage.md). No backward compatibility: old code is deleted.

## Alternatives considered

- **Catalogs shared across workspaces** (Databricks' bindings): a workspace is a project; teams and apps get access as principals instead.
- **A bucket per catalog:** replaced by locations and roots, so one bucket can serve many workspaces while access stays per workspace.
- **MinIO for local storage:** no longer maintained; RustFS instead.
- **Secrets encrypted inside Postgres:** one service fewer, but break-glass access would need a master key kept offline; OpenBao instead.
- **Keep the seed script:** a first login with a default admin is what every self-hosted product does.

## Risks

- **The model change touches every service.** One pass, no backward compatibility, every test on SQLite and Postgres.
- **RustFS's STS is young** (it rejects session policies with an `Id`). Test it first; R2's own API stays.
- **OpenBao is one more service to run.** Development runs it in `compose.yml` in dev mode, like Keycloak; a highly available vault comes in phase 4.
- **Creating organizations from our console needs Keycloak's admin API.** A narrowly scoped admin client, audited.

## Dependencies

Keycloak's admin API (task 2), RustFS with STS (tasks 1, 3), OpenBao (task 6). Unity Catalog's storage model is the reference for task 5.

## Decided

- **Default storage:** one RustFS bucket per installation, with a prefix per workspace. Roots keep the paths apart.
- **How a service principal signs in:** with tokens we issue (not a Keycloak client each); the same tokens serve scripts (K4).
- **Who can create a workspace:** organization admins only, to start.

## Plan

| # | Task | Done when |
|---|---|---|
| 1 | Any S3-compatible store: an `Issuer` trait with STS `AssumeRole` (scoped session policy, no `Id` element) beside R2's own API. Test on RustFS (the default store) | a run reads its tables on RustFS with credentials limited to their folders |
| 2 | First login: a default admin in the realm; the account console can create organizations and add people (Keycloak admin API behind ours). Getting started is then compose, `scripts/dev.sh`, and sign in as `admin` (password `admin`, changed on first login); `seed.py` stays as test data only | a new checkout reaches a signed-in workspace with no seed script |
| 3 | Local storage for getting started: RustFS in `compose.yml`, and `scripts/dev.sh` points the server at it. Production changes only the storage settings | a fresh checkout stores and reads a table with no cloud account |
| 4 | Workspace model (decisions of 2026-11-12 and 13): catalogs owned by one workspace, service principals, compute owned by the workspace with a per-catalog read policy. Delete catalog bindings | the schema and the API match the README picture |
| 5 | Storage model (decisions of 2026-11-13): credentials, locations and roots per workspace; managed and external objects; no overlap; storage grants; default storage | a catalog on default storage and one on a bound location both work, and overlapping paths are refused |
| 6 | Secrets and recovery: a `SecretStore` contract with one implementation, OpenBao's HTTP API (KV v2), run in `compose.yml` for development too; references instead of secret files, Iceberg version hint on every commit, continuous backup of Postgres and of Keycloak's database, break-glass access | the server starts with no secret files, a table is read with DuckDB straight from the bucket while the server is stopped, and a restored Keycloak lets people sign in |

## Progress

| task | state |
|---|---|
| 1 Any S3-compatible store | done 13 Nov: `Issuer` trait, `StsIssuer` tested on RustFS (`sts_live`), `scripts/compute-smoke.py` runs a query and a job on it |
| 2 First login | done 13 Nov: `platform-admin`, `POST /api/v1/orgs` and `/people` through a `Directory` (Keycloak adapter, a fake in tests), the console's New organization and People; tried against a real Keycloak. Production still needs the role and directory roles in the `loamz-deploy` realm template |
| 3 Local storage | done 13 Nov: RustFS in `compose.yml`; `scripts/dev.sh` makes the bucket |
| Also | One sign-in for the tools: OpenBao and the RustFS console sign in through Keycloak in development, and the production realm template and settings are still to do (done 13 Nov) |
| 4 Workspace model | in progress: done 13 Nov: catalogs belong to one workspace (migration `0005`, bindings removed from API and console) and service principals with tokens we issue (migration `0006`, `workspaces/principals.rs`, a Settings tab). Not done: compute owned by the workspace with a per-catalog read policy, and one `Principal` type in the code |
| 5 Storage model | slice 1 done 13 Nov: credentials (references into the vault, checked against it) and locations per workspace, no two overlapping in any workspace (migration `0007`, `storage/locations`, a Settings tab). Not done: a root per catalog and the physical layout, managed and external objects, storage grants, default storage per workspace, one store per location |
| 5 (slice 2) | done 13 Nov: every table and volume has a `folder`; a catalog's root is `<workspace id>/<catalog id>` and its objects are `<root>/<id>`; names are unique per workspace; no old layout is kept (no users), and the migrations of this proposal are folded into `0001`, so two workspaces can each have `sales`. Still on the one configured store: roots on a registered location, a store per location, managed versus external objects, storage grants |
| 6 | task 6: done 13 Nov: the version hint on every commit (read by DuckDB with the server stopped) and the secret store (`SecretStore`, OpenBao over KV v2, token and AppRole login, every `*_FILE` replaced by a `*_REF`). Not done: continuous backups of Postgres and Keycloak, break-glass, production OpenBao. Task 5 not started |
