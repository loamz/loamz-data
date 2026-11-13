# Handoff: work for other agents

Tasks that can be done in parallel by another coding agent (Codex, Antigravity, anyone) without colliding with the others: each owns its
own files. Give an agent one task, in its own worktree and branch:

```sh
git worktree add ../loamz-data-<task> -b task/<task>
cd ../loamz-data-<task> && docker compose -f deploy/dev/compose.yml up -d
```

Start every agent with this, then the task text:

> Read `AGENTS.md`, then do the task below. It is part of the spec in `docs/`. Work only in the files the task lists. When done, run the
> checks in `AGENTS.md`, update the docs named in the task, and reply with what you built, what you tested, and what you did not do.

**Who gets what.** Codex works out details from a goal, so give it the large, open tasks and let it design inside the interfaces
below: tasks 1 and 3. Antigravity does better with exact steps, so give it the tasks that are a list of steps, and use the
"Steps" under each (tasks 2, 4, 5, 6). Either can take any task; the split is only about how much guidance each needs.

| task | owns | needs | est. | give to |
|---|---|---|---|---|
| 1. Warm engine | `server/worker/` (new), `server/runner/src/` | Rust, DuckDB | large | Codex |
| 2. SDK and CLI | `sdk/` (new) | Python | medium | Antigravity |
| 3. Bring data in | `web/src/screens/Load*`, `server/server/src/storage/ingest/` (new) | Rust, React | medium | Codex |
| 4. Security hygiene | `.github/`, `deny.toml`, `docs/security/` | CI, supply chain | small | Antigravity |
| 5. User documentation | `docs-site/` (new) | writing, a static site | medium | Antigravity |
| 6. Production deployment | the `loamz-deploy` repository | Ansible, Keycloak, OpenBao | medium | Antigravity |

Tasks 1 to 3 touch the Rust server and may conflict only at `openapi.yaml` and the end of `0001_init.sql`: merge those by hand.

Each task gives the **objects and methods to build** as pseudo-code in the style the code already uses (a trait per strategy, plain
structs for data, one `service.rs` of use cases, no logic in `api.rs`). Names are the contract: change one only if you say why.

## 1. Warm engine (proposal 0001, tasks 2 and 5)

**Why:** `bench/README.md` shows every statement on a compute machine takes 1.5 to 2.3 s whatever the query: it starts a sandbox and
downloads the whole table first. A warm DuckDB engine with ranged reads should take this to well under a second.

**Files:** new crate `server/worker/` (library `loamz-worker`, used by `server/runner`); `server/runner/src/exec.rs` chooses the executor.

**Objects and methods:**

