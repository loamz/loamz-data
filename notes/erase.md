# Erase and prove the audit chain

## Phase 1: think

### Goal and data inventory

Erase one `user:<subject>` in the organization named by the request. Preserve a minimal run/task history, but remove identity links and content that can expose the person. Keep the organization’s audit history verifiable. “Everywhere” means every Data record and byte in that one organization; it does not mean another organization, the identity provider, or a separately scoped `personal:<subject>` account.

The schema inventory below is from `server/server/migrations/0001_init.sql`. A “tombstone” is the stable, organization-scoped pseudonym `erased:<digest>`, not a usable principal.

| Table and person-bearing column(s) | Erase action |
|---|---|
| `orgs.created_by` | Rewrite a matching raw subject to the tombstone; retain the account and its audit scope. |
| `account_admins.principal` | Delete the assignment; a person must not retain admin rights. |
| `account_admins.added_by` | Tombstone a matching raw subject; the historical assignment record remains. |
| `workspaces.created_by` | Tombstone; keep the workspace unless its data is owned by the erased person. |
| `workspace_members.principal`, `entitlements.principal` | Delete matching memberships and capabilities. |
| `securables.owner` | Delete each matching owned securable, its descendants, grants and files. Ownership is functional, so a detached tombstone owner would be misleading. |
| `grants.principal` | Delete matching grants; `grants.granted_by` is tombstoned as attribution. |
| `audit.actor`, `audit.on_behalf_of` | Keep each audit row and hash. Write stable pseudonyms from the outset so erasing a person never requires rewriting or rehashing history. |
| `audit.target` | Keep resource labels and ids; pseudonymize embedded `user:<subject>` references before appending. The erase request itself must never put the raw subject in its target. |
| `tasks.subject` | Tombstone after the erase has used the original subject; retain task state and timestamps. |
| `tasks.requested_by`, `tasks.on_behalf_of` | Tombstone matching raw subjects; retain task attribution without retaining the identity. |
| `tasks.last_error` | Clear on successful completion; an error string is not a safe place to retain a subject or file path. |
| `compute_targets.created_by`, `service_principals.created_by`, `storage_credentials.created_by`, `storage_locations.created_by` | Tombstone matching raw subjects; keep independently owned resources. |
| `runs.requested_by`, `runs.on_behalf_of` | Tombstone matching raw subjects; keep the run id and timing/state for operational history. |
| `runs.title`, `runs.spec`, `runs.result`, `runs.stdout`, `runs.stderr`, `runs.error` | Clear content for matching runs. `spec.as` includes subject, name, email and groups; script, result, logs and errors may contain arbitrary personal data. Cancel an active run before clearing it. |
| `run_tokens.identity` and the rest of the token row | Delete tokens for matching runs; they contain a serialized identity and are credentials, not history. |
| Volume/table files | Delete all bytes in owned securables. For a run written to a shared output volume, delete only `runs/<run-id>/` files for the erased person’s runs; preserve other runs and unrelated files. |
| `orgs.id` and matching `audit.org_id` | Keep as organization/chain keys. A separate `personal:<subject>` account is outside the requested organization-scoped erase; rewriting an organization key would split or destroy its independently verified chain. |

`created_by`, `added_by`, `granted_by`, `requested_by`, and `on_behalf_of` contain raw identity subjects, while `principal` and `owner` contain typed values such as `user:<subject>`. Erasure must match both forms without widening a query beyond the requested organization. A group grant is the group’s data, not a direct person reference; no group membership list is stored here.

### Audit design

At audit append time, encode identity values as a deterministic, organization-scoped pseudonym and replace explicit person-principal references in `target` the same way. Hash the pseudonymized entry. This preserves existing audit row bytes and hashes through erase; `verify` continues to recompute the original chain. It also avoids cross-organization correlation from one subject’s pseudonym. The current code hashes raw actor fields, so tests must prove that new audit entries do not persist the subject.

Rejected: rewrite actor strings in old audit rows and recompute every later hash. That makes the chain verify only after changing its historical evidence and makes an erase operation indistinguishable from deliberate audit rewriting.

Queue insertion plus `erase.requested` is already one `Uow`. For completion, delete external files idempotently first, then scrub metadata, finish the task, and append `erase.done` in one database transaction. A crash before that transaction leaves a retryable task; a crash after commit leaves both the scrub and its completion evidence. Never record a completed erase separately from its final database changes.

### Failure modes to attack

