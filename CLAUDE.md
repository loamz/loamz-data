# loamz-data

The governed data platform of Loamz, in Rust. Design: `docs/charter.md`. Plan: `docs/plan.md`. Contract: `specs/data/v1/openapi.yaml`.

## Layout

- `server/lake` (`loamz-lake`): storage library. `sql.rs` is one database layer for SQLite and Postgres; `files.rs` is volumes on a POSIX path.
- `server/server` (`loamz-data-server`, binary `loamz-data`): the governed API.
  - `kernel/`: settings, token check, errors (problem+json), the one `authz::authorize`, app state. Knows no feature.
  - `governance/{audit,iam,catalogs,lifecycle}/`: each has `model.rs` (pure), `service.rs` (use cases), `store.rs` (SQL), `api.rs` (HTTP).
  - `storage/volumes/`: files in a volume, reached only through governance.
- `server/runner` (`loamz-runner`): the sandbox runner (`loamz-runner run-job` runs one job on a compute machine, reaching Data only through its API).
- `web/`: the web app (React, Tailwind, Radix). `web/src/design/` is the shared Loamz design system, copied from loamz-chat by `web/scripts/sync-design.sh`: never edit it here. Screens in `web/src/screens`, one file per screen; see `web/src/design/DESIGN.md` for the rules (primitives, tokens, orange = action, violet = selected).
- `server/engine`: the Python `loamz-engine` package, kept because Agents builds against it (`uv build --package loamz-engine`).

## Rules

- Services form one line: audit < iam < catalogs < volumes < lifecycle. A service uses only those below it. `tests/layering.rs` checks this.
- Only `iam::service` decides who may do what, using `kernel::authz`. Nothing else reads grants.
- A caller who may not see a thing gets 404, never 403. Every refusal is audited.
- Erase and drop are durable tasks (lease, retry, verify, audit). Never a best-effort call.
- SQL is written once in the portable subset (TEXT ids, BIGINT numbers, `$1`, NOT NULL columns); every database test runs on SQLite and on Postgres.
- Errors are RFC 9457; the API is versioned (`/api/v1`); the OpenAPI document and the router must agree (a test checks it).
- No backward compatibility: delete old code instead of shimming.
- Builds never write tracked files. Never commit secrets. Pushes and releases are done by the owner.

## Commands

- `make check`: fmt, clippy, tests on SQLite. For Postgres: `LOAMZ_TEST_POSTGRES_URL=postgres://... cargo test -- --include-ignored`.
- Web app: `cd web && pnpm install && pnpm test && pnpm build`; `scripts/dev-ui.sh` starts a server with sample data, then `pnpm dev` and open the address with `#token=<token>`; `web/scripts/e2e.sh` drives it in a browser (needs Python Playwright, `PLAYWRIGHT_PY`).
- `scripts/smoke.sh`: start the release binary and walk the main path. `scripts/budget.sh`: idle memory under 30 MB, ready under 200 ms.
- `cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.35 -p loamz-data-server`: the ARM server build.
