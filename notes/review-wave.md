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

The security-review agent is checking current `main` and will provide specific
attack evidence before tests are written. No production code is changed until
the failing tests are in place.

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

## Findings

The bounded review identified three likely defects. Tests are being added
before the corresponding production changes.

| Attack | Test | Result before fix | Fix |
|---|---|---|---|
| An S3 `POST /bucket?%64elete` contains a key in a read-only input prefix; raw query matching skips the bulk-delete body check. | `encoded_bulk_delete_cannot_delete_a_read_only_input` (`runner/src/door.rs`) | **Broke**: forwarded to the mock storage and returned 204 instead of 403. | The door now recognizes decoded query keys; the request is refused before reaching storage. |
| Two runs that export results from the same table receive credentials rooted at the same shared `results/` folder. | `result_folders_are_private_to_each_run` (`kernel/places.rs`) | **Broke**: the test initially failed to compile because the helper had no run-scoped input. | Added run-scoped result paths; focused unit test passes. |
| A result upload succeeds but no completion report is recorded; `result_bytes = 0` causes expiry to skip the object. | `expiry_removes_an_unreported_result_file` (`server/tests/compute.rs`); `insert_run_records_result_folder_before_upload` (`compute/store.rs`) | **Broke**: the expired run's uploaded file remained, and new result folders were not recorded at queue time. | Run folders are recorded atomically with the run; expiry considers any tracked folder, not only reported bytes. The focused SQLite tests pass. |

Other requested attack classes to verify: fetch redirects and private address
forms, DNS rebinding, credentials/userinfo, body size and time limits, result
link permissions after access loss or erase, customer-location credential
rotation and workspace boundaries, and request framing/path normalization in
both doors. The reviewer held custom-prefix NAT64 and credential-generation
cache timing as hypotheses rather than confirmed defects; both need test
evidence before being reported as held.