```rust
// worker/src/lib.rs
pub struct TenantId(String);                       // the organization id: one warm engine never serves two

pub struct TableRef {                              // one table a statement reads, as the lease describes it
    pub name: String,                              // "sales.raw.orders" (catalog.schema.table)
    pub uri: String,                               // "s3://bucket/<workspace>/<catalog>/<id>"
    pub metadata: String,                          // "metadata/00004-x.metadata.json"
}
pub struct Credentials { pub endpoint: String, pub bucket: String, pub key_id: String, pub secret: String,
                         pub session_token: String }       // scoped to the tables' folders, read only, minutes
pub struct Answer {
    pub columns: Vec<Column>,                      // Column { name, type_name }
    pub rows: Vec<Vec<serde_json::Value>>,         // capped at `limit`
    pub truncated: bool,
    pub stats: Stats,                              // plan_ms, run_ms, bytes_read, requests
}

pub struct EngineConfig { pub threads: usize, pub memory_limit_mb: u64, pub extension_dir: PathBuf }

/// One embedded DuckDB, kept running. It is locked down: no local files, no extension loading, S3 only through the secret it was given.
pub struct Engine { /* duckdb::Connection behind a Mutex, tenant, last_used */ }
impl Engine {
    pub fn start(tenant: TenantId, cfg: &EngineConfig) -> Result<Self>;       // LOAD iceberg, httpfs from extension_dir; SET threads, memory_limit
    fn lock_down(&self) -> Result<()>;             // SET enable_external_access=false where possible; allowed_directories=[]; lock_configuration=true
    pub fn attach(&self, tables: &[TableRef], creds: &Credentials) -> Result<()>;
        // CREATE OR REPLACE SECRET (TYPE s3, ... SESSION_TOKEN ..., URL_STYLE 'path'); then per table
        // CREATE OR REPLACE VIEW "sales"."raw"."orders" AS SELECT * FROM iceberg_scan('<uri>/<metadata>')  (create schemas first)
    pub fn run(&self, sql: &str, limit: usize, deadline: Duration) -> Result<Answer>;  // interrupt() at the deadline
    pub fn idle_for(&self) -> Duration;
}

/// The warm engines, one per tenant, started on first use and dropped when idle.
pub struct Pool { /* HashMap<TenantId, Arc<Engine>>, cfg, max_idle */ }
impl Pool {
    pub fn new(cfg: EngineConfig, max_idle: Duration) -> Self;
    pub fn engine_for(&self, tenant: &TenantId) -> Result<Arc<Engine>>;
    pub fn reap_idle(&self);                       // called every minute
}

// runner/src/exec.rs: the strategy that picks how a run is executed
#[async_trait]
pub trait Executor { async fn execute(&self, lease: &Lease, stop: StopSignal) -> Report; }
pub struct SandboxExecutor { /* today's gVisor path: jobs, and anything the warm path refuses */ }
pub struct WarmExecutor { pool: Pool }             // statements only
// choose: run.kind == "sql" -> WarmExecutor; "job" -> SandboxExecutor
```

**Flow of a statement:** lease (server has authorized, vended credentials, listed `tables`) → `WarmExecutor::execute` →
`pool.engine_for(tenant)` → `engine.attach(tables, creds)` → `engine.run(sql, limit, deadline)` → `Report { state, result, ... }`.
Never attach a table the lease did not list. A statement that fails lock-down checks is failed, not retried in the sandbox.

**Done when:** `bench/run.py 3` shows the `machine` column under 0.5 s warm for the five queries, with the same answers (the script
checks row counts; add an answer check); `scripts/compute-smoke.py` still passes; lock-down has tests (a statement that reads
`/etc/passwd`, loads an extension, or reaches another bucket is refused); `bench/README.md` has the new table beside the old one.

**Docs:** proposal 0001 tasks 2 and 5, `docs/design/compute.md`, `bench/README.md`.

## 2. SDK and CLI (requirements K4, K5)

**Why:** scripts and tools need to talk to Loamz Data without the console.

**Files:** `sdk/pyproject.toml`, `sdk/src/loamz/{client,models,errors,sql,duck,cli}.py`, `sdk/tests/`.

**Objects and methods:**

```python
# errors.py
class LoamzError(Exception):                       # RFC 9457 problem+json
    status: int; title: str; detail: str

# client.py
class Client:
    def __init__(self, base_url: str, *, token: str | None = None, token_file: str | None = None): ...
        # token from the argument, then LOAMZ_TOKEN, then the file; never a command-line argument; sent as "Authorization: Bearer"
    def me(self) -> Me: ...                                            # GET /api/v1/me
    def workspaces(self) -> list[Workspace]: ...
    def workspace(self, name_or_id: str) -> "WorkspaceHandle": ...

class WorkspaceHandle:
    id: str
    def catalogs(self) -> list[Securable]: ...                         # GET .../securables?parent_id=
    def tables(self, catalog: str, schema: str) -> list[Securable]: ...
    def sql(self, statement: str, *, limit: int = 1000, wait_s: int = 30, target: str | None = None) -> Rows: ...
        # POST .../sql/statements; 202 means keep asking GET .../runs/{id} until the state is final
    def table(self, full_name: str) -> "TableHandle": ...             # "catalog.schema.table"

class Rows:
    columns: list[str]
    rows: list[list]
    def to_arrow(self): ...                        # needs pyarrow
    def to_pandas(self): ...                       # needs pandas

class TableHandle:
    def duckdb(self):                              # a duckdb connection with the table attached through the Iceberg REST endpoint
        ...                                        # ATTACH '<catalog>' AS lz (TYPE iceberg, ENDPOINT '<base>/api/v1/workspaces/<id>/iceberg', TOKEN '<token>')

# cli.py   (entry point `loamz`)
#   loamz ls [catalog[.schema]]       loamz sql "<statement>" [--limit N] [--json]       loamz whoami
```

