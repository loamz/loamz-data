# Messenger implementation status

This is a partial implementation of Phase 0 and security groundwork. It is not a completed messenger launch or an end-to-end encrypted product.

## Implemented

- One runtime store for channels, pairs, hosted workers and observers.
- Membership-based chats with human and agent members, stable tenant-aware direct-chat ids, admin authorization and tenant-scoped access.
- Versioned group API matching the console's message and file contracts, durable edit/reaction/tombstone markers, read/delivered cursors, working state, limits and breaker settings.
- On-demand channel agents built from saved profiles; uncached activation reloads roster and tool permissions across processes.
- Durable feed catch-up and Redis pointer fan-out with database fallback.
- Organization people lookup and invite endpoints using the directory service account.
- Console people selection, direct conversations, bot badges, visible AI disclosure and viewer-specific message ownership.
- PWA manifest and worker; only a public offline page is cached. Optional VAPID subscriptions and a content-free wake. Notifications remain generic until local decryption exists.
- Nonce CSP, browser response hardening and report-only Trusted Types.
- Proposed threat model and E1 design for review.

## Verified locally

The project-local server environment was used. The new root-level environment created by environment auto-configuration was moved outside the workspace; the existing environments of individual projects were preserved.

- Agent-server suite: 156 tests pass.
- Focused engine suite: 41 tests pass, including a SQLite human/two-agent wake and reply test.
- Focused console suite: 37 tests pass; TypeScript, touched-component lint and the design baseline pass.
- Eight import contracts pass; Ruff passes.
- Live development console renders the people picker, agent badges and plaintext disclosure. Nonce CSP headers are returned on chat routes, including the base path.

These checks do not establish production-browser security or a two-user encrypted conversation.

## Remaining before Phase 0 is complete

- Per-tenant encryption at rest for chat/runtime content and governance tokens, including managed key provisioning and rotation. Current chat storage is plaintext.
- Atomic catalog/runtime membership transitions, concurrent admin-role changes and full in-flight revocation coverage.
- Per-person read-tick presentation and full role-management UI. The server exposes per-person cursors, but the console still primarily presents the aggregate seen tick.
- Ephemeral human typing and presence; observing agent pair chats through the feed's `watch` option.
- Two-process Redis/SSE acceptance, disconnect/loss tests over a real transport, two-user browser acceptance, browser push-service integration and Lighthouse installability checks. Current tests exercise store-backed catch-up and mock push delivery, not this full deployment matrix.
- Recreate the development agent service with the new compose directory settings. An existing container without them reports the directory as unavailable rather than returning fabricated people. Configure optional VAPID keys in the deployment secret store.
- Group agent media loading, sandbox publishing and complete delegation assembly need parity checks against saved profile permissions.

## Remaining in Phase S

Trusted Types is report-only. Complete the model renderer/Markdown audit and XSS corpus, production CSP checks, enforced Trusted Types, SRI/build-manifest verification and browser regression checks before treating Phase S as complete.

## Encryption gate

No MLS implementation, agent keyholder, multi-device recovery, cross-signing, archive retention, safety-number system or audit has been implemented. E1 must wait for owner review of [the threat model](./e2ee-threat-model.md) and [the wire/state proposal](./e1-design.md), as required by the plan. Complete the earlier phase gates first. Never turn the `e2ee` flag on to represent encryption that does not exist.
