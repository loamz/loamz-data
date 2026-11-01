# loamz-data

The governed data platform of Loamz, in Rust. Design: `docs/charter.md`. Plan: `docs/plan.md`. Contract: `specs/data/v1/openapi.yaml`.

## Layout

- `server/lake` (`loamz-lake`): storage library. `sql.rs` is one database layer for SQLite and Postgres; `files.rs` is a folder per volume or table on a POSIX path; `objects.rs` is the `Store` the server uses: a bucket (Cloudflare R2/S3, the home of data so remote machines can query it; signed GET/PUT addresses for exactly one file) or a folder. Settings: `LOAMZ_DATA_BUCKET` + `_ENDPOINT` + `_KEY_ID_FILE` + `_SECRET_FILE`, or `LOAMZ_DATA_STORES_DIR`.
- `server/server` (`loamz-data-server`, binary `loamz-data`): the governed API.
  - `kernel/`: settings, the token check (OIDC only), errors (problem+json), the one `authz::authorize`, app state. Knows no feature.
  - `governance/{audit,orgs,workspaces,iam,catalogs,lifecycle}/` and `compute/`: each has `model.rs` (pure), `service.rs` (use cases), `store.rs` (SQL), `api.rs` (HTTP). `governance/context.rs` works out what a request is about (`OrgCtx`, `WsCtx`, `FileCtx`).
  - `storage/volumes/`: the files of volumes, reached only through governance.
  - `storage/tables/`: tables are **Iceberg only**. This server is their REST catalog (`/api/v1/workspaces/{ws}/iceberg/v1/…`; the warehouse is a catalog, a namespace a schema): it checks who may load or commit, writes the metadata file, moves the pointer (`securables.metadata`) in one transaction with its audit entry, and hands the client short-lived credentials for that table's folder only. DuckDB 2.0 creates, writes and reads through it.
  - `compute/`: compute targets (a machine that connects out, or a *serverless* provider endpoint the server calls), runs (SQL statements and jobs), the agent protocol, and the scoped tokens a run carries. `compute/sql.rs` parses a statement and lists the tables it reads.
- `server/runner` (`loamz-runner agent`): a compute agent. It connects out to a Data server, leases work, fetches only what the run's token allows, runs it in a gVisor sandbox (limits written into the sandbox's own OCI config and checked before it starts), and reports. `src/sandbox/run_sql.py` is what runs a statement inside the sandbox, with DuckDB locked down. `rootfs/build.sh` builds the sandbox filesystem (Python and DuckDB); `scripts/fetch-runsc.sh` fetches gVisor. Both keep their output in `~/.cache/loamz/`.
- `web/`: the web app (React, Tailwind, Radix), served as two kinds of site from one build (`web/src/site.ts`): the **account site** (`data.loamz.com`: product page, sign-in, Home, account console) and one **site per workspace** (`w-<id>.data.loamz.com`: overview, catalog, SQL editor, compute, settings; nothing links out, only Sign out). Sign-in on a workspace site comes back through the account site's `/relay`, and the server does the Keycloak token exchange (`/api/v1/auth/token`) because Keycloak cannot allow wildcard origins. Production needs wildcard DNS and TLS for `*.data.loamz.com`, and the server serving `web/dist` (not built yet).
  - `web/src/design/` is the shared Loamz design system, copied from loamz-chat by `web/scripts/sync-design.sh`: never edit it here. Rules in `web/src/design/DESIGN.md` (primitives, tokens, orange = action, violet = selected).
  - Screens in `web/src/screens`; the SQL editor and Compute are loaded on demand because they carry the code editor.
- `deploy/dev`: the local stack (Keycloak 26.8.1 and Postgres, in docker), with the realm rendered from `../loamz-deploy`'s template. `seed.py` makes people (ana is an account admin of the organization acme), `token.py` signs one in.
- `server/engine`: the Python `loamz-engine` package, kept because Agents builds against it (`uv build --package loamz-engine`).

- **Data lives in R2** (bucket `loamz-data`, one prefix per securable id); a folder store exists for tests only. Settings: `LOAMZ_DATA_BUCKET`, `_BUCKET_ENDPOINT`, `_BUCKET_KEY_ID_FILE`, `_BUCKET_SECRET_FILE`, `_BUCKET_ISSUER_TOKEN_FILE` (a Cloudflare token that can issue temporary credentials), `LOAMZ_DATA_SERVERLESS_TOKEN_FILE` and `LOAMZ_DATA_SERVERLESS_ENDPOINT` (the deployed Modal URL: with the token, the Compute page can create serverless SQL warehouses without typing it). Dev keeps them in `~/.config/loamz/r2/` and `~/.config/loamz/serverless-token`. Live test: `LOAMZ_TEST_R2_DIR=~/.config/loamz/r2 cargo test -p loamz-lake --test r2_live -- --include-ignored`.
- `server/serverless/modal_app.py`: the serverless SQL endpoint (Modal; `modal deploy`, secret `loamz-sql` holds the token; stops itself a minute after its last call; `scripts/serverless-close.sh` stops it now). Tools venv with DuckDB 2.0 and Modal: `~/.cache/loamz/modal-venv`. `scripts/iceberg_seed.py` makes and fills a table the way any DuckDB client does.