**Done when:** `pytest` in `sdk/` passes against the dev stack (tests start nothing: they need `scripts/dev.sh` running and
`scripts/sample_data.py` loaded), and the README shows the three examples above.

**Docs:** requirements K4, K5 status; `README.md` (one line).

### Steps (task 2, in order)

1. `pyproject.toml`: name `loamz`, Python 3.10+, dependencies `httpx`; optional extras `arrow = ["pyarrow"]`, `pandas = ["pandas"]`, `duckdb = ["duckdb>=2"]`; script `loamz = loamz.cli:main`.
2. `errors.py`: `LoamzError.from_response(r)` reads the JSON body (`title`, `detail`, `status`); falls back to the HTTP status text.
3. `client.py` `Client._send(method, path, json=None)`: one `httpx.Client` with the bearer header; raise `LoamzError` for any status >= 400; return parsed JSON or `None` for 204.
4. `Client.me`, `workspaces` (from `me()["workspaces"]`), `workspace(name_or_id)` (match id, else name; raise `LoamzError` 404-like if none).
5. `WorkspaceHandle.sql`: POST `/api/v1/workspaces/{id}/sql/statements` with `{"sql", "limit", "wait_s", "target"}`.
   If the status is 202: loop `GET /runs/{id}` every 0.5 s (cap the total at 10 minutes) until `state` is `succeeded`, `failed` or `cancelled`.
   If not `succeeded`: raise `LoamzError` with the run's `error`. Else return `Rows(columns=[c["name"] for c in result["columns"]], rows=result["rows"])`.
6. `Rows.to_arrow`: import `pyarrow` lazily; on `ImportError` say `pip install loamz[arrow]`.
7. `TableHandle.duckdb`: import `duckdb` lazily; connect; `INSTALL iceberg; INSTALL httpfs; LOAD ...`; `ATTACH` the catalog name with the endpoint and token; return the connection.
8. `cli.py`: `argparse` with sub-commands `whoami`, `ls`, `sql`; base URL from `LOAMZ_URL` (default `http://127.0.0.1:8020`); print a table, or JSON with `--json`.
9. Tests: against the running dev stack; `ana`'s token comes from `deploy/dev/token.py ana` (skip the tests with a clear message if the stack is down).

## 3. Bring data in (requirement D9)

**Why:** nobody can try the product without data in it. Today only `scripts/sample_data.py` loads any.

**Files:** server `server/server/src/storage/ingest/{mod,model,service,api}.rs` (a new feature after `tables` in the layering order:
add it to `tests/layering.rs`); console `web/src/screens/LoadData.tsx`, `web/src/api.ts`.

**Objects and methods:**

