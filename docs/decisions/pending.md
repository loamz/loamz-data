# Decisions taken alone, to review

Each is the smallest reversible choice made while the owner was away. Change any freely. (Replaces the first list, written before real
sign-in, workspaces and compute existed.)

## Identity and access

1. **An organization's identity here is its Keycloak alias.** Keycloak never lets an alias change, and the token's `organization` claim carries aliases only (ids need a custom mapper, and the built-in one cannot be renamed). If aliases ever need to change, add a mapper that writes ids and move to them.
2. **Account admins live in Data**, not in Keycloak. The Keycloak role `account-admin` only lets someone *create* an organization's Data account (and become its first admin); after that admins are managed in the account console. An account always keeps one admin.
3. **A person's own space is a real organization** (`personal:<subject>`), made on first sign-in, with a workspace called Personal.
4. **Privileges are Unity Catalog's** (`USE_CATALOG`, `SELECT`, `MODIFY`, `READ_VOLUME`, …). Reader, Contributor and Owner are bundles; `USE` privileges are placed on the catalog and schema above the thing granted, as Unity Catalog does.
5. **Hide, don't forbid.** Missing and forbidden look the same (404), including for SQL tables. The tenant's admin doors answer 403 to members.
6. **An agent acting for a person gets the intersection of both rights**, and an admin's agent is not an admin.
7. **Groups come from the token** (`groups`, full paths). A run keeps the groups the person had when they asked; group changes mid-run are not seen by that run.
8. **Workspace entitlements**: `sql-access` to run SQL, `compute-create` to run jobs, `sandbox-access` reserved. Workspace admins and account admins have all of them.
9. **A catalog bound to no workspace shows in all of them**; bound to some, only there.
10. **Audit keeps ids** (never content) and survives an erase by design; one chain per organization; a change and its entry are one transaction.

## Sites and sign-in

11. **A workspace is a website of its own** (`w-<id>.<base>`), and nothing in it links out except Sign out. The workspace switcher jumps to another workspace's site. Say if even that should go.
12. **The server does the Keycloak token exchange** (`/api/v1/auth/token`, two grants only, always as client `loamz-data`) because Keycloak cannot allow wildcard origins. Chosen over opening its token endpoint to every origin.
13. **`/signout` on the account site can be triggered by any link** (it ends the whole session). Low risk; a confirmation step would close it.

## Compute

14. **Compute belongs to the organization and is usable from every workspace of it.** No per-workspace binding yet (catalog-style bindings would be the same pattern).
15. **One run at a time per agent**; a run is queued for a target picked when it is submitted (the first online one big enough), and fails if its agent stops answering for 90 seconds or nobody takes it for 10 minutes. Not retried: a job may have side effects.
16. **A job's token lasts the run plus five minutes**, is deleted when the run ends, and is checked against the person's rights on every file call, so taking a grant away stops a running job at its next file.
17. **A table is an Iceberg table** (owner, 2026-10-31; replaces CSV/Parquet folders). This server is its REST catalog; DuckDB 2.0 reads and writes it.
18. **Without root, gVisor cannot enforce memory and process limits**, and can map only the agent's own user into the sandbox (as an unprivileged root of an empty namespace). The agent warns at start and the check verifies the user that will really run. Verified as root on Oracle (1.02 of 2 CPUs used; 2.5 GB killed at the 2 GB limit).
19. **The default sandbox platform is `systrap`;** `ptrace` is needed on WSL2.
20. **Result rows are capped** (10,000 rows, about 3.5 MB); a statement says when it was cut.
21. **Data bytes live in the R2 bucket `loamz-data`**, one prefix per securable id. The server alone holds the bucket key; a remote machine is given credentials issued for minutes, for one prefix, read-only unless it writes (`loamz-lake::grants`, proven against the real bucket).
22. **Serverless compute** (a provider endpoint such as Modal) is a compute target of kind `serverless`; the server calls it over https. It takes SQL only, only over data classified `normal`, and is used when asked for or when no machine of the organization answers.
23. **The server holds two storage secrets**, as mounted files: the bucket's S3 key, and a Cloudflare token that can issue temporary credentials (today it covers all buckets of the account; narrow it if other buckets appear).
