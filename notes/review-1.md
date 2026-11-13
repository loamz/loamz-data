# Hostile review: policies, storage locations, and import

## Phase 1: think

Goal: find concrete ways to bypass row filters or column masks, escape a managed
location, or make `loamz import` expose, corrupt, or unexpectedly execute data.
Done means every plausible attack below is classified against code and a test;
each real hole has a reproducing test that fails before its fix and passes
afterward. The focused Rust and Python tests, `nice -n 19 make check`, and the
documented Postgres suite must pass. Changes are limited to these paths, their
direct dependencies, tests, and this note.

The items below are attack hypotheses, not conclusions. Their result, test and
fix will be recorded after checking the implementation.

### A. Row filters and column masks

| Attack | Result | Test name | Fix |
|---|---|---|---|
| Qualify a masked column as `schema.table.col` after the table is wrapped. | To verify | — | — |
| Qualify it as `catalog.schema.table.col` after wrapping. | To verify | — | — |
| Alias a table to the name of a CTE and reference the CTE name. | To verify | — | — |
| Apply a mask to a column absent from the table schema. | To verify | — | — |
| Read two same-named tables in different schemas and confuse their policies. | To verify | — | — |
| Read a protected table through a view or nested query. | To verify | — | — |
| Put a protected table in one arm of a `UNION` or `UNION ALL`. | To verify | — | — |
| Use `SELECT * EXCLUDE` / `EXCEPT` / `REPLACE` to expose or transform a masked column. | To verify | — | — |
| Make a row-filter expression error only for selected rows. | To verify | — | — |
| Vary identifier case or quote identifiers such as `"Orders"` and `ORDERS`. | To verify | — | — |
| Read the same table twice using different aliases. | To verify | — | — |
| Put a file/network-reading DuckDB function into a policy expression. | To verify | — | — |
| Exploit a function missing from the allow-list, using DuckDB 2.0's actual functions. | To verify | — | — |
| Use a timing or error side channel to infer a masked value. | To verify | — | — |
| Lose group membership after queueing but before execution. | To verify | — | — |
| Submit a job whose declared tables differ from the tables the SQL actually reads. | To verify | — | — |
| Use a nested/subquery or parser edge case to hide a table from authorization. | To verify | — | — |
| Cause filter/mask rewriting to change an identifier or literal into executable SQL. | To verify | — | — |

### B. Catalogs on storage locations

| Attack | Result | Test name | Fix |
|---|---|---|---|
| Use `..` path segments to put a catalog outside its location root. | To verify | — | — |
| Use percent-encoded traversal or separators. | To verify | — | — |
| Use `_` or `%` in a location name to widen a `LIKE` prefix match. | To verify | — | — |
| Use a trailing slash to make equivalent roots compare differently. | To verify | — | — |
| Use Unicode normalization variants to collide or evade overlap checks. | To verify | — | — |
| Vary path or bucket case to evade equality/overlap checks. | To verify | — | — |
| Create overlapping catalogs through two workspaces. | To verify | — | — |
| Delete a location while a catalog is concurrently being created on it. | To verify | — | — |
| Reassign or edit a location after a catalog has captured its prefix. | To verify | — | — |
| Delete a location whose catalog is missed by `catalogs_under`. | To verify | — | — |
| Construct a prefix that makes SQL `LIKE` escape handling misidentify descendants. | To verify | — | — |
| Use an unregistered or foreign credential/location to select an unintended store. | To verify | — | — |
| Create two catalog ids or names that resolve to the same physical prefix. | To verify | — | — |
| Supply an empty or root prefix and claim more storage than intended. | To verify | — | — |
| Race two creates so the overlap check passes for both. | To verify | — | — |
| Exploit an error path that deletes metadata or files before refusing the request. | To verify | — | — |

### C. `loamz import`

