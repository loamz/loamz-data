# Encryption and customer-managed keys

Everything at rest is encrypted by default with the cloud provider's keys. To control the keys yourself (rotation,
revocation, audit in your key service), give the Terraform module for your cloud a key. The module encrypts the object
buckets and the PostgreSQL database with it and grants the services what they need to use it.

| Cloud | Variable | What it protects | What the module also does |
| --- | --- | --- | --- |
| AWS | `kms_key_arn` | S3 buckets (SSE-KMS with bucket keys), the RDS instance and its master-password secret | Adds `kms:Decrypt`, `GenerateDataKey` and `DescribeKey` on the key to the control role, and `kms:Decrypt` to the storage-vending role |
| Google Cloud | `kms_key_name` | Cloud Storage buckets and the Cloud SQL instance | Grants the Cloud Storage and Cloud SQL service agents `cryptoKeyEncrypterDecrypter` on the key |
| Azure | `key_vault_key_id`, `key_vault_id` | The storage account and the PostgreSQL flexible server | Creates one user-assigned identity and grants it *Key Vault Crypto Service Encryption User* on the vault |

On AWS, pass the same ARN to the chart (`data.storageKmsKeyArn`, already in the Terraform output `helm_values`): the
credentials the data service vends to workers for one statement then include `kms:Decrypt` on that key, so a query can read
the encrypted files and nothing else.

## Requirements and behaviour
- The key must be in the cell's region. On Azure the vault needs soft delete and purge protection.
- The key policy (AWS), the IAM binding (Google Cloud) or the vault access (Azure) must let the identities above use
  the key; the modules add the identity side.
- **Revoking or disabling the key makes the data unreadable**, including for backups encrypted with it. That is the
  point; test it in a non-production cell first.
- Existing data is not re-encrypted when you add a key to a running cell: new objects use it, and existing buckets and
  databases are rewritten by the provider according to its rules. Plan the change for a new cell where you can.
- Keys for in-transit encryption (TLS certificates, the internal CA) are separate and are described in the security
  guide.

## Chat content and stored tokens (application-level sealing)

Beyond the provider's disk encryption above, the agent server seals what people write in shared chats (text, edits, cards) and
the OAuth tokens it caches, before they reach the database. A database dump, a backup or a replica holds only
`sealed:v1:<key id>:...` values. Each chat and each person's token has its own key, derived from a master key with HKDF, and the
field's name is authenticated, so a sealed value copied to another row does not open.

- **Configure:** `LOAMZ_AGENTS_SEALING__KEYS__K1` (32+ random bytes, base64url; compose and Helm generate one as
  `CHAT_SEALING_KEY` / `chat-sealing-key`) and `LOAMZ_AGENTS_SEALING__PRIMARY`. With no key, content stays readable in the
  database: development only.
- **Back the key up.** Without it sealed content cannot be read, by anyone.
- **Rotate:** add `K2`, set it primary, redeploy. New writes use `K2`; old values still open with `K1`, which stays in the ring
  until nothing sealed with it remains.
- **Existing content** written before keys were set stays readable, and is sealed the next time it is written.

What this is not: the server process holds the keys and reads the content, and 1:1 agent conversations (the run log) are not
sealed at the application level because the server searches and summarises them; the run log relies on the disk encryption
above. This is protection at rest, not end-to-end encryption (see `docs/security/e2ee-threat-model.md`). Wrapping the master
key with your KMS is the next step.
