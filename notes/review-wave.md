# W13 hostile security review

## Goal and done

Try to break the security-sensitive changes merged or written on `main` from
2026-11-12 through 2026-11-14. For every concrete attack, add a regression test
that demonstrates the pre-fix behavior, record whether it was held or broke,
and fix confirmed defects in small commits. Done means the findings and
uncertainties are recorded here, `scripts/heavy.sh make check` and the required
Postgres suite pass, and the branch remains unmerged.

Production review scope is limited to the four areas in the request:

1. `server/runner/src/fetch.rs` and URL imports.
2. `server/server/src/kernel/places.rs` and storage locations.
3. Arrow result export, runner upload, result links, and result expiry.
4. `server/runner/src/door.rs`.

The security-review agent completed a bounded read-only review before changes.
Fail-first tests were added and run before each confirmed production fix.

## Decisions

1. Test at the trust boundary named by the attack: unit tests for pure URL/path
   decisions, integration tests for permission and lifecycle behavior, and
   runner tests for door protocol parsing. Rejected: infer safety from helper
   names or source inspection alone.
2. Treat a confirmed leak or cross-scope access as a defect even if it is
   difficult to reproduce through the UI. Rejected: defer issues that require
   a direct API or protocol client.
3. Keep production changes limited to proven defects. Preserve existing
   contracts and update docs only where behavior changes. Rejected: broad
   refactors during a security round.

## Attack/test matrix

| Area | Attacks to test |
|---|---|
| URL import | Redirects to loopback/private/link-local addresses; IPv4-mapped IPv6, NAT64 and 6to4; DNS rebinding between resolution and connect; URL userinfo and unusual ports; oversized and slow bodies; private addresses or credentials appearing in run views, audit, logs or errors. |
| Customer locations | Catalog roots on a customer location; credential rotation/deletion during use; cross-workspace access through `loc-` folder markers or guessed folder names; error messages exposing credential material or vault references. |
| Large results | A caller without current run-detail rights requesting a result link; links after run erasure; result objects surviving erase/expiry; upload attempts outside the one authorized result key. |
| Runner doors | Conflicting request framing / request smuggling; encoded separators, dot segments, alternate path encodings, wrong methods, wrong workspace/catalog, and storage-key escapes. |

For each agent-confirmed candidate, add the narrowest test that fails before
any fix; record its exact name and result below. Include both SQLite and
Postgres for database-backed behavior.

## Initial agent candidates

The bounded review identified three likely defects. Fail-first tests were run
before production changes; further path and IPv6 probes found two additional
defects and a late-upload cleanup race.

| Attack | Test | Result before fix | Fix |
|---|---|---|---|
| An S3 `POST /bucket?%64elete` contains a key in a read-only input prefix; raw query matching skips the bulk-delete body check. | `encoded_bulk_delete_cannot_delete_a_read_only_input` (`runner/src/door.rs`) | **Broke**: forwarded to the mock storage and returned 204 instead of 403. | The door now recognizes decoded query keys; the request is refused before reaching storage. |
| Two runs that export results from the same table receive credentials rooted at the same shared `results/` folder. | `result_folders_are_private_to_each_run` (`kernel/places.rs`) | **Broke**: the test initially failed to compile because the helper had no run-scoped input. | Added run-scoped result paths; focused unit test passes. |
| A result upload succeeds but no completion report is recorded; `result_bytes = 0` causes expiry to skip the object. | `expiry_removes_an_unreported_result_file` (`server/tests/compute.rs`); `insert_run_records_result_folder_before_upload` (`compute/store.rs`) | **Broke**: the expired run's uploaded file remained, and new result folders were not recorded at queue time. | Run folders are recorded atomically with the run; expiry considers any tracked folder, not only reported bytes. The focused SQLite tests pass. |

## Final attack outcomes

