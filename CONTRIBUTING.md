# Contributing

How to build, run, test and change Loamz Data. Read the [architecture overview](docs/architecture/overview.md) first.

## Set up
Rust (the version in `rust-toolchain.toml`), Node 22 with pnpm, `uv`, and Docker.

```sh
docker compose -f deploy/dev/compose.yml up -d    # Keycloak (:8082), Postgres (:55434), RustFS (:9000), OpenBao (:8200)
scripts/dev.sh                                     # server :8020, workbench http://localhost:5180; sign in as admin / admin
```

The first sign-in is `admin` (password `admin`, changed at once). It also opens OpenBao's UI (`http://127.0.0.1:8200`, method OIDC) and the RustFS console (`http://127.0.0.1:9001`, "Login with Loamz"): both are Keycloak clients (`deploy/dev/realm_dev.py`, `deploy/dev/openbao_sso.py`). OpenBao and RustFS share Keycloak's network in `compose.yml` so they reach it as `127.0.0.1:8082`, the address in its tokens. The fixed keys (`loamz-dev-only`) are for machines. The admin creates organizations and adds people in the console.
For test data instead, `deploy/dev/seed.py` makes ana, bob, cyd, pia and the organization acme (passwords in `.dev/accounts.json`,
which `deploy/dev/token.py` and the smoke tests use). `cargo build --release -p loamz-data-server -p loamz-runner` builds the release
binaries the smoke tests run.

Data is stored in the RustFS container, in the bucket `loamz-data`. Another S3-compatible store is only different settings, all
`LOAMZ_DATA_BUCKET_*`: `_ENDPOINT`, `_KEY_ID_REF`, `_SECRET_REF`, and how machines are given access: `_STS_ENDPOINT` (RustFS, S3;
optionally `_STS_REGION`, `_STS_ROLE_ARN`) or `_ISSUER_TOKEN_REF` (Cloudflare R2). Every secret is a reference into OpenBao
(`path#key`, for example `loamz-data/bucket#secret`), never a value or a file; `scripts/dev.sh` puts the dev ones into the OpenBao
container (dev mode, root token `loamz-dev-only`). The server logs in to it with `LOAMZ_DATA_OPENBAO_ADDR` and
`LOAMZ_DATA_OPENBAO_TOKEN_FILE`, or an AppRole (`_ROLE_ID` and `_SECRET_ID_FILE`). Serverless SQL (Modal) needs a store reachable
from the internet, such as R2, and `LOAMZ_DATA_SERVERLESS_TOKEN_REF`; starting a warehouse costs Modal time: stop it when done
(`scripts/serverless-close.sh`).

## Test
- `make check`: format, Clippy, tests on SQLite, web types and tests. Run it before every commit.
- Postgres: `LOAMZ_TEST_POSTGRES_URL=postgres://data:data-dev-only@127.0.0.1:55434/data cargo test -p loamz-data-server -- --include-ignored`
  (it creates databases and does not remove them: use a throwaway instance).
- Sandbox: `scripts/fetch-runsc.sh` and `server/runner/rootfs/build.sh` once, then
  `cargo test -p loamz-runner --test gvisor -- --ignored --test-threads=1` with `LOAMZ_RUNSC` and `LOAMZ_ROOTFS` set.
- End to end without a browser: `scripts/compute-smoke.py`. In a browser: `web/scripts/e2e.sh`.
- A store with STS: `LOAMZ_TEST_STS_ENDPOINT=http://127.0.0.1:9000 cargo test -p loamz-lake --test sts_live -- --include-ignored` (RustFS
  from the compose file, with the bucket `loamz-data` made: `scripts/dev.sh` does that).
- Footprint: `scripts/budget.sh` (idle memory under 30 MB, ready under 200 ms).

## Write code like this
- **Small files, grouped by subject.** Each feature has `model.rs` (pure types and rules), `service.rs` (use cases), `store.rs` (its
  SQL: the repository) and `api.rs` (HTTP). One file per thing; a folder per subject.
- **Each module's first line says what it is and why.** A reader should not need the rest of the file to know where to look.
- **Contracts are traits with one job.** A kind of thing (a compute provider, a credential vendor) is one implementation chosen by a
  registry (the strategy pattern), not a `match` on its kind spread through the code.
- **Decisions are pure.** `authorize`, `sql::analyze` and the audit hash take facts and return answers: no I/O, no clock.
- **One change, one transaction.** A change and its audit entry go through `Uow`.
- **Plain and readable:** named intermediate results; ordinary loops over clever chains; repeat yourself before you abstract.
- **Strict types:** newtypes for ids, no `unwrap` outside tests; errors are problem documents.
- **SQL** in the portable subset (TEXT ids, BIGINT numbers, `$1`, NOT NULL columns), never built by joining strings.
- **No backward compatibility:** delete old code instead of keeping it alive.
- **Measure:** every change on the query path reruns the benchmark (proposal 0001) and puts the numbers in the commit message.

## Propose a change
- Small changes: just a pull request.
- Anything that changes a boundary, a format, the API or the query path: a proposal in `docs/proposals/` first (context, goals and non-goals,
  design, alternatives, risks, plan), agreed before code.
- Record each decision as one line in [decisions](docs/architecture/decisions.md).
- Never commit secrets. Pushes and releases are done by the owner.
