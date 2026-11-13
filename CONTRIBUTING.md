# Contributing

How to build, run, test and change Loamz Data. Read [ARCHITECTURE](ARCHITECTURE.md) first.

## Set up
Rust (the version in `rust-toolchain.toml`), Node 22 with pnpm, `uv`, and Docker.

```sh
docker compose -f deploy/dev/compose.yml up -d    # Keycloak (:8082) and Postgres (:55434)
deploy/dev/seed.py                                 # ana, bob, cyd, pia and the organization acme; passwords in .dev/accounts.json
cargo build --release -p loamz-data-server -p loamz-runner
scripts/dev.sh                                     # server :8020, workbench http://localhost:5180
```

Data is stored in Cloudflare R2: put `endpoint`, `key-id`, `secret` and `api-token` in `~/.config/loamz/r2/`. Serverless SQL needs
`~/.config/loamz/serverless-token`. Starting a serverless warehouse costs Modal time: stop it when done (`scripts/serverless-close.sh`).

## Test
- `make check`: format, Clippy, tests on SQLite, web types and tests. Run it before every commit.
- Postgres: `LOAMZ_TEST_POSTGRES_URL=postgres://data:data-dev-only@127.0.0.1:55434/data cargo test -p loamz-data-server -- --include-ignored`
  (it creates databases and does not remove them: use a throwaway instance).
- Sandbox: `scripts/fetch-runsc.sh` and `server/runner/rootfs/build.sh` once, then
  `cargo test -p loamz-runner --test gvisor -- --ignored --test-threads=1` with `LOAMZ_RUNSC` and `LOAMZ_ROOTFS` set.
- End to end without a browser: `scripts/compute-smoke.py`. In a browser: `web/scripts/e2e.sh`.
- Footprint: `scripts/budget.sh` (idle memory under 30 MB, ready under 200 ms).

## Write code like this
- Plain and readable: each module says what it does in its first line; named intermediate results; ordinary loops over clever chains;
  repeat yourself before you abstract.
- Strict types; no `unwrap` outside tests; errors are problem documents.
- SQL in the portable subset (TEXT ids, BIGINT numbers, `$1`, NOT NULL columns), never built by joining strings.
- No backward compatibility: delete old code instead of keeping it alive.
- Every change that touches the query path reruns the benchmark (RFC 001) and puts the numbers in the commit message.

## Propose a change
- Small changes: just a pull request.
- Anything that changes a boundary, a format, the API or the query path: an RFC in `docs/rfcs/` first (context, goals and non-goals,
  design, alternatives, risks, plan), agreed before code.
- Record each decision as one line in [decisions.md](decisions.md).
- Never commit secrets. Pushes and releases are done by the owner.