## Rules

- Services form one line: audit < orgs < workspaces < iam < catalogs < volumes < compute < lifecycle. A service uses only those before it. `server/server/tests/layering.rs` checks this, and that `model.rs` files are pure.
- Only `iam::service` decides who may do what, using `kernel::authz` (Unity Catalog privileges and ownership). Nothing else reads grants. A job's own token (`jt_…`) is the person's rights narrowed to what the run declared, checked in the same place (`iam::require`), and only the file routes (`FileCtx`) accept one.
- A caller who may not see a thing gets 404, never 403. Every refusal is audited. SQL never says whether a table exists: missing and forbidden are the same answer.
- Every change and its audit entry are one database transaction (`audit::service::Uow`); each organization has its own hash chain.
- Erase and drop are durable tasks (lease, retry, verify, audit). Never a best-effort call.
- A statement is parsed (`compute/sql.rs`): one plain query, every table named `catalog.schema.table` and authorized (`SELECT`) before it is queued. DuckDB inside the sandbox is a second, separate lock.
- Compute agents connect out; the server never connects to a machine. An agent holds only its own secret (shown once, kept as a hash). A serverless target is the one exception: the server calls the provider's https endpoint, which takes SQL only, only over data classified `normal`.
- A person's token issued to a client named in `LOAMZ_DATA_AGENT_CLIENTS` (the agents server's, reached only by token exchange) is that agent acting for the person: `act` is not needed, and the pair gets what both may do. Deploy: `loamz-agents` client in the realm template (standard token exchange on; local: `deploy/dev/agents_client.py`).
- A machine never holds the bucket's key: a run is given credentials for exactly its tables' folders, read-only, for its time limit plus five minutes (a statement has no job token at all; only a job's volume files go through the API with one).
- SQL is written once in the portable subset (TEXT ids, BIGINT numbers, `$1`, NOT NULL columns); every database test runs on SQLite and on Postgres.
- Errors are RFC 9457; the API is versioned (`/api/v1`); the OpenAPI document and the router must agree (a test checks it).
- No backward compatibility, and no stand-ins in the product: nothing that skips real sign-in, and no authorization from the environment. Delete old code instead of shimming.
- Never commit secrets. Pushes and releases are done by the owner.

## Commands

- `make check`: fmt, clippy, tests on SQLite, web types and tests, import contract. Postgres too: `docker compose -f deploy/dev/compose.yml up -d postgres`, then `LOAMZ_TEST_POSTGRES_URL=postgres://data:data-dev-only@127.0.0.1:55434/data cargo test --workspace -- --include-ignored`.
- The local stack: `docker compose -f deploy/dev/compose.yml up -d`, then `deploy/dev/seed.py` (passwords go to `.dev/accounts.json`, never printed).
- Sandbox pieces, once per machine: `scripts/fetch-runsc.sh` and `server/runner/rootfs/build.sh`. gVisor tests: `LOAMZ_PLATFORM=ptrace LOAMZ_RUNSC=~/.cache/loamz/runsc LOAMZ_ROOTFS=~/.cache/loamz/rootfs LOAMZ_ROOTLESS=1 cargo test -p loamz-runner --test gvisor` (`ptrace` on WSL2; drop it and run as root on a real server to test the memory and process limits).
- Web: `cd web && pnpm install && pnpm test && pnpm build`; `scripts/dev.sh` runs the server and the app; `web/scripts/e2e.sh` is the browser test (real sign-ins, a real agent; needs Python Playwright in `PLAYWRIGHT_PY`, the release builds of the server and the runner, and the sandbox pieces above).
- `scripts/compute-smoke.py`: a query and a job, end to end, without a browser. With `REMOTE=user@host KEY=…` the agent runs on that machine instead.
- `scripts/budget.sh`: idle memory under 30 MB, ready under 200 ms. `scripts/smoke.sh`: the governance path with `curl`.
- `cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.35 -p loamz-data-server -p loamz-runner`: the ARM builds.
