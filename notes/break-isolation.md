# Break the isolation

## Goal and done

Prove through the HTTP router that a caller cannot cross organization, workspace, or principal boundaries, including with identifiers and scoped tokens. Done means each attack below has a test in `server/server/tests/adversarial.rs`, unauthorized cross-boundary requests return 404 rather than 403, protected rows/files remain unchanged, and any refusal that the code path audits has an audit entry. The coverage table goes in `docs/design/governance.md`; `make check` and the documented Postgres test command pass.

The worktree already has unrelated staged/uncommitted state (including edits in `tests/api.rs`, `tests/common/mod.rs`, compute/storage code, and an untracked `deploy/dev/openbao.hcl`). Preserve it; stage and commit only this task's files and necessary fixes on `task/break-isolation`.

## Attack inventory

1. **Read an Acme securable through Rival's workspace.** `governance/catalogs/service.rs::target_in` must scope lookup to both organization and workspace; response 404, no data disclosed.
2. **Delete an Acme securable through Rival's workspace.** `governance/catalogs/service.rs` and `governance/lifecycle` must not queue a drop or alter the row; response 404.
3. **Grant access using an Acme securable id in Rival.** `governance/iam/api.rs` and `catalogs::target_in` must reject before writing `grants`; response 404, no grant or success audit.
4. **Read or cancel an Acme run through Rival's workspace.** `compute/service.rs::get`/`cancel` and `compute/store.rs::run_get` must scope by workspace; response 404, run state unchanged.
5. **Operate on an Acme compute target through Rival.** `compute/service.rs`/`compute/store.rs` must scope secret, start/stop, and delete operations to the workspace; response 404, target unchanged.
6. **Delete an Acme storage credential through Rival, especially while it is in use.** `storage/locations/service.rs::delete_credential` must establish workspace ownership before checking global location use; response 404, no credential/location changes. The current unscoped `locations_using(id)` precheck may leak a 409.
7. **Create a Rival location using an Acme credential id.** `storage/locations/service.rs::create_location` and `store::get_credential` must return 404 without inserting a location.
8. **Delete an Acme location through Rival.** `storage/locations/service.rs::delete_location` and its workspace-scoped store delete must return 404 without changing either workspace.
9. **Rotate or delete an Acme service principal by id under Rival.** `governance/workspaces/principals.rs` and its store operations must return 404; the principal and token remain usable only in its original workspace.
10. **Use a service-principal (`lsp_…`) token in another workspace (same org and different org).** `governance/context.rs::Identity` plus `workspaces::open` must enforce `confined_to`; response 404 and no file/list access.
11. **Use a run token to read a volume not declared in the run's reads.** `FileCtx` and `iam::within_scope` must return 404 even if the submitting person can read it; no bytes disclosed and a refusal is audited on the `iam::require` path.
12. **Use a run token to write/delete a declared input or another volume instead of its declared output.** `FileCtx` and `iam::within_scope` must return 404, leave bytes unchanged, and audit the refusal where the gate records it.
13. **Reuse an expired or finished run token.** `compute/service.rs::identity_for_token` must reject both with 401 before any file operation; no bytes changed. (These are invalid credentials, not hidden resource ids.)
14. **Escape a volume with `..` or an encoded slash plus `..`.** `storage/volumes/api.rs` wildcard extraction and `loamz-lake::files::Volumes::components/resolve` must not normalize into another path; per the requested hidden-boundary result, return 404 and leave the outside file and volume unchanged.
15. **Name a compute target omitted from a catalog's allowed-compute policy.** `compute/service.rs::pick` and the lease-time `authorize_lease` check must refuse explicit target selection with 404, queue no run, and record a refusal where applicable.
16. **Create a location overlapping a different workspace's prefix.** `storage/locations/service.rs::create_location` and `store::prefixes_in_bucket` must refuse without writing; return 404 as requested, and do not disclose the other location. Also verify a non-overlapping sibling prefix still works.

## Three easy-to-get-wrong decisions

1. **Hidden resource status:** use 404 for every cross-boundary id or policy-hidden target, never 403. I reject preserving the current 400/409 for the policy/overlap attacks because the requested outcome explicitly requires 404; adjust existing expectations only if the new tests prove the response must change.
2. **Run-token scope:** reads are exactly the declared inputs and writes/deletes are exactly the declared output. I reject inheriting the person's broader grants or treating a write target as readable; the run token is narrower than its issuer.
3. **Path handling:** rely on the existing path-component validator after router extraction; reject traversal rather than cleaning or double-decoding it. I reject normalization because it can change which object an encoded path names.

## Must be learned by running

- How Axum's wildcard extractor presents raw `..` and `%2F` paths to the volume validator, and the final HTTP status/error conversion.
- Whether a foreign credential id currently leaks its use through the global `locations_using` check, and whether policy/overlap refusals have the requested status without partial writes.
- That the token checks and audit assertions behave the same on SQLite and Postgres, including after a run transitions to a terminal state.

## Hostile review follow-ups

1. A run token's declared output volume was writable at every path, while the runner writes only under `runs/{run_id}/`. Constrain writes and deletes to that run prefix; test that attempts to overwrite another run's file and write at the volume root are hidden and preserve data.
2. Explicit compute selection checked target availability before catalog policy, so a hidden but offline target could reveal itself as unavailable. Check policy first; test the hidden/offline case returns 404 and queues no run.
3. Foreign compute-secret rotation was not covered even though delete was. Add a router test proving rotation returns 404 and the target remains unavailable to the foreign workspace.

The additional cases run with the SQLite adversarial suite; the Postgres equivalents remain part of the final repository test command.

## Test-first evidence and final checks

Before the fixes, the router tests exposed the wrong boundary results: foreign credential deletion and cross-workspace location overlap returned 409, a policy-hidden compute target returned 400, invalid file paths returned 400, and a run token writing a declared input returned 403. These were changed to the hidden/unauthorized behavior described above.

Final validation passed:

- `make check` (format, clippy, workspace tests).
- `LOAMZ_TEST_POSTGRES_URL=… cargo test -p loamz-data-server -- --include-ignored --skip openbao_live`: all server tests passed, including 20 adversarial tests and the Postgres API and compute suites.
