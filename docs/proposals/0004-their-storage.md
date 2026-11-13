# Proposal 0004: their storage

- **Status:** proposed for lead review, 14 Nov 2026. This is a contract decision, not an implementation or a security certification.
- **Scope:** W2 and the W3 interfaces it needs in [the beta proposal](0003-beta.md). The lead agrees these contracts before code.
- **Starting point:** [storage](../design/storage.md), [governance](../design/governance.md), and the current credential and location APIs. This proposal replaces the storage design's post-beta timing for customer buckets: they are in the beta.

## 1. Decisions

### 1.1 Who holds a customer's keys

**Choose:** A workspace admin or organization account admin creates a credential or replaces its secret through the authenticated, TLS-only Data API. A service principal cannot create or rotate credentials, even if it can read or write data. Key values enter only through a `writeOnly` request body and are stored only in OpenBao KV v2; the relational database stores a server-generated secret reference, generation, status and non-secret connection settings. Clients cannot supply or choose the OpenBao path. Requests and OpenBao calls have bounded body sizes, no body capture in access logs, traces, crash reports or error responses, and no secret-bearing `Debug` output. Audit records only actor, credential ID, operation, generation and outcome, in the database transaction that changes the credential. `GET`, list, errors and audit never return a key, a key ID, a token, a vault response, or a value derived from one. The role ARN and external ID for AWS are configuration, not secret keys. Do not include credentials in URLs or sample commands.

For AWS, prefer the Data installation's own AWS workload identity and a customer role; a customer does **not** give us an AWS access key. For R2 the customer submits the parent S3 key ID, secret and a separately scoped Cloudflare API token. For MinIO/RustFS they submit a dedicated non-root service user's access and secret keys. These are long-lived custody secrets; short-lived, folder-scoped credentials issued to authorized clients and workers are different and may be returned only by the appropriate authorized vending contract.

**Verification without revealing keys:** `POST .../check` takes a location ID that uses this credential, then uses the active credential to make a scoped, non-destructive provider request and returns only `usable`, `unavailable` or `denied`, never a provider response or the submitted material. The check is available after the location is registered and proves connectivity and the checked operation at that moment; it does not prove all future permissions or object contents. The customer may also check their own credential directly with their provider. **The server must read the secret from OpenBao into memory to use it:** write-only means it is never read back by an API caller, not that the trusted service can authenticate without reading it. A compromised server with vault read access can retrieve it. Disable logging of request bodies and upstream errors at every layer, including proxy and SDK; retain OpenBao auditing with sensitive fields protected, and verify it never records raw values. Do not claim that encryption makes the running server unable to use the secret.

**Rotate:** `PUT .../secret` replaces the complete provider-specific secret set, not individual fields. First reserve generation N+1 and its deterministic server-generated OpenBao path in a durable pending operation, then write the complete secret to that path. In a database transaction lock the credential, compare its expected generation, switch the active reference, increment generation and audit. A durable reconciler retries pending operations and deletes orphaned candidate paths after a failed commit and old OpenBao versions after successful cutover; versioned backups and provider-side copies have separate retention and must be handled by the operator. Retry is idempotent by client-supplied idempotency key; never store the body or key material in the idempotency record. A failed vault write or DB commit leaves the old generation active. If the outcome is unknown, the client queries non-secret metadata, not the secret. The customer keeps the old provider key valid until `GET` reports the new generation active and a check succeeds, then revokes the old key at the provider.

The pending operation has an owner lease and deadline so a sweeper cannot delete a candidate while it is being written; after an abandoned request, the client must resubmit the secret because it was never saved in the database. Reusing an idempotency key returns the first operation's status and never applies the later request body; use a new key for a new candidate. Cutover invalidates generation-keyed clients and vended-credential caches across all server replicas. Block new leases until each replica observes the committed generation; check the current generation before vending and before every publish. Affected queued runs reauthorize; running work is cancelled and its results and uncommitted output are withheld. Cancellation is best effort; already issued direct credentials remain usable until expiry or provider revocation. Rotation alone is not an immediate remote revocation guarantee. No automatic old-key rollback after cutover: rotate again to a known working key if needed.

**Reject:** putting keys in the database, allowing callers to choose secret references, returning a masked key as evidence, and a long-lived cached client that continues with an old key. Those make custody, rotation or tenant isolation harder to verify.

### 1.2 One store and issuer per location

**Choose:** Give every location a stable ID, credential ID and canonical provider identity; every catalog, table and volume carries its explicit location ID and an object-relative folder. A schema inherits its catalog's location; an external object explicitly names its own location and path. The installation default is a real reserved location, provisioned per workspace with a disjoint prefix, not an empty location ID. Resolve `(org, workspace, location_id)` first, validate ownership and state, then resolve its credential. Reject a missing, suspended or foreign location; never fall back to the installation bucket. An Iceberg metadata pointer is always paired with its location; absolute Iceberg URIs must resolve to that location and the object's allowed prefix. Moving an object is a separate verified copy and metadata-rewrite operation, outside this proposal.

Replace the deployment-wide `App.stores` and `App.issuer` with a registry of `(org_id, workspace_id, location_id, credential_generation)` clients and vendors. Create lazily, use bounded size/idle eviction and single-flight initialization, and drop entries on rotation, deletion or suspension. Never key a registry, result cache or credential cache by bucket name or folder alone. Replicas use DB-backed generation checks plus cross-replica invalidation; a notification is a hint, and an unreachable generation authority fails closed. Store and issuer calls take an already authorized location handle, not arbitrary caller URLs. Update **all** table creation/load/commit/config, Iceberg credential vending, volume operations, lifecycle/erase tasks, storage management, compute lease construction, runner/serverless storage descriptors, and audits that currently use `app.stores` or `app.issuer`; tests and default-store configuration use the same registry. A multi-location run gets a separate scoped access descriptor per location; no merged bucket-wide credential and no silently using one store for another.

