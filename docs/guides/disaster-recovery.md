# Disaster recovery

What can be lost, how to bring it back, and how we prove it works.

## Objectives (design targets, not a contract)
| | Target | How |
| --- | --- | --- |
| Recovery point (data loss) | 5 minutes for the database; versions for files | Point-in-time recovery on the managed database; object-store versioning |
| Recovery time, one database | Minutes (measured weekly, see below) | Restore from snapshot or backup |
| Recovery time, a whole cell | Hours | Terraform recreates the infrastructure, then restore in the order below |
| Controller or API replica failure | Seconds | Replicas, disruption budgets and the elected controller (`operations.md`) |

## What to protect
| State | Where | Protection |
| --- | --- | --- |
| Catalog, grants, audit chain, access tokens, legal holds | PostgreSQL `loamz_data` | Managed backups and point-in-time recovery (14 days in the Terraform modules); multi-AZ option |
| Agent threads, runs, scheduled work | PostgreSQL `agentdb` | Same |
| Document and memory index | PostgreSQL `data_retrieval` | Same; can also be rebuilt from the stored documents (`reindex`) |
| Compute queue | PostgreSQL `loamz_compute` (HA) or a volume | Same; the queue is transient: jobs that were running are retried after a restore |
| Identity | PostgreSQL `keycloak` | Same; export the realm for a second copy |
| Table and file data | Object storage | Bucket versioning (volumes), cross-region replication if your recovery point needs it, optional customer-managed keys |
| Secrets and keys | Kubernetes secret, key service | Your secret manager; losing a customer-managed key makes the data unreadable by design |

## Restoring a cell, in order
1. **Infrastructure.** Re-apply the Terraform module (network, cluster, database, buckets) in the recovery region.
2. **Object storage.** Restore the buckets from replication or versions; no application step is needed.
3. **PostgreSQL.** Restore to the chosen point in time (RDS point-in-time restore, Cloud SQL clone or restore, Azure flexible
   server restore), one database at a time or the whole instance. Create the databases that are missing
   (`agentdb`, `keycloak`, `loamz_data`, `data_retrieval`, `loamz_compute`, `loamz_ui`).
4. **Secrets.** Recreate the cell secret (or `existingSecret`), including the ticket keys and, if used, point the cell at
   the same KMS key.
5. **Deploy** the chart at the version that wrote the data. The services run their schema upgrades on start.
6. **Verify.** `GET /api/v1/audit-events:verify` returns `ok: true`; list a catalog and read a file; run the end-to-end
   script against the cell.

Changes made between the recovery point and the failure are lost; the audit log shows where it ends.

## The drill
`deploy/scripts/restore_drill.py` takes a real backup of the catalog database, restores it into a scratch database, and
checks the copy with the application's own code: every governed table has the same number of rows, the audit hash chain
verifies, and an altered audit event is detected. It prints the time of each step and fails loudly on any mismatch.

- **Scheduled:** the `Restore drill` workflow runs it every week against a seeded database and publishes the timings in
  the run summary. A failing run is the signal that backups can no longer be trusted.
- **On your cell:** run it against a restored copy or a read replica before an audit or a change window:

  ```bash
  uv run --package loamz-server python deploy/scripts/restore_drill.py \
      --source postgresql://…/loamz_data --admin postgresql://…/postgres --prove-detection
  ```

  Use `pg_dump` and `pg_restore` of the server's major version.
- A full-cell exercise (steps 1 to 6 in a second region) is the operator's to schedule at least twice a year; record the
  time it took against the objectives above.
