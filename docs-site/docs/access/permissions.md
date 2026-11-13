# Permissions and Governance

Loamz Data implements fine-grained, inheritance-based access control across all catalog resources.

---

## Privileges

Access is granted at the catalog, schema, or table level:

| Privilege | Applicable To | Description |
|---|---|---|
| `SELECT` | Table, Schema, Catalog | Allows reading table rows and fetching Iceberg snapshots. |
| `MODIFY` | Table, Schema, Catalog | Allows inserting, updating, or deleting rows in tables. |
| `CREATE_TABLE` | Schema, Catalog | Allows creating new tables under the specified schema. |
| `CREATE_SCHEMA` | Catalog | Allows creating new schemas under the catalog. |
| `DROP` | Table, Schema, Catalog | Allows deleting the specified resource. |

---

## Permission Inheritance

Privileges assigned to parent containers automatically inherit down to child resources:
- Granting `SELECT` on catalog `nyc` grants read access to all schemas and tables inside `nyc`.
- Granting `MODIFY` on schema `nyc.main` allows writing to all existing and future tables created within `nyc.main`.

---

## The Zero-Enumeration Rule

Security by design: If a caller attempts to view, read, or query a catalog, schema, or table they do not possess permissions for, Loamz Data returns `404 Not Found` rather than `403 Forbidden`. This prevents attackers or unauthorized users from enumerating confidential dataset names within the organization.

---

## Audit Log

Every permission grant, revocation, data load, and query execution is appended to an immutable audit chain. Workspace administrators can inspect audit records from the console or via `GET /api/v1/workspaces/{id}/audit`.