Normalize provider, region, bucket and prefix before overlap checks. Use provider-verified bucket identity and a transactional lock covering all locations and default roots for that physical bucket; reject equal or ancestor/descendant paths, including concurrent creates and objects pending deletion. Endpoint strings alone cannot prove two custom DNS names are different stores. For custom endpoints on a shared installation, accept only operator-verified canonical store identities; reject an unverified alias rather than assume it is distinct. AWS/R2 identities come from their provider configuration and validation, not a customer-provided label. Resolve and pin allowed destinations per connection, reject redirects and DNS rebinding to loopback, link-local, private and metadata networks; an operator may explicitly register a private endpoint for a self-hosted installation. Use TLS and verify certificates outside the explicitly configured development stack. Do not put passwords in endpoints.

**Reject:** one global store with bucket branches throughout services, client caches keyed by endpoint alone, deriving location from a path, and assuming a different hostname means a different physical bucket.

### 1.3 How compute gets scoped access

**Choose:** A location declares one tested vending adapter. Before each issuance, governance checks the current person and delegated app, workspace, target compute, object and operation. The adapter issues only the exact object's folder(s) in **one** bucket and allowed operations. Different locations receive separate credentials. The run carries expiry and generation, and neither a parent key nor an OpenBao reference. Clients that cannot consume the issued format fail clearly. Treat provider session-policy and prefix enforcement as capabilities proven against that exact provider/version, not a property inferred from the words “S3 compatible”. Fail closed when a provider rejects or ignores scoping.

**AWS S3.** The customer creates an IAM role in their account with the following trust policy and attached permissions policy. `LOAMZ_SERVER_ROLE_ARN` is the exact workload role ARN operated by this installation, and `EXTERNAL_ID` is a unique opaque value generated by Loamz for this credential and supplied to the customer out of band or in credential metadata. It is not a password. Replace `BUCKET` and `PREFIX` with the verified bucket and a **nonempty**, normalized prefix with no trailing slash or IAM wildcards; provisioning rejects `*`, `?`, path escapes and an empty prefix. This policy is intentionally for a location the service may manage; use the read-only variant by omitting the four write/delete actions when the location is explicitly read-only. Do not add bucket administration, IAM, ACL, public access, or version-deletion rights.

Trust policy on the **customer role**:

```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"AWS": "LOAMZ_SERVER_ROLE_ARN"},
    "Action": "sts:AssumeRole",
    "Condition": {"StringEquals": {"sts:ExternalId": "EXTERNAL_ID"}}
  }]
}
```