1. **Owned catalog includes a subtree:** removing only the root leaves descendants or bytes. Check `lifecycle::remove_subtree`, `catalogs::owned_by/subtree`, and `volumes::erase_files`; assert owned files and rows are gone.
2. **Last account admin:** current code can delete owned content before `orgs::forget_principal` refuses. Preflight in `lifecycle::run`; assert no rows or files change when another admin has not been added.
3. **Last workspace admin:** erase must remove the person’s workspace assignment without leaving workspace access or bypassing the account-admin invariant. Check `workspaces::forget_principal`.
4. **Raw subject versus typed principal:** one representation is missed in creator or attribution columns. Check the erase SQL in `lifecycle::store` and every `created_by` column in `0001_init.sql`.
5. **A grant survives:** delete the grantee’s grants but preserve unrelated grants and tombstone matching `granted_by`. Check `iam::forget_principal` and its store.
6. **Run result survives:** rows, SQL/script, title, stdout, stderr or error remain after identity removal. Check `compute::store::runs` and the erase cleanup.
7. **Serialized run identity survives:** `run_tokens.identity` or `runs.spec.as` keeps the subject, name, email or groups. Check `compute::service::As` and `run_tokens`.
8. **Run output files survive:** files under `runs/<run-id>/` in a volume owned by someone else remain, or unrelated files are deleted. Check runner output layout and the `Store::list/delete` path.
9. **Erase races an active run:** a late heartbeat/completion writes results back after cleanup. Check `compute::service::complete`, `compute::store::finish`, token deletion and run state transitions.
10. **Task record leaks the target:** `tasks.subject`, requester, on-behalf identity or error text retains the erased subject. Check `lifecycle::run_once`, `lifecycle::store::finish/fail`.
11. **Audit leaks identity or breaks:** actor, on-behalf or target contains the raw subject, or rewriting a row invalidates later hashes. Check `audit::Who`, `Uow::audit`, `model::hash/first_broken`, and `/audit/verify`.
12. **Erase and completion audit split:** a crash commits the erase but not `erase.done`, or vice versa. Check the final transaction in `lifecycle::run_once`.
13. **Erase requested twice or retried:** duplicate work damages unrelated data or leaves an incomplete task. Check idempotence of `remove_subtree`, token/file deletion and final task state.
14. **Same person in another organization:** a global subject-only delete crosses the request boundary or pseudonyms correlate accounts. Check every cleanup query for `org_id` and the pseudonym’s organization scope.
15. **Concurrent grant/assignment:** a grant arrives after cleanup and survives. Check `iam::assign`, workspace/account assignment paths and active erase task handling.
16. **Caller lacks erase permission:** an outsider learns whether the org/person exists, or receives 403 where the requested hidden result is 404. Check `lifecycle::api`, `request_erase`, task lookup, audit verification and OpenAPI responses.
17. **Non-person principal is erased:** a `group:` or `sp:` value accidentally enters person erasure. Check `request_erase` validation; only `user:` is a person.
18. **SQLite/Postgres diverge:** unsupported SQL, row-lock assumptions or transaction behavior makes one backend partial. Check portable SQL in lifecycle/compute stores and run `both!` cases.
19. **Crash after file deletion:** a retry errors because bytes are already gone, or marks the task done too soon. Check idempotent volume removal and task leasing.
20. **Audit chain has a gap or tamper:** verification reports success or the wrong sequence. Check paged `audit::service::verify` and `first_broken`, including an entry changed in place.

### Endpoint and permissions

This checkout already has `POST /api/v1/orgs/{org}/erase` with `{ "subject": "user:<subject>" }`, returning `202` and a durable task id; `GET /api/v1/orgs/{org}/tasks/{id}` reports progress. Only account admins may request or inspect an erase. An out-of-scope org or principal is hidden as 404, never 403. The task’s subject is one person, not a group or service principal.

`GET /api/v1/orgs/{org}/audit/verify` was already routed, implemented and listed in OpenAPI. It is account-admin-only and returns integrity, entry count and first broken sequence.

## Phase 2: test-first

`server/server/tests/erase.rs` exercises owned subtrees and files, personal references, shared run outputs and tokens, late runner completion, last-admin protection, org scoping/idempotence, concurrent grants, hidden permissions, audit verification/tampering, and atomic request/completion audit. The pre-implementation SQLite run had 8 intended failures: owned data, last admin, person references, run tokens/results, late completion, grant race, hidden permissions and raw audit identity. The two passing cases confirmed existing repeat behavior and request auditing. Test-fixture name/borrow compile errors were corrected before this baseline was accepted.

## Phase 3: build

The existing lifecycle worker now preflights and locks the account-admin rows before external file deletion. It removes owned securables and run-scoped output files, then scrubs person references, clears run content and tokens, finishes the task and appends `erase.done` in one `Uow`. Audit identities and explicit `user:` targets are pseudonymized on append; old rows are never rewritten. Pending erases block new grants and workspace/account assignments. No migration was needed.

## Phase 4: hostile review and validation

Hostile review found and fixed three gaps: a last-admin check could race file deletion, a run output reference needed organization scoping and active-run file coverage, and task diagnostics outside the current task could retain the erased subject. Added/extended tests cover last-admin preservation, active run files/late completion and unrelated task errors. Audit target punctuation is also tested to avoid partial-subject leaks.

Validation: `make check` passed. The configured Postgres container at port 55434 was unhealthy and still syncing its pre-existing data directory, so the exact command could not connect. The full command from `AGENTS.md` passed against an isolated fresh Postgres 17 instance at port 55435; all SQLite and Postgres tests passed, including all 10 erase scenarios on both backends.
