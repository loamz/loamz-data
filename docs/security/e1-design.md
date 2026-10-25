# E1: proposed MLS wire and state design

Status: awaiting owner review, alongside [the threat model](./e2ee-threat-model.md). No E1 crypto implementation or encrypted-chat claim is included in the Phase 0 changes.

## Protocol boundary

Use RFC 9420 MLS, ciphersuite 0x0001. A TypeScript `MlsClient` port separates chat code from the selected implementation. The browser and future agent keyholder use the same exact pinned dependency version. Sharing a library does not guarantee interoperability; test the implementations together and against official interop vectors. Select and verify the library release and license at implementation time, then include it in the audit scope.

E1 supports people-only chats on one device each. Do not admit agents or archives until their authenticated recipient, key custody, journal and disclosure paths exist. Do not silently downgrade an encrypted chat to plaintext.

## Device credentials

The application credential is a versioned, canonically encoded structure containing issuer, tenant, user, device, kind, Ed25519 public signing key, issued-at and expires-at. Sign the canonical bytes with a domain separator dedicated to messenger credentials; never reuse data-token signatures as messenger signatures. Pin accepted issuer keys and rotate by explicit key id. Validate bounds and reject unknown versions, algorithms, fields and expired credentials.

Use this authenticated identity inside an MLS basic credential, binding the device key to the MLS signature key. The exact encoding must be covered by golden byte vectors before implementation. A human `kind` is issued only to verified human sessions, and callers cannot select it in a registration request. In E1 the issuer remains trusted for identity. E3 adds user cross-signatures and verification outside the delivery service.

Every device is a leaf. KeyPackages have explicit ids, suite, protocol version, expiry, credential and device binding. The service validates the public structure and claims a package atomically. A retry id returns the same claim to the same requester; another requester cannot consume it. Publish Welcomes only with the successful corresponding roster transition.

## Channel entries and fence

An encrypted application entry has empty `text` and `data.mls = {version: 1, wire: <base64url MLS bytes>, epoch: e}`. Public channel fields include kind, sequence, reply target and bounded agent mention addresses. Attachment metadata, reactions and edited text stay inside the authenticated MLS payload. Bind the immutable chat id, purpose and version in MLS authenticated data. Decryption rejects a public envelope inconsistent with that data.

A handshake entry is hidden, not unread and wakes nobody. Its append is committed on the same database transaction as the roster transition and epoch increment. The server's fence is the last committed epoch, initially the group's initial epoch. Application append at e checks fence=e and leaves it unchanged. Commit append based on e checks fence=e and atomically stores fence=e+1. Stale operations change no state. A commit retry uses an idempotency key and returns the original committed entry, never a second increment.

The server cannot verify MLS secrets. It checks application authorization and the proposed visible roster, while clients independently validate the commit, credential chain, roster and notices. Store the cryptographic transcript fingerprint with local state. A mismatch or missing epoch blocks sends and triggers catch-up. Never accept a server-supplied snapshot in place of verifying the transcript.

## Local transaction and concurrency

A dedicated worker owns MLS mutations under a Web Lock keyed by signed-in account and device. Tabs and the service worker send requests to that owner. Persist encrypted MLS state, processed-sequence cursors and outbound retry records atomically in IndexedDB. Use a non-extractable wrapping key, call `navigator.storage.persist()`, and explain eviction and recovery behavior.

The send transaction must account for MLS ratchet advancement even if the network result is unknown. Persist the exact ciphertext and retry id before upload, and resend those bytes on retry. A decrypt processed before a crash must not consume its ratchet twice: persist the decrypted application result or a suitable encrypted replay record with the state transition. Killing an owner at each boundary is a required test.

A new member cannot read pre-join channel history or fetch blobs outside their membership grant. Catch-up returns handshakes in order as well as application entries. Pagination must start at the member's join boundary, not discard one old page and stop.

## Files, search and export

Generate a new AES-256-GCM key and nonce for each file. Upload opaque bytes through the blob service, authorize access by chat membership, and put the key, nonce, hash, media type, size and thumbnail inside the MLS message. Verify authentication and size before display. Use local object URLs; the server's image and preview services receive no plaintext.

Decrypt previews and search on the device. Server export must omit encrypted content, including attachment keys, previews and MLS secrets. Push remains exactly a content-free wake. Show a generic visible notification if the local owner cannot decrypt.

## Acceptance and unresolved decisions

Required before merging E1: reviewed byte-level credential encoding, concrete library/version/license, official vector source and CI invocation, fence race tests, random roster transitions, sender retry after uncertain acknowledgement, crash/rollback tests, multi-tab/service-worker tests, no-plaintext scans, and completed Phase S browser verification. Cross-device recovery and user cross-signing remain E3 features and must not be advertised in E1.

Owner review is required before writing E1 code. In particular, agree on the issuer trust in E1, the consent UI for roster changes, the atomic outbox/ratchet rules, offline send behavior, and the explicit absence of recovery until E3.

## Review notes (owner review pass)

Accepted, with these additions:

- **Retention is a cryptographic roster change.** An org that retains some people adds a visible `archive` member (its own MLS leaf,
  credential kind `archive`) to exactly the chats that include a retained person. It is never a server-side switch or a silent copy,
  and every member sees the notice before they can send to the new epoch. A chat of people who are not retained never gets one.
- **Wrong recovery key** fails with no partial output and no oracle about how close the key was (E3 test).
- **First-party delivery is the weakest link** and stays in the product's privacy text: the operator serves the JavaScript. A
  published build manifest and Subresource Integrity are the mitigations until independent distribution exists.
- **Library choice is open until the vectors run.** `ts-mls` is the candidate; pin an exact version and make the official interop
  vectors a CI gate before any E1 code depends on it. Fall back to OpenMLS (Rust) if it fails them.
- **The credential's golden bytes come first.** Write the canonical encoding and its test vectors (including negative cases: unknown
  version, expired, wrong issuer, extra fields) before the registration endpoint.