Permissions policy attached to that role:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "s3:ListBucket",
      "Resource": "arn:aws:s3:::BUCKET",
      "Condition": {"StringLike": {"s3:prefix": ["PREFIX", "PREFIX/*"]}}
    },
    {
      "Effect": "Allow",
      "Action": ["s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload", "s3:ListMultipartUploadParts"],
      "Resource": "arn:aws:s3:::BUCKET/PREFIX/*"
    }
  ]
}
```

The Loamz workload identity also needs an identity policy permitting `sts:AssumeRole` on the approved customer role ARN; the customer cannot grant that permission in our account. At run time Loamz assumes the role using its external ID and supplies a **narrower session policy** for each object's exact folder and read/write actions. STS intersects the role and session policies. The customer must grant any additional KMS permissions separately if they choose SSE-KMS; until an object-prefix-compatible KMS policy and tests are agreed, reject SSE-KMS rather than promise this policy is sufficient. Bucket policy, organization restrictions and explicit denies still apply. Multipart bucket-wide listing is not granted; operations that require it are unsupported until a narrower verified design is agreed.

AWS `AssumeRole` has a 900-second minimum, and role chaining caps a session at one hour. For beta choose a 900-second session and a maximum 10-minute direct-access run, with a five-minute expiry margin; reject a request that would exceed this limit rather than hand it credentials expiring mid-run. The reaper's heartbeat/cancellation rules remain in force. Longer runs need a separately reviewed renewable broker and are not silently enabled. AWS role trust with an external ID and session policy follows [AWS's cross-account guidance](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_common-scenarios_third-party.html) and [STS access rules](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_temp_control-access_assumerole.html); the [duration constraint](https://docs.aws.amazon.com/powershell/v5/reference/items/Use-STSRole.html) is provider-specific.

**Cloudflare R2.** Customer creates an R2 parent token/key restricted to the target bucket, plus a Cloudflare API token with the provider's minimum permission to create temporary R2 credentials for that account. Store the parent key pair and API token in OpenBao. Request R2 `object-read-only` or `object-read-write` temporary credentials, bucket and exact folder prefixes, with a 15-minute maximum lifetime; their permissions cannot exceed the parent. Verify the account, parent and bucket together; never substitute the Cloudflare management token for an S3 key. R2 is not AWS STS. See [Cloudflare's temporary-credentials API](https://developers.cloudflare.com/api/python/resources/r2/subresources/temporary_credentials/methods/create/). The exact minimum Cloudflare API-token permission and whether it can be separate from the parent are a provider-conformance gate: verify against the customer's account and document the observed scope before beta, rather than assuming a narrower permission exists. A prefix capability that is ignored or cannot be tested is a hard failure.

**MinIO and RustFS.** Customer creates a dedicated built-in, non-root service identity and attaches the S3 policy above, adapted to the exact bucket/prefix and supported actions. They provide its access/secret pair and verified HTTPS S3/STS endpoints. Loamz uses that identity's signed `AssumeRole` request, with the equivalent exact-folder session policy (no `Id` member), and verifies that a credential cannot read a sibling. MinIO built-in-user `AssumeRole` differs from AWS's cross-account role/external-ID mechanism; do not pretend the AWS trust JSON configures MinIO. RustFS sessions intersect the parent and session policies. See [MinIO STS](https://docs.min.io/aistor/developers/security-token-service/) and [RustFS IAM](https://docs.rustfs.com/en/security-compliance/iam). Test supported version, actions, expiration and policy semantics against each actual deployment; advertised compatibility is not proof.

**No safe vending:** refuse direct Iceberg clients and compute for that location with a clear unavailable capability. The control plane may check a connection but must not send a long-lived key to a client, proxy arbitrary requests, or claim remote signing is already supported. A separately specified scoped broker or Iceberg remote signer may be added after its security and interoperability tests; it is not a silent fallback. **Reject:** long-lived keys on machines, a generic STS implementation assumed to work with every S3 service, and unrestricted network or bucket access to make an unsupported store work.

### 1.4 Managed and external objects

**Choose:** A managed table or volume is created under a catalog's managed root. Its durable, leased drop task checks current object identity and location, locks the exact prefix, deletes its files including orphaned files under that prefix, verifies deletion and audits completion before releasing the path. An external table or volume is registered at an explicitly authorized path under a location, outside every managed root and other object path; registering it does not transfer ownership of bytes. Dropping or erasing the registration removes catalog metadata and grants, but does not delete **any** external data, manifests or metadata files, including files Loamz wrote there. Report separately that removal of external data is the customer's responsibility. An external Iceberg table must satisfy the existing path containment and snapshot checks; raw files or manifests pointing outside the registered prefix are refused. Do not claim record-level erasure in external bytes.

Managed deletion respects provider versioning, object lock and retention: a successful current-key delete is not proof all historical versions are erased. If the policy does not permit version deletion or retention prevents it, keep the durable task visibly incomplete or report the explicit retention limitation; never declare irreversible erasure. Catalog deletion recursively follows each object's ownership rule. Reject a location/credential deletion while referenced by active objects or unfinished tasks. **Reject:** cascading deletion of an external prefix and treating a location-wide grant as ownership of all of its data.

### 1.5 Storage grants and catalog grants

**Choose:** Storage credentials, locations and compute targets are explicit grant targets, isolated by org and workspace. Account admins retain the existing organization-wide override; workspace admins can create and manage storage settings in their workspace. Only an owner or `MANAGE` holder may grant or revoke on a target. For ordinary members and service principals:

| privilege | target | meaning |
|---|---|---|
| `CREATE_EXTERNAL_LOCATION` | credential | create a location with that credential, within the credential's provider-authorized scope; never read its secret |
| `READ_FILES`, `WRITE_FILES` | location | raw bytes under that location; never automatically conferred by `SELECT` or `MODIFY` |
| `CREATE_EXTERNAL_TABLE`, `CREATE_EXTERNAL_VOLUME` | location | register that kind of object under the location |
| `CREATE_MANAGED_STORAGE` | location | bind a catalog's managed root within it |
| `USE_COMPUTE` | compute target | submit a run to that target, subject to workspace entitlements and catalog compute policy |

Creating a table or volume also needs the applicable catalog/schema `USE` and `CREATE` privilege. Reading or writing a registered table needs its own `SELECT` or `MODIFY` and applicable catalog/schema `USE` privileges; an ordinary table query does not also require raw `READ_FILES`. Raw location access is a separate, powerful grant: it bypasses catalog table-level row filters and masks and therefore must never be given as a convenience to users subject to those policies. No such user receives raw file or direct Iceberg credentials for a protected table. Because a location-level raw grant covers the whole location, deny raw use if any protected table under that location is one the caller cannot manage; check again when a policy is added, before each issuance, and at lease. A manager who is allowed raw access can see the unmasked data. Every side of a delegated request must pass; groups contribute membership to a side but are not independently authenticated callers. Existing `404` concealment and audited denials apply.

Compute selection needs `USE_COMPUTE` **and** the workspace's `sql-access`/job entitlement **and** the catalog's compute policy. Check on submission and again at lease; explicit inaccessible targets return the same 404 as missing ones, automatic selection considers only permitted targets. Target owners and workspace admins initially have `USE_COMPUTE`; no implicit grant to all members. **Reject:** replacing table grants with storage grants, granting `READ_FILES` to everyone who can query, and making catalog-to-compute policy substitute for the caller's compute grant.

### 1.6 Revocation and rotation over time

This is the binding interpretation of the [storage revocation table](../design/storage.md):

| Moment | Required behavior |
|---|---|
| Commit | Grant revocation, credential disablement, location suspension or rotation increments a DB-backed authorization/generation epoch with its audit entry. The commit is the linearization point. |
| New requests | Every replica consults the current epoch from the primary database before authorization, issuance and publication; do not use a lagging read replica. Once the commit is observed, reject newly authorized use. If it cannot verify the epoch, fail closed. Operations already past their check may be in flight; do not describe their response as instantly reversible. |
| Caches | Evict affected clients, metadata and vended credentials on the event; on every use compare their location, generation, expiry, operation and authorized folders. Notifications accelerate eviction but cannot be the correctness mechanism. |
| Queued work | Recheck every side, object, location, target and epoch before leasing; refuse rather than silently redirect. |
| Running work | Signal cancellation on the next control check, fence output commit and hide results when inputs or compute access are lost. A lost or unresponsive worker is killed or expires by the existing lease/deadline. Explain that cancellation is not instantaneous, and audit the attempt and outcome. |
| Already issued credentials | Previously handed-off direct credentials cannot generally be recalled by Loamz; they can read their scoped prefix until expiry (AWS beta maximum 900 seconds after issuance), or sooner only if the provider actually revokes them. Ask the customer to revoke the parent/role session at the provider for emergency response; provider propagation and side effects are provider-dependent. |
| Already disclosed bytes | Data already read, downloaded or delivered cannot be recalled. Keep this limitation visible when granting raw access. |

A cached decision is not proof of revocation. Test expiry with a controlled clock and actual provider behavior; do not claim a provider revocation API works without evidence. **Reject:** immediate-revocation promises for direct credentials, letting a failed invalidation notification continue issuance, and allowing a cancelled attempt to publish.

### 1.7 W3: principals, compute and migrations

**Choose:** Introduce one internal `Principal` identity: `{id, org_id, kind, subject, workspace_id}`. `kind` is `person`, `group`, or `service`; a person or group is organization-scoped, a service principal is confined to exactly one workspace. Identity providers still authenticate people and supply group membership. An internal principal ID is opaque and immutable; display names and Keycloak subjects are not authorization keys. Resolve existing `user:`, `group:` and `sp:` references at identity boundaries and convert existing grants, owners, memberships, entitlements and audit actor mappings to principal IDs in one controlled upgrade. Audit's erased-person mappings remain unlinkable after erase. Do not treat groups as login identities. Each delegated side expands to its own person/service and groups, then each side independently passes authorization.

Storage and compute become typed targets in the same authorization service, without pretending they have catalog parents. Storage location/credential ownership and target ownership are explicit and scoped to the workspace. Compute owner/`MANAGE` governs target grants; `USE_COMPUTE` is checked as in 1.5. Preserve `catalog_compute` as the separate catalog-to-target restriction.

Before the beta gate, the lead reconciles all existing migrations and applies the target schema to the baseline `0001_init.sql` according to current repository rules. **Freeze the resulting baseline at beta**: record its checksum in the release and never edit or squash a migration already applied to a beta database. All subsequent changes are new numbered, forward-only migrations. The upgrade test starts from an actual database produced by the frozen beta release, including grants, principals, credentials and audit chains, and checks SQLite and Postgres after upgrading; failed migration leaves the database recoverable from the tested backup. No production upgrade depends on a reset. **Reject:** a new string prefix for each actor type, separate authorization engines for storage/compute, and editing a shipped migration.

### 1.8 Threats and boundaries

| Attacker | What they can get or do | Limits and residual risk |
|---|---|---|
| Read-only DB copy | Tenant IDs, object names, endpoint/role metadata, grants, audit metadata and opaque OpenBao references. No bucket keys, API tokens, vended credentials or secret bodies. | DB contents still reveal sensitive metadata; protect backups and restrict access. A DB copy plus separate vault credentials is a different threat. Audit hashes do not encrypt data. |
| Malicious customer | Submit URLs, keys, role ARNs, prefixes, external metadata or requests intended to reach internal services or another tenant's data. | Tenant-scoped lookups and principal checks, canonical non-overlap under lock, endpoint egress controls, provider external IDs, per-object session policies, strict path handling and rechecks; deny unresolved store aliases. A customer who already controls shared provider credentials can access whatever the provider grants independently of Loamz. |
| Compromised Data server | Read active secrets through its OpenBao identity, assume registered roles and issue allowed operations. | The vault protects a DB-only leak, not a compromised server. Per-installation identity, unique external IDs, provider-side bucket/prefix/action policies, short-lived run credentials, scoped vault policies, egress restrictions, detection and customer provider-side revocation reduce—not eliminate—the blast radius. A server with access to several tenants can potentially reach each of their authorized prefixes. |

Customer bucket policies and vault permissions are security boundaries; the registry and cache are not substitutes. No claim of immediate cancellation or perfect isolation is made until tests and review prove the implemented boundaries.

## 2. Public contract to agree

This is an OpenAPI 3.0 **fragment to merge into** `specs/data/v1/openapi.yaml`; existing workspace and bearer security definitions apply. Existing credential create/list/delete and location create/list/delete paths remain, but create and list schemas are replaced as below; no secret references are client-supplied or returned. Secret rotation requires `If-Match` with the current generation and an idempotency key. All operation errors use the existing RFC 9457 problem response; hidden resources and unauthorized target access are 404. `Cache-Control: no-store` applies to all credential operations.

```yaml
paths:
  /api/v1/workspaces/{ws}/storage/credentials:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
    get:
      summary: List non-secret credential metadata
      responses:
        '200':
          description: Credentials visible to this workspace administrator
          content:
            application/json:
              schema:
                type: object
                required: [items]
                properties:
                  items: {type: array, items: {$ref: '#/components/schemas/StorageCredential'}}
        '404': {$ref: '#/components/responses/Problem'}
    post:
      summary: Create a storage credential
      parameters:
        - {name: Idempotency-Key, in: header, required: true, schema: {type: string, maxLength: 128}}
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/CreateStorageCredential'}
      responses:
        '201':
          description: Non-secret metadata only
          content:
            application/json:
              schema: {$ref: '#/components/schemas/StorageCredential'}
        '400': {$ref: '#/components/responses/Problem'}
        '404': {$ref: '#/components/responses/Problem'}
        '409': {$ref: '#/components/responses/Problem'}
        '503': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/storage/credentials/{id}/secret:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
      - {name: id, in: path, required: true, schema: {type: string}}
    put:
      summary: Replace the complete secret set; body is never returned
      parameters:
        - {name: If-Match, in: header, required: true, schema: {type: string}}
        - {name: Idempotency-Key, in: header, required: true, schema: {type: string, maxLength: 128}}
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/StorageSecret'}
      responses:
        '204': {description: New generation activated; no response body}
        '400': {$ref: '#/components/responses/Problem'}
        '404': {$ref: '#/components/responses/Problem'}
        '409': {$ref: '#/components/responses/Problem'}
        '412': {$ref: '#/components/responses/Problem'}
        '503': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/storage/credentials/{id}/check:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
      - {name: id, in: path, required: true, schema: {type: string}}
    post:
      summary: Check the active credential without revealing it
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              additionalProperties: false
              required: [location_id]
              properties:
                location_id: {type: string}
      responses:
        '200':
          description: Result of a bounded non-destructive check
          content:
            application/json:
              schema:
                type: object
                required: [status, generation]
                properties:
                  status: {type: string, enum: [usable, denied, unavailable]}
                  generation: {type: integer, format: int64}
        '404': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/storage/credentials/{id}:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
      - {name: id, in: path, required: true, schema: {type: string}}
    get:
      summary: Read non-secret credential metadata and generation
      responses:
        '200':
          description: Non-secret metadata and an ETag for If-Match
          headers:
            ETag: {schema: {type: string}}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/StorageCredential'}
        '404': {$ref: '#/components/responses/Problem'}
    patch:
      summary: Replace the AWS role ARN and fence the prior generation
      parameters:
        - {name: If-Match, in: header, required: true, schema: {type: string}}
        - {name: Idempotency-Key, in: header, required: true, schema: {type: string, maxLength: 128}}
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              additionalProperties: false
              required: [role_arn]
              properties:
                role_arn: {type: string}
      responses:
        '204': {description: Role configuration activated and prior generation fenced}
        '400': {$ref: '#/components/responses/Problem'}
        '404': {$ref: '#/components/responses/Problem'}
        '412': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/storage/locations:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
    get:
      summary: List locations visible to this caller
      responses:
        '200':
          description: Authorized location metadata
          content:
            application/json:
              schema:
                type: object
                required: [items]
                properties:
                  items: {type: array, items: {$ref: '#/components/schemas/StorageLocation'}}
        '404': {$ref: '#/components/responses/Problem'}
    post:
      summary: Create an authorized non-overlapping location
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/NewStorageLocation'}
      responses:
        '201':
          description: Non-secret location metadata
          content:
            application/json:
              schema: {$ref: '#/components/schemas/StorageLocation'}
        '400': {$ref: '#/components/responses/Problem'}
        '404': {$ref: '#/components/responses/Problem'}
        '409': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/securables:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
    post:
      summary: Create a catalog, schema, volume or view with its authorized placement
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/CreateSecurableWithStorage'}
      responses:
        '201': {description: Created}
        '400': {$ref: '#/components/responses/Problem'}
        '404': {$ref: '#/components/responses/Problem'}
        '409': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/tables:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
    post:
      summary: Create or register an Iceberg table
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/CreateTableWithStorage'}
      responses:
        '201': {description: Created}
        '400': {$ref: '#/components/responses/Problem'}
        '404': {$ref: '#/components/responses/Problem'}
        '409': {$ref: '#/components/responses/Problem'}
  /api/v1/agent/lease:
    post:
      summary: Lease a run and receive its per-location storage access
      security: [{agent: []}]
      responses:
        '200':
          description: Authorized work order
          content:
            application/json:
              schema:
                type: object
                required: [run, token, limits, storage]
                properties:
                  run: {type: object, description: Existing run and declared inputs}
                  token: {type: string, description: Existing scoped job token}
                  limits: {type: object, description: Existing run limits}
                  storage: {$ref: '#/components/schemas/LeaseStorage'}
        '204': {description: No work available}
        '401': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/compute/targets/{id}/grants:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
      - {name: id, in: path, required: true, schema: {type: string}}
    get:
      summary: List grants on a compute target
      responses:
        '200':
          description: Typed principal grants, including USE_COMPUTE
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PrincipalGrantList'}
        '404': {$ref: '#/components/responses/Problem'}
    put:
      summary: Replace a principal's grants on this target
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PrincipalGrant'}
      responses:
        '204': {description: Grant updated and audited}
        '404': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/storage/{target_kind}/{id}/grants:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
      - {name: id, in: path, required: true, schema: {type: string}}
      - {name: target_kind, in: path, required: true, schema: {type: string, enum: [credentials, locations]}}
    get:
      summary: List grants on a credential or location
      responses:
        '200':
          description: Typed principal grants for this storage target
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PrincipalGrantList'}
        '404': {$ref: '#/components/responses/Problem'}
    put:
      summary: Replace a principal's grants on this credential or location
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PrincipalGrant'}
      responses:
        '204': {description: Grant updated and audited}
        '404': {$ref: '#/components/responses/Problem'}
  /api/v1/workspaces/{ws}/securables/{id}/grants:
    parameters:
      - {name: ws, in: path, required: true, schema: {type: string}}
      - {name: id, in: path, required: true, schema: {type: string}}
    get:
      summary: List grants using typed principal references
      responses:
        '200':
          description: Grants on the securable
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PrincipalGrantList'}
        '404': {$ref: '#/components/responses/Problem'}
    put:
      summary: Replace a principal's grants on this securable
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PrincipalGrant'}
      responses:
        '204': {description: Grant updated and audited}
        '404': {$ref: '#/components/responses/Problem'}