| Attack | Result | Test name | Fix |
|---|---|---|---|
| Inject SQL through a file path used in the DuckDB read expression. | To verify | — | — |
| Inject SQL through a table name. | To verify | — | — |
| Inject SQL through a schema name. | To verify | — | — |
| Use quotes, semicolons or comments in an identifier to run extra statements. | To verify | — | — |
| Use an absolute path, `..`, symlink or glob to read unintended local files. | To verify | — | — |
| Use a URL or DuckDB table function to read the network instead of a local file. | To verify | — | — |
| Put credentials in argv and expose them through the process list. | To verify | — | — |
| Put credentials or sensitive file paths in logs or surfaced exceptions. | To verify | — | — |
| Append to an existing table without permission to modify it. | To verify | — | — |
| Replace or overwrite an existing table unexpectedly. | To verify | — | — |
| Import into a catalog/schema different from the one the user selected. | To verify | — | — |
| Let inferred names or types produce a different or dangerous target than requested. | To verify | — | — |
| Leave a partial table or file after a failed import. | To verify | — | — |
| Follow a symlink that changes between validation and read. | To verify | — | — |
| Accept an unsupported or malformed file but report success. | To verify | — | — |
| Send secrets to a remote endpoint via DuckDB extension/autoload behavior. | To verify | — | — |
| Let a path beginning with an option-like prefix alter CLI argument parsing. | To verify | — | — |
| Reuse an existing table name or schema without surfacing the conflict. | To verify | — | — |

### Decisions most likely to be wrong

1. **SQL policy enforcement:** prefer fail-closed parsing and authorization of
   every resolved base table; reject unsupported or ambiguous query shapes.
   Reject textual substitution or an allow-list based only on the last table
   name because aliases, CTEs and qualified columns can bypass it.
2. **Managed-prefix containment:** prefer canonical component-aware containment
   and database-level serialization/constraints for conflicting create/delete
   operations. Reject raw string-prefix or unescaped `LIKE` checks because
   separators, wildcards and concurrent writes can invalidate them.
3. **Import safety:** prefer fixed local-file readers and properly quoted
   identifiers/values, with the server remaining the authority for target
   permissions. Reject interpolated SQL, arbitrary DuckDB functions, and
   append/overwrite defaults because they can execute unintended work or
   mutate data beyond the user's grant.

### Must be tested by running

- Whether the installed DuckDB 2.0 exposes file or network readers that can be
  invoked from policy expressions, beyond the apparent function allow-list.
- Whether the SQL parser/rewrite preserves qualified references and aliases
  after wrapping protected tables, including `UNION`, views and projection
  modifiers.
- Whether SQLite and Postgres agree on prefix matching, `LIKE` escaping and
  concurrent catalog/location operations.
- Whether `loamz import` sends untrusted strings as SQL, leaks them through
  process arguments/logs, or leaves partial data after a failure.
- Whether group and catalog permissions are rechecked at execution rather than
  only at submission.

## Review results

