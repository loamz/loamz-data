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