components:
  schemas:
    PrincipalRef:
      type: object
      required: [id, kind]
      properties:
        id: {type: string}
        kind: {type: string, enum: [person, group, service]}
    PrincipalGrant:
      type: object
      required: [principal, privileges]
      properties:
        principal: {$ref: '#/components/schemas/PrincipalRef'}
        privileges: {type: array, uniqueItems: true, items: {type: string}}
    PrincipalGrantList:
      type: object
      required: [items]
      properties:
        items: {type: array, items: {$ref: '#/components/schemas/PrincipalGrant'}}
    NewStorageLocation:
      type: object
      additionalProperties: false
      required: [name, credential_id, bucket, prefix]
      properties:
        name: {type: string}
        credential_id: {type: string}
        bucket: {type: string}
        prefix: {type: string, description: Canonical nonempty prefix for customer storage}
    StorageLocation:
      type: object
      required: [id, name, credential_id, bucket, prefix, provider_identity, generation, state]
      properties:
        id: {type: string}
        name: {type: string}
        credential_id: {type: string}
        bucket: {type: string}
        prefix: {type: string}
        provider_identity: {type: string, description: Verified canonical physical-store identity}
        generation: {type: integer, format: int64}
        state: {type: string, enum: [active, disabled]}
    ObjectStorage:
      type: object
      required: [mode]
      properties:
        mode: {type: string, enum: [managed, external]}
        location_id: {type: string, description: Required for external objects}
        path: {type: string, description: Required for external objects; forbidden for managed objects}
    CreateSecurableWithStorage:
      type: object
      required: [kind, name]
      properties:
        kind: {type: string, enum: [catalog, schema, volume, view]}
        parent_id: {type: string}
        name: {type: string}
        classification: {type: string, enum: [normal, personal, sealed]}
        location_id: {type: string, description: Catalog location; omitted uses workspace default}
        storage: {$ref: '#/components/schemas/ObjectStorage'}
    CreateTableWithStorage:
      type: object
      required: [parent_id, name, storage]
      properties:
        parent_id: {type: string}
        name: {type: string}
        columns:
          type: array
          description: Required for managed table creation; forbidden when registering an external Iceberg table
          items:
            type: object
            required: [name, type, required]
            properties:
              name: {type: string}
              type: {type: string}
              required: {type: boolean}
        storage: {$ref: '#/components/schemas/ObjectStorage'}
    LeaseStorageAccess:
      type: object
      description: Short-lived run-scoped access, not a customer parent key; visible only to the authorized compute target.
      required: [location_id, bucket, endpoint, expires_at_ms, credential_generation, folders, operations, tables, temporary_credentials]
      properties:
        location_id: {type: string}
        bucket: {type: string}
        endpoint: {type: string, format: uri}
        expires_at_ms: {type: integer, format: int64}
        credential_generation: {type: integer, format: int64}
        folders: {type: array, minItems: 1, items: {type: string}}
        operations: {type: array, minItems: 1, uniqueItems: true, items: {type: string, enum: [read, write]}}
        tables:
          type: array
          items:
            type: object
            required: [id, uri, metadata, folder]
            properties:
              id: {type: string}
              uri: {type: string}
              metadata: {type: string}
              folder: {type: string}
        temporary_credentials:
          type: object
          required: [access_key_id, secret_access_key, session_token]
          properties:
            access_key_id: {type: string}
            secret_access_key: {type: string}
            session_token: {type: string}
    LeaseStorage:
      type: array
      description: Exactly one scoped descriptor per location needed by the run.
      items: {$ref: '#/components/schemas/LeaseStorageAccess'}
    StorageSecret:
      type: object
      writeOnly: true
      additionalProperties: false
      required: [kind, access_key_id, secret_access_key]
      properties:
        kind: {type: string, enum: [r2, sts]}
        access_key_id: {type: string, writeOnly: true, minLength: 1}
        secret_access_key: {type: string, writeOnly: true, minLength: 1}
        r2_api_token: {type: string, writeOnly: true, minLength: 1, description: Required only for R2}
    CreateStorageCredential:
      type: object
      additionalProperties: false
      required: [name, provider, endpoint, region]
      properties:
        name: {type: string}
        provider: {type: string, enum: [aws, r2, minio, rustfs]}
        endpoint: {type: string, format: uri}
        region: {type: string}
        role_arn: {type: string, description: Required for AWS; rejected for other providers}
        secret: {$ref: '#/components/schemas/StorageSecret'}
    StorageCredential:
      type: object
      additionalProperties: false
      required: [id, name, provider, endpoint, region, generation, state]
      properties:
        id: {type: string}
        name: {type: string}
        provider: {type: string, enum: [aws, r2, minio, rustfs]}
        endpoint: {type: string, format: uri}
        region: {type: string}
        role_arn: {type: string}
        external_id: {type: string, description: AWS trust-policy value, not a secret}
        generation: {type: integer, format: int64}
        state: {type: string, enum: [active, disabled]}