DuckDB 2.0's [blob-function documentation](https://duckdb.org/docs/2.0/sql/functions/blob.html)
documents `read_blob`, and its [direct-file-reading guide](https://duckdb.org/docs/2.0/guides/file_formats/read_file.html)
documents `read_blob` and `read_text` as file readers. They are not safe query
functions for policy-protected data. The local DuckDB Python package is not
installed, so I verified their documented behavior and tested the server's AST
boundary rather than running a local DuckDB engine.

### Phase 2: test-first evidence

Before the fixes, the focused tests failed for the expected reasons:

- `compute::sql::tests::reading_files_is_not_reading_tables` accepted
  `SELECT read_blob(...) FROM a.b.c`.
- `rules_apply_to_everyone_who_does_not_manage_the_table::sqlite` returned
  `202` and queued the SQL with the `read_blob` call intact.
- `test_duckdb_attach_escapes_catalog_endpoint_and_token` observed unescaped
  single quotes in the generated `ATTACH` SQL.

The policy test `governance::catalogs::policy::tests::a_rule_can_never_read_or_run_anything_else`
also now names `read_blob`; the policy allow-list already rejects it.

### Attack outcomes

The initial tables are the phase-one hypotheses. These outcomes record the
code path and the closest focused test. "Held (static)" means the parser or
authorization structure prevents the attack, but the SDK's optional DuckDB
runtime was unavailable for end-to-end execution.

#### A. Row filters and column masks

| Attack | Result | Test/evidence | Fix |
|---|---|---|---|
| `schema.table.col` after wrapping | Held (static); the base relation becomes a derived table, so its original qualification cannot access unwrapped rows. | `a_table_with_rules_is_read_only_through_them`; runtime not exercised. | None. |
| `catalog.schema.table.col` after wrapping | Held (static), same derived-table boundary. | `a_table_with_rules_is_read_only_through_them`; runtime not exercised. | None. |
| Table alias collides with a CTE name | Held; each parsed base relation is authorized and each table factor is independently wrapped. | `every_way_to_name_the_table_is_covered`, `a_name_from_with_is_not_a_table_but_the_tables_inside_it_are`. | None. |
| Mask names a missing column | Held by DuckDB's `SELECT * REPLACE` missing-column error; no result is produced. | `a_table_with_rules_is_read_only_through_them` covers rewrite shape; no DuckDB runtime test. | None; runtime remains unverified. |
| Same last table name in different schemas | Held; authorization keys are full lower-cased catalog/schema/table tuples. | `finds_every_table_a_query_reads`. | None. |
| View or nested query reads a protected table | Nested query held; views are not accepted as tables by the catalog lookup. | `a_name_from_with_is_not_a_table_but_the_tables_inside_it_are`; view rejection reviewed in `readable_table`. | None. |
| Protected table in one `UNION` arm | Held by AST traversal of all relations and per-factor wrapping. | `union_tables_with_the_same_name_get_their_own_rules`. | None. |
| `* EXCLUDE` / `EXCEPT` / `REPLACE` exposes a masked value | Held by applying the mask inside the derived table before the caller's projection. | `duckdb_syntax_is_understood`, `a_table_with_rules_is_read_only_through_them`; runtime not exercised. | None. |
| Filter errors on selected rows | Query fails rather than returning rows from before the filter. | Static rewrite boundary; no DuckDB runtime test. | None; error/timing side channels are not eliminated. |
| Identifier case and quoting | Unquoted case is normalized; quoted identifiers retain parser values. | `finds_every_table_a_query_reads`, `every_way_to_name_the_table_is_covered`. | None. |
| Read same table under two aliases | Held; every table factor is wrapped; relation deduplication is only for policy lookup. | `every_way_to_name_the_table_is_covered`. | None. |
| File/network reader in a policy | Held; policy expression allow-list refuses non-approved functions. | `a_rule_can_never_read_or_run_anything_else`. | None. |
| DuckDB file-reading scalar function in user SQL | **Broke; fixed.** `read_blob` and `read_text` were callable in a query with an authorized table. | `reading_files_is_not_reading_tables`; API assertion in `rules_apply_to_everyone_who_does_not_manage_the_table`. | Refuse both AST function calls, including qualified/case-varied names; table functions were already refused. |
| Timing/error side channel for masks | Not fully addressed; failures can still differ based on a caller's expression. | No direct test. | No general side-channel mitigation added; scope is masked query results, not constant-time execution. |
| Group removed after SQL is queued | **Broke; fixed.** Lease used the saved group snapshot. | `rules_apply_to_everyone_who_does_not_manage_the_table`. | Refresh groups at lease and regenerate the guarded statement. |
| Group removed after SQL completes | **Broke; fixed.** Results could outlive the policy context that produced them. | `completed_sql_results_are_hidden_after_group_revocation`. | Re-evaluate the current guarded statement before returning details; otherwise hide results. |
| Job queued before row policy is added | **Broke; fixed.** Raw table files could be handed to the job. | `a_policy_added_after_job_submission_blocks_raw_table_input`. | Check table restriction at lease before vending credentials. |
| Nested/parser edge hides a base table | Held by parser traversal and rejection of non-full table names. | `a_name_from_with_is_not_a_table_but_the_tables_inside_it_are`, `tables_must_be_named_in_full`. | None. |
| Policy text rewrites into executable SQL | Held; expressions are parsed, allow-listed, substituted as AST values, and applied only after validation. | `plain_rules_are_saved`, `a_rule_can_never_read_or_run_anything_else`, `words_about_the_person_become_values`. | None. |

#### B. Catalogs on storage locations

| Attack | Result | Test/evidence | Fix |
|---|---|---|---|
| `..` path segments | Held; prefix segments reject `.` and `..`. | `prefixes_are_plain_folders`. | None. |
| Percent-encoded traversal/separators | Held; `%` and separators other than `/` are not valid segment characters. | `prefixes_are_plain_folders` now includes `%2e%2e`. | None. |
| `_` or `%` widens the catalog lookup | Held; `catalogs_under` escapes `LIKE` metacharacters. | `location_like_wildcards_do_not_match_other_catalog_prefixes` covers `_`; `%` is rejected in stored prefixes. | None. |
| Trailing slash creates an alternate root | Held; `normalize_prefix` trims leading/trailing slashes before storage. | `prefixes_are_plain_folders`. | None. |
| Unicode normalization collision | Held; non-ASCII prefix characters are rejected. | `prefixes_are_plain_folders`; static. | None. |
| Case variation escapes containment | Bucket names are lowercase-only; object prefixes are case-sensitive, matching object-store paths. | `buckets_are_named_as_s3_does`, `locations_overlap_when_one_is_inside_the_other`. | None. |
| Overlap across workspaces | Held in a single transaction; all locations for endpoint and bucket are checked, not just the current workspace. | `a_catalog_keeps_its_data_where_its_location_says`; cross-workspace overlap not separately tested. | None. |
| Delete races catalog creation | **Residual integrity risk, not reproduced.** The delete pre-check and catalog location lookup happen before their write transactions; process-local serialization does not coordinate multiple server instances. | No concurrent Postgres regression test. | Not fixed; needs a database-wide serialization/constraint strategy. |
| Location changes after catalog creation | Held; locations have no edit route; catalogs retain their resolved folder. | Route inventory and `a_catalog_keeps_its_data_where_its_location_says`. | None. |
| Delete misses a descendant catalog | Held; `catalogs_under` uses an escaped, separator-bounded prefix. | `a_catalog_keeps_its_data_where_its_location_says`, `location_like_wildcards_do_not_match_other_catalog_prefixes`. | None. |
| `LIKE` escape handling | Held; backslash, `%`, and `_` are escaped and the query declares an escape character. | `location_like_wildcards_do_not_match_other_catalog_prefixes`; `%` prefix invalid. | None. |
| Foreign credential/location selects another store | Held for catalog placement; credential must belong to workspace and catalog locations must resolve to the server's own writable store. | `a_catalog_keeps_its_data_where_its_location_says`; route tests verify workspace denial. | None. |
| Two location records resolve to same prefix | Held by overlap check for identical endpoint/bucket/prefix. | `locations_overlap_when_one_is_inside_the_other`; no concurrent test. | None. |
| Empty/root prefix claims whole bucket | Location may represent a bucket root, but `managed_prefix` refuses it for catalogs; own shared bucket roots are also refused. | `a_catalog_keeps_its_data_where_its_location_says`; root cases static. | None. |
| Concurrent overlapping location creates | **Residual integrity risk, not reproduced.** The in-process lock does not serialize writes from separate server instances. | No concurrent Postgres regression test. | Not fixed; same database-wide serialization gap as delete/create. |
| Failure deletes metadata or bytes before refusal | Held; create/delete metadata changes and audit are transactional; no bytes are deleted by location deletion. | `a_change_that_fails_leaves_no_record`, `a_catalog_keeps_its_data_where_its_location_says`. | None. |

#### C. `loamz import`

| Attack | Result | Test/evidence | Fix |
|---|---|---|---|
| SQL injection through file path | Held; path is a SQL string literal. | `test_import_escapes_file_path_and_target_identifiers`. | None. |
| SQL injection through table name | Held; target components use quoted identifiers. | `test_import_escapes_file_path_and_target_identifiers`. | None. |
| SQL injection through schema name | Held; target components use quoted identifiers. | `test_import_escapes_file_path_and_target_identifiers`. | None. |
| Quotes, semicolons or comments in identifiers | Held by doubling identifier quotes; punctuation remains inside identifier. | `test_import_escapes_file_path_and_target_identifiers`. | None. |
| Absolute path, `..`, symlink or glob reads unintended files | The importer reads the explicit local file the caller selected; `Path.is_file()` follows symlinks, so it is not a sandbox against the local process owner. | No separate test; this is a local SDK operation. | No local-file sandbox added. |
| URL/network read via import path | Held; `Path.is_file()` rejects URLs and reader choice is fixed to CSV/Parquet. | `test_import_escapes_file_path_and_target_identifiers`; static. | None. |
| Credentials exposed in process arguments | Held; attach uses a DuckDB SQL string in-process, not subprocess argv. | `test_duckdb_attach_escapes_catalog_endpoint_and_token`. | None. |
| Credentials or paths logged | No logging in importer; errors may include a local file path from Python/DuckDB. Token is not included in the raised messages by this code path. | Static review; no log-capture test. | None. |
| Append without modify permission | Server's Iceberg endpoint authorizes the write; SDK does not bypass server grants. | Existing integration `test_import_file_makes_a_table_and_adds_to_it`; server-side authorization reviewed. | None. |
| Unexpected overwrite | Held; non-append uses `CREATE TABLE`, which errors on an existing table rather than replacing it. | `test_import_file_makes_a_table_and_adds_to_it`; no overwrite-specific assertion. | None. |
| Import into a different catalog/schema | The selected `TableHandle` components determine the target; components are quoted. | `test_import_escapes_file_path_and_target_identifiers`; runtime unavailable. | None. |
| Inferred names/types change target | File schema determines columns only; destination is the fixed handle target. | Static review; no malformed-schema integration case. | None. |
| Failed import leaves partial table/data | Not independently verified against DuckDB transaction behavior. | No local DuckDB runtime installed. | Not changed; local runtime validation remains open. |
| Symlink changes between validation and read | Possible for a caller with write access to the same local filesystem; this is not a privilege boundary against the SDK caller. | No race test. | No path snapshot/copy added. |
| Malformed/unsupported file reports success | Fixed reader raises on parse failure; row count follows successful import. | Existing `test_import_file_makes_a_table_and_adds_to_it`; malformed-file case untested. | None. |
| Extension/autoload sends secrets remotely | The code explicitly installs/loads DuckDB extensions and passes the token to the Iceberg attach; extension supply-chain/network behavior was not runtime-tested. | No DuckDB runtime installed. | Not independently validated. |
| Option-like path changes CLI parsing | Path is passed as a quoted SQL string, not an argv option. | `test_import_escapes_file_path_and_target_identifiers`. | None. |
| Existing table/schema reused silently | `CREATE TABLE` errors if table exists; append is explicit. Schema creation is `IF NOT EXISTS`. | Existing integration `test_import_file_makes_a_table_and_adds_to_it`; conflict case not separately tested. | None. |

### Phase 4: hostile re-read

The stale group snapshot, stale result context, and job raw-file handoff were
the three concrete authorization gaps found on re-read. Their regressions are
`rules_apply_to_everyone_who_does_not_manage_the_table`,
`completed_sql_results_are_hidden_after_group_revocation`, and
`a_policy_added_after_job_submission_blocks_raw_table_input`. The location
create/delete race remains an unaddressed integrity risk across server
instances; local SQLite concurrency and the Postgres test suite are still
needed before treating the review as complete.

### Validation status

- `sdk/.venv/bin/python -m pytest -q sdk/tests/test_import.py`: 2 passed.
- `rustfmt --check` on the changed Rust files and `git diff --check`: passed.
- Rust tests, `nice -n 19 make check`, and the Postgres suite were not run.
  The requested `pgrep -c cc1plus` gate stayed at 3 because the remaining
  `cc1plus` entries were zombies; I did not run Cargo while that count was
  nonzero.
