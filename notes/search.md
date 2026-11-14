# Workspace catalog search

## Goal and done-when

Let a workspace member find catalogs, schemas, tables, volumes, and views by name, description, tag, or owner, without learning that hidden securables exist. Add editable description and tag metadata to each securable.

Done when:

- `PUT /api/v1/workspaces/{ws}/securables/{id}/metadata` replaces description and tags, accepts only the specified limits, requires `MODIFY`, and commits the metadata change with its audit entry.
- `GET /api/v1/workspaces/{ws}/search` requires at least one filter and returns only objects accepted by the existing visibility predicate. Results are ordered by `(name, id)` after visibility filtering, capped by the requested limit, and expose no totals, offsets, `has_more`, or cursor.
- Candidate work is bounded to 100 rows per batch and 20 batches per request; no full-table load is used.
- SQLite and Postgres tests cover metadata, permissions, filters, escaping, non-disclosure, and result ordering; scan bounds are explicit in the implementation.
- The Catalog screen searches the workspace and displays visible results by path. Vitest coverage is added; e2e coverage is added only if the existing setup can exercise it without new infrastructure.
- OpenAPI and docs-site API documentation are updated; `docs-site/check_api.py` passes.
- The hostile-review outcomes and any unresolved limitations are recorded here. `scripts/heavy.sh make check` and the Postgres suite pass. Commits are on `task/search`; nothing is merged.

## Decisions

- Metadata is one description and one JSON-encoded tag array per securable. Store `description` as nullable `TEXT` and `tags` as `TEXT NOT NULL DEFAULT '[]'`; decode/encode tags with the existing JSON library, not database-specific JSON functions.
- The metadata PUT is a replacement: the body contains both `description` (string or null) and `tags` (array). Null clears the description; an empty tag array clears tags.
- Description is at most 2,000 Unicode characters. Tags are nonempty strings of at most 32 characters containing only lowercase ASCII letters, digits, `-`, and `_`; allow at most 20 unique tags. Reject duplicates rather than silently changing the caller's input.
- `q` is a case-insensitive substring over name, description, tags, and owner. Escape `%`, `_`, and the escape character before building a `LIKE` parameter. `tag` and `owner` filters are exact; `kind` is one of catalog, schema, table, volume, or view.
- The database's SQLite `lower()` is ASCII-only, so keep Rust-lowercased `description_folded` and `owner_folded` columns in sync on create, metadata update and ownership transfer; name and tags are constrained to lowercase ASCII already.
- At least one of `q`, `kind`, `tag`, or `owner` is required. An empty `q` is allowed only with another filter. Reject `q` longer than 2,000 characters, an invalid kind/tag, and limits outside 1–100.
- `limit` defaults to 25 and is applied only after visibility filtering. Candidate SQL batches contain at most 100 records; stop after 20 batches. If the scan budget is reached, return only the visible matches already found without signaling that more candidates existed.
- Sort the visible hits by `(name, id)`; hidden candidates cannot change the relative order of returned visible hits. Do not return total counts, offsets, `has_more`, or a cursor.
- Search results include a `path` for the console, built from the candidate's ancestor chain only after the candidate passes visibility. The existing tree model allows a caller with a grant inside a container to see that container for navigation.
- Use `catalogs::service::may_view` as the single visibility predicate. Fetch candidates in SQL and check each in bounded batches; do not copy IAM rules into a second implementation.
- Timing is out of scope and may vary with the number of candidate batches. This limitation is not reflected in response fields.
- Successful metadata updates and their `securable.metadata.set` audit entries use the existing `Uow` transaction pattern. A failed authorization or validation does not modify metadata.
- The catalog screen's current tree filter searches only loaded child nodes. Reuse its existing `TreePane` filter as the server-backed query and render results by path; do not change the shared design system.
- No migration compatibility path is needed: the current schema is still edited in `server/server/migrations/0001_init.sql`.

## Attack list