```rust
// model.rs  (pure)
pub enum Format { Csv, Parquet }
pub struct Column { pub name: String, pub iceberg_type: String, pub required: bool }
pub struct CsvOptions { pub header: bool, pub delimiter: u8 }
pub fn format_of(file_name: &str, content_type: &str) -> Option<Format>;
pub fn valid_table_name(n: &str) -> bool;                          // reuse catalogs::model::valid_name
pub fn columns_from_duckdb(described: &[(String, String)]) -> Result<Vec<Column>, String>;   // DuckDB type -> Iceberg type

pub struct LoadRequest { pub schema_id: String, pub table: String, pub format: Format, pub csv: CsvOptions }
pub struct Loaded { pub table_id: String, pub rows: i64, pub bytes: u64, pub columns: Vec<Column> }

// service.rs  (use cases)
pub async fn load_upload(app: &App, ask: &Ask<'_>, req: LoadRequest, body: Body) -> Result<Loaded>;
    // 1 iam::require(CREATE_TABLE) on the schema (tables::create_under does this)        2 stream body to a temp file, cap at app.max_upload
    // 3 DuckDB (in a worker, not in the request thread): DESCRIBE read_csv/read_parquet -> columns
    // 4 tables::create_under(schema_id, NewTable{ name, columns }) -> folder + credentials
    // 5 write the rows as Parquet into the table's data/ folder and commit them through tables::commit (one append snapshot)
    // 6 audit "table.load" with the table id (never the file name's content); delete the temp file on every path
pub async fn load_url(app: &App, ask: &Ask<'_>, req: LoadRequest, url: &str) -> Result<Loaded>;
    // https only, no credentials in the URL, no private addresses (SSRF check in model.rs: refuse loopback, link-local, RFC 1918)
```

**API:** `POST /api/v1/workspaces/{ws}/securables/{schema}/load?table=<name>&format=csv|parquet` (the file is the body, streamed) and
`POST …/load-url` with `{"table": "...", "url": "https://..."}`. Errors: 404 no schema or no `CREATE TABLE`, 400 bad name or format,
413 too large, 409 the table exists.

**Done when:** tests load a CSV and a Parquet file and read them back by SQL; every refusal above has a test; a file that fails
half-way leaves no table; the OpenAPI document is updated; the console action works against the sample workspace.

**Docs:** requirement D9, `docs/design/data.md`.

## 4. Security hygiene (requirement P1)

**Files:** `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `deny.toml`, `docs/security/threat-model.md`.

**Structure:**

```yaml
# ci.yml   on: push, pull_request
jobs:
  rust:      # fmt --check, clippy -D warnings, cargo test (SQLite)
  postgres:  # services: postgres:17; LOAMZ_TEST_POSTGRES_URL; cargo test -- --include-ignored --skip openbao_live
  web:       # pnpm install --frozen-lockfile; tsc --noEmit; vitest; pnpm audit --prod
  supply:    # cargo deny check; cargo audit
