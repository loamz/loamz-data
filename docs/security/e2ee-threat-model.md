# Messenger encryption threat model

Status: proposed architecture for owner review. The current messenger is not end-to-end encrypted. Do not display an encryption lock or publish an end-to-end encryption claim until verification and independent audit are complete.

## Assets and trust

Protect message bodies, edits, reactions, files, previews, recovery material, device signing keys and MLS state. Organization membership, chat membership, send times, IP addresses, agent mentions, delivery and read cursors, and approximate padded sizes remain visible to the delivery service.

People-only encrypted chats exclude the server operator, organization administrators and AI providers. An agent is a visible recipient whose runtime, sandbox, session storage and AI provider receive plaintext. An archive is a visible recipient whose organization may export plaintext under audited access. Neither belongs in a strictly private chat.

The initial web application and updates are supplied by the operator. A compromised server that serves malicious JavaScript can steal plaintext or use non-extractable browser keys. CSP and Trusted Types reduce injection risk; they do not solve malicious first-party delivery. Independent client distribution, reproducible builds and transparency are later mitigations. The privacy description must disclose this boundary.

## Adversaries

- An attacker with a database dump, object-store access, Redis access, or push-service access.
- A hostile or compromised delivery service that reorders, drops, replays or forks traffic and attempts unauthorized roster changes.
- A malicious authenticated colleague, an organization administrator, or a malicious chat member.
- Injected Markdown, model-written UI, compromised frontend dependencies, or a compromised build pipeline.
- A stolen, revoked or restored device; concurrent tabs; a device that crashes while advancing its MLS state.
- A compromised agent keyholder, agent tool or archive export session.

MLS does not protect against a legitimate recipient copying plaintext. A delete cannot retract data already decrypted, exported or retained by another member. Forward secrecy also depends on erasing superseded secrets; it is not a property of keeping an encrypted database backup indefinitely.

## Identity and roster

A server signature proves that the authentication service issued a credential. It does not stop that service from inventing a human identity or issuing a ghost device. Before cross-signing and independent identity verification, the server remains trusted for identity. A bot badge derived only from a server-issued `kind` is not cryptographic protection against a malicious issuer mislabelling a recipient.

A member must verify the issuer, tenant, user, device, signing key, expiry and supported protocol version. E3 additionally requires the user's signature and an independently verifiable user-signing-key fingerprint. Role and retention changes cannot silently change the cryptographic roster. Compare every authenticated MLS roster to the application roster; block sends on mismatch rather than merely showing a dismissible warning.

Adding an agent or archive requires the consent notice before sending to the new epoch. The server cannot safely add an archive by itself: a current authorized crypto member must commit the addition, and all members must process the new roster. If policy requires an archive but no authorized member can add one, block sending. An export must not broaden retention to chats without a retained person.

## Delivery and state

Authenticate the chat id, protocol version, epoch and entry purpose with MLS authenticated data. Authorize membership before serving an entry, Welcome, group info, blob or key package. Bind blob grants to the chat and tenant, not just possession of a storage path.

Serialize commits with an atomic epoch compare-and-set on the same transaction as the channel append and roster update. Applications at epoch e do not advance the fence. A commit consumes exactly e and advances to e+1. A stale commit cannot publish a Welcome or claim a successful membership change. Detect omitted or reordered epochs; recover by fetching ordered commits, never by disabling verification.

Claim each KeyPackage once in SQL. Make retries idempotent by authenticated device and request id. Bound package sizes, expiry, lifetime and mailbox growth. Redis contains pointers, not plaintext or authority. Reconnect from durable sequence positions; retain enough commit history for devices to catch up.

## Endpoint and recovery risks

IndexedDB encryption with a non-extractable WebCrypto wrapping key helps against casual extraction, but origin scripts can still ask it to decrypt. Eviction, browser profiles and device theft need explicit product behavior. A Web Lock protects one crypto owner across tabs and the service worker; persist state atomically before exposing a successful send or acknowledgement. Test a crash at each state transition.

The recovery key must be a random 256-bit secret displayed once. Encrypt backups with authenticated context, versioning and rollback detection. A wrong key must fail without returning partial data. Losing every device and the recovery key loses private history. Recovery does not bypass MLS authentication or automatically add a new device to existing groups.

## Agent and archive exposure

The keyholder is a separate trust boundary, authenticated over a Unix socket or mTLS. Authenticate the agent, tenant, chat and membership for every request. Separate keys per agent or archive and scope replay caches to the agent, chat, epoch and sequence. Cache enough replay state durably for a crashed agent run to see the same plaintext, while preventing one agent from reading another's data.

Journaling must record ciphertext before decryption. Encrypt before posting. Model prompts, tool calls, sandbox files, tracing, error messages and provider retention are separate plaintext exposure paths; each needs an explicit policy. Organization KMS/BYOK keys protect archive storage, not the runtime while it is decrypting for an authorized export.

## Browser and launch gates

Phase S requires nonce CSP, enforced and tested Trusted Types, no third-party chat scripts, a reviewed renderer allowlist or sandbox, audited Markdown URLs, build integrity and an XSS corpus. Report-only Trusted Types is groundwork, not completion of Phase S. Browser checks must cover authenticated production builds, sign-in, model UI, file previews, workers, push and multiple users.

E1 requires pinned MLS dependencies, official interop vectors, concurrent commit properties, no-plaintext database/blob/log scans, and lock-leader crash tests. E2 adds browser/keyholder interoperability and agent journal/replay tests. E3 adds cross-signing, revocation, recovery and retention isolation. E4 adds matching safety numbers, key-change notices, fuzzing, SBOM and an independent audit with all blocking findings closed.

Open reviews: first-party web delivery trust; authenticated retention consent; recovery rollback; device verification UX; keyholder compromise scope; storage and log encryption; metadata minimization; ephemeral presence; archive export authorization. No E1 implementation is authorized by this document alone: the plan requires owner review of this threat model and the wire design first.
