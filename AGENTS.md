# Instructions for coding agents

Loamz Data is a governed data platform in Rust (server, runner) with a React console. Read these first, in this order:

1. `CLAUDE.md`: what is where, and the rules the code keeps (written for Claude Code, true for anyone).
2. `CONTRIBUTING.md`: how to set up, test, and the code style (small files, `model.rs` pure, `service.rs`, `store.rs`, `api.rs`).
3. `docs/product/requirements.md` and `docs/proposals/`: what is built and what is planned. The requirement ids (G5, K4, …) are the
   spec; do not invent behavior the docs do not describe, and update them when you finish.

Rules that are easy to break:
- No backward compatibility and no stand-ins. Delete old code instead of keeping it working. Nothing in the product skips real sign-in.
- Every change that touches the API updates `specs/data/v1/openapi.yaml` (a test checks routes against it).
- The migration is one file, `server/server/migrations/0001_init.sql`, edited in place (nothing is deployed). If you need a table, add it
  at the end under a comment block of your own.
- A caller who may not see something gets 404, never 403. Every change and its audit entry are one transaction.
- Never print or commit a secret. Secrets live in OpenBao and are named by reference (`path#key`).
- Plain words in anything a user reads. The console never says "Data account"; say what the thing is.

Done means: `make check` passes; for database code also
`LOAMZ_TEST_POSTGRES_URL=postgres://data:data-dev-only@127.0.0.1:55434/data cargo test -p loamz-data-server -- --include-ignored --skip openbao_live`
(start the stack with `docker compose -f deploy/dev/compose.yml up -d`); the docs you touched are updated; and you say plainly what you did
not do. Do not commit to `main`: work on your own branch.

## Working with other agents on one machine

- Read `docs/proposals/0003-beta.md`: who owns what, and how a wave runs (write everything first, verify once).
- Every build or test goes through the shared lock: `scripts/heavy.sh make check`. Never run two heavy commands at once, never open another worktree's folder in the editor, and build the SQL worker only with `make worker`.
- One branch and one worktree per task: `git worktree add ../loamz-data-<task> -b task/<task> main`. Do not merge to `main`; the lead does.
- Report what you ran and its last lines. A claim without the output will be re-run by the lead.