```

On creation, `secret` is required for `r2`, `minio` and `rustfs`; AWS instead requires `role_arn` and rejects `secret`. R2 requires `r2_api_token`; STS credentials reject it. The server enforces these rules and matching immutable provider on rotation. AWS rotation changes the role configuration through a separate admin update with the same generation and fencing rules; `PUT .../secret` is not available for AWS. List/get/rotation/check never serialize secret properties, even as null or masked values. No request body, error, or telemetry includes them.

The existing location create body is `{name, credential_id, bucket, prefix}`; its non-secret response adds `id`, `state`, `generation`, and provider-verified canonical store identity. Catalog creation accepts a location ID rather than inferring location from `folder`. Table/volume creation adds `storage: {mode: managed|external, location_id, path}`; managed objects omit caller path and inherit the catalog location, external objects require both location ID and path. Iceberg REST load returns the standard single-table response and that table’s location-scoped credential; agent/serverless work orders carry **a list of per-location** storage descriptors; any client unable to use multiple locations gets an explicit unsupported response. The grant endpoints above replace string-encoded principals in existing securable grant requests/responses with `PrincipalRef`. These public shape changes are coordinated by the lead before implementation; update every route, OpenAPI schema and consumer together.

## 3. Database contract to agree

The following is the **target schema delta**, expressed in portable SQL for the existing schema, not a migration to run from this document. The lead edits the unfrozen baseline before beta; after freeze the same logical changes use new numbered migrations and a real data backfill. Exact backfill must run before making columns non-null and before old columns are removed; both SQLite and Postgres are tested. Use transactions and typed validation in the service in addition to these constraints.

```sql
-- One stable identity for each organization person/group, and each workspace service principal.
CREATE TABLE principals (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('person', 'group', 'service')),
    subject TEXT NOT NULL,
    workspace_id TEXT NOT NULL DEFAULT '',
    CHECK ((kind = 'service' AND workspace_id <> '') OR
           (kind IN ('person', 'group') AND workspace_id = '')),
    UNIQUE (org_id, kind, workspace_id, subject)
);