# release.yml  on: tag v*   -> build release binaries, CycloneDX SBOM (cargo cyclonedx), attach both
```

```toml
# deny.toml
[advisories]  # vulnerability = deny, yanked = deny
[licenses]    # allow = ["Apache-2.0","MIT","BSD-2-Clause","BSD-3-Clause","ISC","MPL-2.0","Unicode-3.0","Zlib"]
[bans]        # multiple-versions = "warn"
```

**Threat model** (`docs/security/threat-model.md`): a table per trust boundary (browser to server, server to runner, runner to
sandbox, server to store, server to OpenBao, server to Keycloak, tool to tool through Keycloak) with columns: asset, who can attack,
what stops them (name the file or test), what does not yet (be honest: read `docs/architecture/platform.md` risks).

**Done when:** `actionlint` passes if available, `cargo deny check` passes locally, and every boundary in the architecture picture has a
row.

### Steps (task 4, in order)

1. `ci.yml`: `runs-on: ubuntu-24.04`; Rust job uses `dtolnay/rust-toolchain` with the version in `rust-toolchain.toml`, `Swatinem/rust-cache`; run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
2. `postgres` job: `services.postgres` image `postgres:17-alpine` with `POSTGRES_USER=data POSTGRES_PASSWORD=data-dev-only POSTGRES_DB=data`, `max_connections=400` (the tests open many); run the `--include-ignored --skip openbao_live` command from `AGENTS.md`.
3. `web` job: Node 22, `pnpm/action-setup`, `cd web && pnpm install --frozen-lockfile && pnpm exec tsc --noEmit && pnpm test && pnpm audit --prod`.
4. `supply` job: `EmbarkStudios/cargo-deny-action` and `rustsec/audit-check`.
5. `release.yml`: on tags `v*`; `cargo build --release`; `cargo install cargo-cyclonedx && cargo cyclonedx --format json`; upload binaries and the SBOM as release assets.
6. `deny.toml` as above; run `cargo deny check` locally and add a `[[licenses.exceptions]]` only with a comment saying why.
7. `threat-model.md`: write the table (one row per boundary listed above, at least three rows of "what stops them" each citing a real file or test), then a "Not yet" list taken from `docs/architecture/platform.md` section 6 and `docs/design/*.md` (planned items).

## 5. User documentation (requirement P2)

**Why:** every document in `docs/` is for people building Loamz Data. Users need their own, at `data.loamz.com/docs` (private for now).

**Files:** `docs-site/` (MkDocs with the Material theme, or any generator that outputs plain files).

**Pages:**

```
docs-site/docs/
  index.md                what Loamz Data is (two paragraphs)
  start/first-steps.md    sign in, a workspace, load data, run a query (running example: nyc.main.trips)
  console/{catalog,sql,compute,settings}.md
  connect/{duckdb,spark,trino,python}.md      through the Iceberg REST endpoint; vended credentials
  access/{people,service-principals,permissions}.md
  api/index.md            generated by docs-site/gen_api.py
docs-site/gen_api.py      def render(openapi: dict) -> dict[str, str]   # one section per tag/path; every path in the file appears
```

Plain, short, no internal words ("securable", "Data account"). **Done when:** one command builds it (in the README), links resolve, and
the API page lists every path of `specs/data/v1/openapi.yaml` (a script checks this).

### Steps (task 5, in order)

1. Choose MkDocs Material; `docs-site/mkdocs.yml` with the nav above; `docs-site/README.md` says `pip install -r requirements.txt && mkdocs build` (output in `docs-site/site/`, which is git-ignored).
2. Write the pages in this order: `start/first-steps.md` (it is the one people read), `connect/duckdb.md`, `console/*`, `access/*`, `connect/{spark,trino,python}.md`, `index.md`. Every code block is something you ran against the dev stack.
3. `gen_api.py`: `load openapi.yaml`; for each path and method write a heading `METHOD path`, the summary, the description, a parameters table, the request body schema, and the response codes; group by the first path segment after `/api/v1/`; write `docs/api/<group>.md` and an index listing all groups.
4. `check_api.py`: assert every `(method, path)` in the OpenAPI file appears in the generated pages; exit 1 listing the missing ones. Run it from `mkdocs.yml` hooks or the README's build command.
5. Read the pages once as a new user: remove any word from `docs/` that a user would not know.

## 6. Production deployment (the `loamz-deploy` repository)

**Files:** `roles/keycloak/templates/realm-loamz.json.j2`; new `roles/openbao/{tasks/main.yml,templates/openbao.hcl.j2,files/policies/*.hcl}`;
`roles/backup/{tasks/main.yml,templates/loamz-backup.{service,timer}.j2}`.

**Do:**

```text
realm template   + client role platform-admin on loamz-data
                 + directory client: realm-management roles manage-users, manage-realm (view-users, query-users, query-groups, view-realm stay)
                 + clients loamz-openbao (roles admin, break-glass) and loamz-rustfs (role consoleAdmin), secrets from the vault, not the template
                 (deploy/dev/realm_dev.py in loamz-data is the development version: copy its shape, not its passwords)
openbao role     three-node Raft; config file declares the audit device (OpenBao refuses it through the API); policies deny by default:
                   server-approle   read secret/data/loamz-data/*        break-glass   read secret/data/loamz-data/bucket only, 1 hour
                 AppRole for the server: LOAMZ_DATA_OPENBAO_ROLE_ID + LOAMZ_DATA_OPENBAO_SECRET_ID_FILE (a systemd credential)
                 OIDC login method against Keycloak, as deploy/dev/openbao_sso.py does
server settings  every secret by reference: LOAMZ_DATA_BUCKET_KEY_ID_REF, _SECRET_REF, LOAMZ_DATA_DIRECTORY_SECRET_REF,
                 LOAMZ_DATA_METRICS_TOKEN_REF, LOAMZ_DATA_OPENBAO_ADDR (see CONTRIBUTING.md)
backup role      timer: scripts/backup.sh daily; scripts/restore-check.sh weekly, which must fail loudly (alert) when it fails
```

**Done when:** a fresh deploy to a test host signs in as the first admin, creates an organization, and the server starts with no secret
file but its AppRole secret id; a backup restores; a break-glass sign-in reads the bucket credential and nothing else.

### Steps (task 6, in order)

1. In `loamz-deploy`, read how the existing `roles/keycloak` renders the realm and where secrets come from; copy that pattern.
2. Edit `realm-loamz.json.j2`: add the roles and clients listed above; client secrets are Jinja variables from the existing vault file, never literals.
3. New `roles/openbao`: install the package, `templates/openbao.hcl.j2` (listener, `storage "raft"`, the `audit "file" "stdout"` block), systemd unit, then `tasks` that initialize once, unseal from the recovery process the team already uses, and apply `files/policies/*.hcl`.
4. Tasks to enable the AppRole method, create the role `loamz-data` bound to the policy `server-approle`, write the role id to the server's environment file and the secret id as a systemd credential.
5. Tasks to enable the OIDC method against Keycloak and create the roles `default` (claim `roles` contains `admin`) and `break-glass` (contains `break-glass`, one hour), as `deploy/dev/openbao_sso.py` does.
6. New `roles/backup`: install `postgresql-client`, copy the two scripts from `loamz-data`, a daily timer for `backup.sh`, a weekly timer for `restore-check.sh` that sends a failure alert.
7. Run against a test host; write down in the role's README the commands that proved each "Done when" line.

## 7. Break the isolation (for Copilot; requirements G1 to G4)

Goal: prove, with tests that try to break it, that one organization, workspace or principal can never see or change another's data.
Only tests and the small fixes the tests force. Owns: `server/server/tests/adversarial.rs` (new) and whatever governance code a
failing test shows is wrong. Touches nothing in `compute/` warm-engine code or `storage/ingest`.

Think before you write anything. First list every way a caller could cross a boundary: every route in `specs/data/v1/openapi.yaml`
with an id in its path or body, every place an id from one org could be used under another, every token kind (`user`, `sp:`, run token).
Then write the attacks, run them, and fix what they find.

Done when: a table in `docs/design/governance.md` lists each attack, the test that covers it, and the result; `make check` and the
Postgres test command pass; no attack returns 403 where 404 is the rule.

## 8. Personal data: erase and prove the audit chain (for Copilot; requirement G7)

Goal: `DELETE` of a person's data removes their content everywhere but leaves the audit chain intact and verifiable, and a new
`GET /orgs/{org}/audit/verify` walks the chain and reports the first broken link, if any. Owns: `governance/audit` and
`governance/lifecycle`. Read `docs/design/governance.md` first.

Think before you write anything. Work out what "their data" means in this schema (rows naming them as owner, grantee, requester,
`on_behalf_of`; files in volumes; run results) and which must be deleted, rewritten or kept. Write that decision into the design doc
before the code.

Done when: tests show an erase leaves `verify` passing; tampering with one audit row makes `verify` name that row; the erase itself is
audited; `make check` and the Postgres test command pass.
