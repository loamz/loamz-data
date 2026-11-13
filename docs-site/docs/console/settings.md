# Workspace Settings

Workspace settings configure data storage locations, default entitlements, audit retention, and workspace identity.

---

## Workspace Configuration

- **Name & Description**: Human-readable label for team navigation.
- **Default Storage Location**: The S3-compatible bucket and prefix where new managed catalogs and tables are provisioned.
- **Organization Association**: The parent organization managing billing, subscription limits, and enterprise directory policies.

---

## Storage Credentials

Configure S3-compatible credentials used for reading and writing data:
- **Cloudflare R2, AWS S3, MinIO, or RustFS**: Provide bucket name, endpoint URL, and reference to access keys in the vault.
- **Vended Credential Strategy**: Choose between AWS STS temporary session tokens or presigned URL vending.

---

## Workspace Danger Zone

- **Archive Workspace**: Puts the workspace in read-only mode, disabling new compute runs and table mutations.
- **Delete Workspace**: Permanently deletes workspace metadata and catalog definitions after confirming all tables have been dropped.