-- Replace the old string-principal grants after converting every row and validating counts.
CREATE TABLE principal_grants (
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL REFERENCES principals(id),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('securable', 'credential', 'location', 'compute', 'metastore')),
    target_id TEXT NOT NULL,
    privilege TEXT NOT NULL,
    granted_by_principal_id TEXT NOT NULL REFERENCES principals(id),
    created_ms BIGINT NOT NULL,
    PRIMARY KEY (org_id, workspace_id, principal_id, target_kind, target_id, privilege)
);
CREATE INDEX principal_grants_target ON principal_grants (org_id, workspace_id, target_kind, target_id);

-- Before setting these NOT NULL, backfill each existing catalog/object to a reserved default location.
ALTER TABLE securables ADD COLUMN location_id TEXT NOT NULL DEFAULT '';
ALTER TABLE securables ADD COLUMN storage_mode TEXT NOT NULL DEFAULT 'managed';
ALTER TABLE storage_locations ADD COLUMN provider_identity TEXT NOT NULL DEFAULT '';
ALTER TABLE storage_locations ADD COLUMN state TEXT NOT NULL DEFAULT 'active';
ALTER TABLE storage_locations ADD COLUMN generation BIGINT NOT NULL DEFAULT 1;
ALTER TABLE storage_credentials ADD COLUMN provider TEXT NOT NULL DEFAULT '';
ALTER TABLE storage_credentials ADD COLUMN secret_generation BIGINT NOT NULL DEFAULT 1;
ALTER TABLE storage_credentials ADD COLUMN vault_bundle_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE storage_credentials ADD COLUMN external_id TEXT NOT NULL DEFAULT '';
ALTER TABLE storage_credentials ADD COLUMN state TEXT NOT NULL DEFAULT 'active';
ALTER TABLE compute_targets ADD COLUMN owner_principal_id TEXT NOT NULL DEFAULT '';

