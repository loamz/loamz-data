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
