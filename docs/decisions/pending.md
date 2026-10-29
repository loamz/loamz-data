# Decisions taken alone, to review

Each is the smallest reversible choice made while the owner was away (2026-10-29 night). Change any of them freely.

1. **One generic "securables" resource** (`/api/v1/securables`, with a `kind`) instead of separate catalog, schema and table endpoints. Less code; the OpenAPI document can split it later without touching storage.
2. **No principals table.** A principal is whatever id the identity provider puts in a token. IAM stores grants only. Revisit if you want to list "all people" or attach display names.
3. **Tenant is taken from the token.** `u-<subject>` for a person with no organization, `o-<alias>` for one organization; several organizations in one token are refused. Organization owners are named in `LOAMZ_DATA_ADMINS`. This is a placeholder for real Keycloak roles; an agent acting for an owner does not inherit that.
4. **Hiding, not forbidding.** A caller who may not see something gets 404. The tenant root is the exception: its admin endpoints answer 403 to members.
5. **The creator becomes owner** of what they create (and so does an agent acting for them), as in Unity Catalog.
6. **One global audit chain** across tenants, not one per tenant. Simple and tamper-evident; a tenant reads only its own entries, but `verify` walks everything. Split per tenant if tenants must be audited apart.
7. **A change and its audit entry are two writes, not one transaction.** A crash between them could leave a change with no entry. Fix by writing both in one database transaction.
8. **Audit keeps ids.** It survives an erase by design, so it still names the person's id (never content). If an erased person's id itself must go, the chain needs per-subject pseudonyms; not built.
9. **Only the `local` placement exists.** Other placements (S3, Postgres-backed) are refused with 400 rather than quietly stored locally.
10. **Volumes are folders named by the volume's id**, not its name, so renames and odd names never touch the disk path.
11. **A dropped entry vanishes at once and its bytes follow in the task.** Names stay reserved until the task finishes.
12. **Erase is tenant-wide for one subject:** everything they own (with what is inside), plus every grant they hold. Not tonight: derived data, backups.
13. **Task retries:** five attempts, a growing wait, a 60 second lease, then `failed` with an audit entry.
14. **Single writer.** SQLite gets one connection; the audit chain is serialized by a lock in the process. Two servers on one database are not supported.
15. **`serde_yaml`** is used only by the spec test (it is archived upstream, but is dev-only).