-- Durable coordination across DB and OpenBao: no secret body is written here.
CREATE TABLE storage_secret_tasks (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    credential_id TEXT NOT NULL,
    generation BIGINT NOT NULL,
    secret_ref TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('delete_orphan', 'delete_old')),
    state TEXT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    next_attempt_ms BIGINT NOT NULL DEFAULT 0,
    lease_until_ms BIGINT NOT NULL DEFAULT 0,
    UNIQUE (credential_id, generation, operation)
);
CREATE TABLE storage_authorization_epochs (
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    epoch BIGINT NOT NULL,
    PRIMARY KEY (org_id, workspace_id)
);
```

At final cutover remove `storage_credentials.key_id_ref`, `secret_ref` **old per-field meanings** and `issuer_token_ref`, replacing them with the new single server-generated `vault_bundle_ref` shown above. Rebuild the table on SQLite to drop the legacy columns only after backfill; retain only the bundle reference and never accept one from a client. Preserve `role_arn` as non-secret configuration; create a unique external ID per AWS credential. Do not store key ID or a hash/fingerprint of a key in the database. Make `location_id`, storage mode, principal/target scope and referential checks mandatory after backfill; catalog/schema inheritance and view/no-storage cases must be explicitly represented instead of accepting empty IDs for file-bearing objects. A transaction/lock for canonical bucket identity plus prefix is required for the overlap invariant; a simple SQL `UNIQUE` on the exact prefix is insufficient.

Replace principal strings in `workspace_members`, `account_admins`, `entitlements`, `securables.owner`, run actors, service-principal references and grant/audit mappings as a coordinated migration. Preserve the content and verification of existing audit hash chains; do not rewrite hashed historic entries. Secrets and their references are removed only after the new generation is active and durable cleanup is recorded. In-flight runs at cutover are drained or cancelled before schema switch.

## 4. Test-first acceptance

Write these tests **first**, watch the relevant new tests fail for the intended missing behavior, then implement. Run database scenarios on SQLite and Postgres; use isolated provider accounts/containers for live tests and a fake vault/provider only for failure injection.

1. Secret create, rotate, list, get, check and error tests use unique canary values; assert no key, key ID, token, request body or vault reference is present in responses, logs, traces, audit, database rows or idempotency records. Only the intended OpenBao path contains the secret; no API can read it back. Include wrong org, workspace, service principal and non-admin callers.
2. Inject vault-write, DB-commit, duplicate idempotency, crash and timeout at every rotation boundary; assert exactly one active generation, transactional audit, safe retries, orphan cleanup and no old-secret restoration. Check that the verification endpoint returns only its documented status, never an upstream error or secret, and performs no destructive probe.
3. Two locations, two buckets and two credentials in different organizations work for independent tables, volumes, Iceberg commits, lifecycle tasks and runs; same object names do not collide. Deny missing/foreign/suspended location, wrong bucket, stale generation and any fallback to default. A multi-location run receives only distinct per-location access.
4. Race equal and nested location/object registrations on two server instances and test prefix aliases, encoded traversal, default roots, deletion locks and unverified DNS aliases. Exactly one overlapping registration can win; unrelated sibling prefixes succeed. Test DNS rebinding, redirects, cloud metadata addresses and customer-controlled endpoint strings against the production egress policy.
5. AWS trust policy includes the exact installation principal and tenant-specific external ID; wrong principal/external ID fails. Test provider role policy and session policy separately: intended object works, sibling prefix and writes with a read-only session fail; 900-second minimum and one-hour role-chaining ceiling are handled. Test SSE-KMS refusal until separately configured and approved.
6. Against real supported RustFS, MinIO and R2 installations, issue short-lived read and write credentials; assert permitted object works and sibling prefix, other bucket and extra operations fail. Exercise expiry, unsupported session policies, provider timeout, key rotation, malformed responses and maximum TTL. A mock cannot be the only proof of a provider's enforcement.
7. Managed drop deletes and verifies only its own prefix, survives crash/retry and handles versioning/retention honestly. External drop, catalog drop and erase preserve all external bytes and metadata. Reject external metadata referencing another prefix and path overlap, including pending deletion.
8. Exercise every privilege in the table: owner, workspace admin, ordinary person, group, service principal and delegated app; include insufficient catalog/schema grants, raw file access with row masks, denial on policy addition, foreign workspace and 404 concealment. A table grant alone does not permit raw location access.
9. Submit, lease and complete runs while revoking SELECT, `USE_COMPUTE`, a location grant or rotating/disabling credentials. Assert cross-replica epoch rechecks, cache invalidation even when notifications are lost, failed renewal, cancellation, stale-attempt fencing, hidden results, provider credential lifetime and the explicit inability to recall previously delivered bytes.
10. Convert representative legacy `user:`, `group:` and `sp:` data to the typed principal schema on SQLite and Postgres; compare rights, group and delegated intersections, workspace confinement, compute grants, ownership and audit-chain verification. Fail safely on malformed, duplicate or ambiguous identities.
11. Freeze a real beta baseline and verify a forward-only upgrade, checksum mismatch detection, backup/restore, crash rollback and successful sign-in on both databases. No edited previously applied migration is accepted.
12. Verify all changed router paths against OpenAPI, validate generated client shapes and write-only schemas, and exercise the real console-to-vault-to-customer-store flow on RustFS and R2. The lead runs the full wave checks under `scripts/heavy.sh`; these proposed tests are not evidence that the implementation exists.

## 5. Lead amendments (14 Nov 2026)

Agreed with these changes, for a beta of one server and fewer than three customers:

1. **No principal migration.** Section 1.7's `principals` and `principal_grants` tables are not built. The code gets one `Principal` type
   that wraps the existing `user:`, `group:` and `sp:` strings, and `credential`, `location` and `compute` become new kinds of grant
   target in the existing grants table. Migrations still freeze at the beta.
2. **No multi-replica machinery.** The beta runs one server. A single in-process generation check against the database replaces the
   epoch table and the notification between replicas; it can be added when there is a second replica.
3. **Simpler rotation.** One OpenBao KV v2 path per credential (server-generated). Rotation writes a new vault version, then, in one
   transaction with the audit entry, compare-and-swaps the generation and the pinned vault version, then destroys the old version.
   A failed commit leaves an unreferenced version that the next rotation destroys. `storage_secret_tasks` and idempotency records are
   not built.
4. **AWS needs a Loamz AWS identity** (an IAM user in an AWS account owned by Loamz) to assume the customer's role; until the owner
   provides one, AWS is documented as not yet supported. R2 and MinIO/RustFS come first. Only providers with a passing conformance
   test are offered.
5. **Split of part 2.** Codex: key custody, `build_location`, endpoint safety (2a). Lead: the per-location registry, columns, managed and
   external objects, storage grants and `USE_COMPUTE` (2b).