| Severity | Attack | Test | Result before fix | Fix |
|---|---|---|---|---|
| HIGH | An S3 `POST /bucket?%64elete` contains a key in a read-only input prefix; raw query matching skips the bulk-delete body check. | `encoded_bulk_delete_cannot_delete_a_read_only_input` (`runner/src/door.rs`) | **Broke**: forwarded to mock storage and returned 204 instead of 403. | Match decoded query keys; refuse the request before forwarding. |
| HIGH | Runs exporting results from the same table shared the `results/` credential prefix, allowing one run to access sibling Arrow objects. | `result_folders_are_private_to_each_run` (`kernel/places.rs`) | **Broke**: the test initially required a run-scoped helper argument that did not exist; source review showed the shared prefix. | Include the run id in the result folder and issue the per-run key beneath it. |
| MEDIUM | A successful upload with no completion report was untracked (`result_bytes = 0`); erasure could clear the marker before an in-flight upload arrived. | `insert_run_records_result_folder_before_upload` (`compute/store.rs`); `expiry_removes_an_unreported_result_file` (`server/tests/compute.rs`); `a_late_result_upload_after_erase_is_cleaned` (`server/tests/erase.rs`) | **Broke**: the unreported object remained; a late upload after erase became untracked. | Persist the folder in the run insert; keep erased pending folders tracked until 24 hours after finish (all result credentials expire within 1h 5m), retry deletion, then clear the marker. |
| HIGH (conditional) | Private-use NAT64 and other transition IPv6 forms were treated as public destinations. | `only_public_addresses_pass` (`runner/src/fetch.rs`) | **Broke**: `64:ff9b:1::a9fe:a9fe` passed the pre-fix public-address check. | Reject local-use NAT64, Teredo, and obsolete IPv4-compatible addresses; continue checking embedded IPv4 for well-known NAT64 and 6to4. |
| LOW | Percent-encoded dot segments, separators, or backslashes passed the catalog-door route check and could be normalized differently by the HTTP URL parser. | `the_catalog_door_passes_only_the_run_s_tables` (`runner/src/door.rs`) | **Broke**: `%2e%2e` was accepted as a table path segment. | Reject decoded `.`, `..`, slash, and backslash segments before forwarding. |
| LOW | An import's run title exposed the remote hostname, although the full signed URL was hidden. | `an_import_from_an_address_is_downloaded_by_the_machine_and_never_shown_back` (`server/tests/compute.rs`) | **Broke**: fail-first assertion showed `Import files.example.com into sales.raw.fresh`. | Use a generic `remote file` title; keep the URL only in the machine-side fetch request. |

### Held paths and limits

- URL scheme, userinfo, nonstandard ports, private literal/name destinations,
  IPv4-mapped IPv6, well-known NAT64, 6to4, and generic error redaction:
  `the_address_as_written_is_checked`, `names_and_literals_for_private_hosts_are_refused_before_any_request`,
  and `only_public_addresses_pass` pass. The import API test also proves the
  signed URL and hostname are absent from the run view, generated script, and
  audit response:
  `an_import_from_an_address_is_downloaded_by_the_machine_and_never_shown_back`.
- Redirects are manual (`Policy::none`); each `Location` is joined and checked
  on the next loop iteration. DNS results are pinned with `resolve_to_addrs`.
  No end-to-end redirect/DNS-rebinding test was run because the test harness
  has no trusted HTTPS/DNS fixture. Custom NAT64 prefixes configured by a
  customer's network cannot be identified from the address alone; the known
  local-use prefix is blocked, but that environment-specific case remains
  uncertain.
- Fetch size and total-time limits are enforced from Content-Length and while
  streaming, with cleanup on timeout/error. The 2 GiB and 10-minute boundaries
  were code-reviewed, not exercised at those real sizes/times.
- Credential rotation/deletion and secret/vault-path redaction:
  `customer_storage_secrets_are_write_only_and_rotation_is_fenced`,
  `rotation_and_deletion_invalidate_cached_location_handles`, and
  `location_creation_and_credential_deletion_do_not_orphan_a_location` pass.
  Workspace location names and overlapping prefixes are scoped by
  `storage_is_set_per_workspace`, `location_like_wildcards_do_not_match_other_catalog_prefixes`,
  and `a_catalog_keeps_its_data_where_its_location_says`.
- A caller who loses input access cannot obtain a result link:
  `completed_results_require_current_input_access`. Erasure removes an
  uploaded Arrow file and returns 404 for its link:
  `run_results_tokens_and_shared_output_files_are_erased`. Per-run result
  folders constrain the issued credential; the real provider's STS policy was
  not exercised because no live RustFS service was configured. The full check
  also found the existing result-link route missing from OpenAPI; it is now
  documented with its authorization and response contract.
- Request framing and body caps are held by
  `transfer_encoding_and_expect_cannot_smuggle_a_second_request_body` and
  `the_storage_door_rejects_a_declared_body_over_its_limit`. Storage path
  escapes and catalog path normalization are covered by
  `the_storage_door_allows_only_the_leased_folders_and_known_operations` and
  `the_catalog_door_passes_only_the_run_s_tables`.