1. A search scoped to another workspace or organization returns a matching object.
2. A `dropping` securable, or one under a missing/dropping parent, appears.
3. A leaf granted directly is hidden even though the existing `may_view` rule allows it, or a new search-only visibility rule accidentally allows more.
4. An unviewable candidate is returned by name, description, tags, owner, id, or path.
5. Hidden candidates change the relative ordering of visible results.
6. Hidden candidates affect the externally returned count, page boundary, total, `has_more`, or cursor.
7. More than 100 candidates are loaded at once, or more than 20 batches are processed.
8. A cap at 20 batches creates a response field or ordering that reveals where hidden candidates occur.
9. A filter for a hidden tag/owner distinguishes hidden matches through a different error or response shape.
10. `%`, `_`, or `\` in `q` broadens a `LIKE` match rather than matching literally.
11. `_` in an exact tag filter matches a different tag because SQL treats it as a wildcard.
12. A quoted substring or another JSON array element fakes an exact tag match.
13. SQL is injected through `q`, `kind`, `tag`, `owner`, or `limit`.
14. SQLite and Postgres disagree on case-insensitive matching, escaping, `LIKE`, or ordering.
15. Invalid kinds, uppercase/non-ASCII tags, forbidden tag characters, duplicate tags, or more than 20 tags are accepted.
16. The 32-character tag or 2,000-character description boundary is bypassed with Unicode or byte-count confusion.
17. Malformed stored JSON tags turn into an empty/success-shaped result instead of a visible server error.
18. Metadata is changed without `MODIFY`, across workspace boundaries, or on an unseen id without the required 404.
19. The metadata row changes but the audit entry does not, or an audit entry commits when the update fails.
20. A visible leaf's path includes an ancestor the caller cannot see.
21. A stale asynchronous UI search overwrites newer results or results from a different workspace.
22. Search result names, descriptions, tags, owners, or paths are rendered as HTML.
23. A caller infers hidden candidate density from response timing. Timing is explicitly out of scope.
24. Search at the 20-batch ceiling returns fewer than `limit` without a paging hint; document this bounded-incompleteness tradeoff.

## What must be tested by running

- That validation and JSON tag persistence behave identically on SQLite and Postgres, including empty/null description and exact tag matches.
- That the shared visibility check filters every kind without returning hidden candidates in any response field and preserves `(name, id)` order among visible results.
- That query escaping works on both databases for `%`, `_`, and backslash, and tag matching treats underscore literally.
- That a metadata update and audit entry are atomic in both database backends.
- That the route table, OpenAPI spec, and docs-site API page stay in sync.
- That a Catalog-screen search request handles rapid query changes and renders the returned path without exposing hidden rows.
- Whether the existing browser e2e flow can cover this feature without requiring additional services or test infrastructure.

## Hostile review record

| # | Attack | Result and evidence | Fix |
|---|---|---|---|
| 1 | Cross-organization/workspace search | Held by both `org_id` and `workspace_id` predicates in `search_candidates`; no dedicated cross-workspace API fixture. | None. |
| 2 | Dropping object or inactive/missing ancestor | Held by the SQL `state = 'active'` filter and `target_in`; `search_returns_only_visible_matches_in_visible_order` covers hidden/missing visibility behavior, not a concurrent drop. | None. |
| 3 | Incorrect leaf/container visibility | Held by the shared `may_view`; the same search test proves a granted descendant makes its schema visible while an ungranted catalog stays absent. | Replaced `contains_any`'s in-memory child enumeration with a bounded-depth recursive existence query, preserving the shared predicate without loading all children. |
| 4 | Hidden name, description, tag, owner, id, or path in output | Held by only constructing result items after `may_view`; `search_returns_only_visible_matches_in_visible_order` checks the hidden id is absent and a hidden-only query has an empty `items` array. | None. |
| 5 | Hidden candidates affect visible relative order | Held by `ORDER BY name, id` and preserving candidate order; the visible `alpha`, `zulu` results remain ordered around hidden `middle_hidden` in `search_returns_only_visible_matches_in_visible_order`. | None. |
| 6 | Hidden count or paging metadata | Held: response contains only `items`; asserted by `search_returns_only_visible_matches_in_visible_order`. | No totals, offsets, cursors or `has_more`. |
| 7 | Unbounded rows per batch or scan | Held by `LIMIT 100`, keyset pagination, and a 20-page loop in `search`; inspection only (no query-count instrumentation test). | None. |
| 8 | Scan cap reveals the hidden boundary | Held by returning only visible items and no paging hints; inspected the 20-page early stop. More than 2,000 SQL-matching candidates can make results incomplete. | Documented this cap; no attempt to hide timing. |
| 9 | Hidden tag/owner filter gives a distinct result shape | Held by a single `items` response shape and exact bound filters; hidden-only query behavior is covered in `search_returns_only_visible_matches_in_visible_order`. | None. |
| 10 | `%`, `_`, or `\` broadens `q` | Held by literal escaping; `search_filters_are_exact_and_like_wildcards_are_literals` tests all three. | None. |
| 11 | `_` in a tag acts as a wildcard | Held by escaped exact JSON-string matching; the same filter test distinguishes `finance_x` from `finance-x`. | None. |
| 12 | JSON punctuation or another tag creates a false exact hit | Held by matching the full quoted JSON tag token; exact tag cases in `search_filters_are_exact_and_like_wildcards_are_literals`. | None. |
| 13 | SQL injection through filters | Held by bound values, filter allow-lists and literal escaping; the filter test sends an SQL-looking `q` and expects no matches. | None. |
| 14 | SQLite/Postgres Unicode case behavior differs | Broke: SQLite `lower()` did not match `CAFÉ` with `café`. The failing `q=caf%C3%A9` assertion in `search_filters_are_exact_and_like_wildcards_are_literals` reproduced it. | Added Rust-folded description/owner columns and lowercased `q`; SQLite and Postgres API tests pass. |
| 15 | Invalid kinds/tags or duplicate/too many tags | Held by validation; `securable_metadata_is_validated_authorized_and_audited` covers uppercase, punctuation, duplicates, and count limits; search filter tests cover invalid kind/tag. | None. |
| 16 | Unicode/byte-count limit bypass | Held: description count uses Unicode scalar values; metadata API test accepts 2,000 `é` and rejects 2,001, and tag validation only accepts ASCII bytes. | None. |
| 17 | Damaged stored tag JSON silently succeeds | Held by explicit `Error::Internal` on JSON decode; inspection only (no test mutates persisted data). | None. |
| 18 | Update without MODIFY, across workspaces, or on unseen id | Held by `target_in` and `iam::require`; `securable_metadata_is_validated_authorized_and_audited` checks reader 403, contributor success, and unseen 404, including invalid input on an unseen id. No separate wrong-workspace fixture. | Moved validation after the target/permission check to preserve hidden-object 404 behavior. |
| 19 | Metadata update/audit split or no-op update audited | Held by one `Uow` and an active-row affected-row check; metadata test checks the audit action and invalid inputs. A concurrent delete race is not forced by a test. | A no-op due to a concurrent drop now returns 404 and the unit of work rolls back. |
| 20 | Visible leaf path reveals an unseen ancestor | Held by `target_in` and building the path only after `may_view`; search test asserts the returned full path for a visible leaf. | None. |
| 21 | A stale UI query overwrites current results or crosses workspaces | Initially untested. `web/e2e/search-smoke.mjs` holds the OLD response until NEW renders and verifies OLD does not replace it; result state is keyed by workspace and query. | Added keyed search state and the browser test. |
| 22 | Search result text renders as HTML | Held by React text rendering (`TreeRow` label); inspection only, no HTML injection path. | None. |
| 23 | Timing reveals candidate density | Not addressed by design; explicitly out of scope. | No timing padding or constant-time scan. |
| 24 | 20-page ceiling silently truncates matches | Held to the agreed bounded-work rule: return visible matches found, with no count or continuation hint. No test creates >2,000 candidates. | Documented the possible incompleteness; increase the cap only if measured usage requires it. |

Two other defects found during the hostile read were fixed: metadata bodies with a missing `description` had been treated as a clear operation (now required and tested), and the initial OpenAPI edit accidentally placed the owner request schema under the metadata operation (corrected and structurally checked). The existing `web test:browser` smoke currently times out on its pre-existing workspace-settings `Member` combobox before reaching Catalog; the focused `web/e2e/search-smoke.mjs` passes independently.

The existing browser e2e setup supports a fixture-backed focused test, so one was added. Final verification passed: `scripts/heavy.sh make check` and the full Postgres suite (including the search API cases). The Postgres command ended with `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s` for doc-tests; all preceding unit and integration test groups also reported zero failures. Timing leakage and the 2,000-candidate scan ceiling remain deliberate limitations.
