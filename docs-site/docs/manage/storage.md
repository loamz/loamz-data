# Storage Locations and Credentials

Loamz Data supports bringing external cloud object storage—such as AWS S3, Cloudflare R2, MinIO, or RustFS—for catalogs, managed tables, external tables, and volumes. Storage management decouples authentication credentials from bucket locations while enforcing strict write-only custody.

---

## Architecture: Credentials vs. Locations

Storage resources are divided into two distinct objects:
- **Storage Credential**: Holds the provider authentication settings and secret custody. Secret keys are written directly into OpenBao and are never stored in the relational database or returned to clients.
- **Storage Location**: Defines a specific cloud storage bucket and subfolder prefix (e.g. `s3://acme-analytics/production/data/`) associated with an authorized credential.

Catalogs, schemas, tables, and volumes declare an explicit location ID and relative path. Prefix overlap checks prevent different workspaces or catalogs from claiming overlapping storage folders.

---

## Supported Providers

### 1. AWS S3
For AWS S3, Loamz Data uses workload identity federation and AWS Security Token Service (STS):
- The customer creates an IAM role in their AWS account with a trust policy referencing Loamz's server role and an installation-assigned unique `sts:ExternalId`.
- Customers do **not** provide static AWS root access keys.
- At runtime, Loamz assumes the customer role and applies an intersecting STS session policy scoped strictly to the authorized table or volume folder.

### 2. Cloudflare R2
- The customer provides an R2 parent access key and secret key scoped to the target bucket, along with a Cloudflare API token with permission to issue temporary R2 credentials.
- Secrets are stored in OpenBao. Workspace compute jobs receive short-lived, prefix-scoped R2 temporary credentials (15-minute maximum lifetime).

### 3. MinIO and RustFS
- The customer provides dedicated service account credentials (access key and secret key) and verified HTTPS S3 and STS endpoints.
- Loamz requests short-lived STS session tokens with scoped folder policies for sandboxed compute runners.

---

## Write-Only Secret Custody

All storage credentials adhere to strict confidentiality standards:
- **Write-Only Input**: Secret material enters only through authenticated, TLS-encrypted `POST` or `PUT` requests and is written directly to OpenBao KV v2.
- **Database Insulation**: The relational database stores only a server-generated secret reference string, the current generation number, and non-secret connection metadata (name, S3 endpoint URL, region, and IAM role ARN).
- **Zero Secret Egress**: API endpoints (`GET`, list), server logs, crash reports, and audit trails **never** return or display key IDs, access keys, session tokens, or vault secrets.

---

## Safe Connection Verification

Before deploying tables to a storage location, workspace administrators can verify connectivity without revealing secrets or storing temporary credentials:

```sh
curl -X POST "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/storage/credentials/<credential-id>/check" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "location_id": "<location-id>"
  }'
```

The server performs a non-destructive probe against the location's bucket prefix and returns a simple status result:
- `usable`: The credential successfully authenticated and accessed the location prefix.
- `unavailable`: The storage endpoint could not be reached (DNS failure, network timeout, or connection refusal).
- `denied`: Authentication failed or provider IAM policies refused access to the prefix.

---

## Zero-Downtime Secret Rotation

Storage credentials support safe, transactional secret rotation without interrupting active workloads:

```sh
curl -X PUT "http://127.0.0.1:8020/api/v1/workspaces/<workspace-id>/storage/credentials/<credential-id>/secret" \
  -H "Authorization: Bearer $TOKEN" \
  -H "If-Match: <current-generation>" \
  -H "Content-Type: application/json" \
  -d '{
    "key_id": "<new-key-id>",
    "secret": "<new-secret-key>"
  }'
```

Rotation workflow:
1. The client supplies `If-Match` with the expected current generation integer.
2. The server stages generation N+1 in OpenBao.
3. In a database transaction, the active reference is updated, the generation counter increments, and an audit entry is committed.
4. Cached provider clients across all server replicas are invalidated. Existing running jobs continue using their existing short-lived vended sessions until expiry.

---

## Vended Credentials for Compute

Customer compute machines and sandboxed workers never receive long-lived storage credentials:
- When a job or query is scheduled, governance verifies table permissions, row filters, and column masks.
- The control plane vends an ephemeral, folder-scoped credential (typically 10 to 15 minutes lifetime) restricted strictly to the required table or volume path.
- The compute machine accesses object storage directly via governed loopback doors.

---

## What is Not Built Yet

- **Console UI for Credential Rotation**: Registering storage credentials and performing secret rotation are executed via the REST API; interactive rotation forms in the web console are not built.
- **Granular Storage Grants**: Standalone privileges (`CREATE_EXTERNAL_LOCATION`, `READ_FILES`, `WRITE_FILES`) separate from workspace admin rights are not built; storage locations and credentials are currently managed by workspace admins.
- **Non-S3 Storage Protocols**: Native Azure Data Lake Storage Gen2 (ADLS) and Google Cloud Storage (GCS) APIs are not built; external storage must expose an S3-compatible interface or AWS STS compatible endpoint.
- **Live Location Migration**: Moving existing tables between storage locations requires creating a new table and executing a data copy; in-place metadata relocation across physical locations is not built.
