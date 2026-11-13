# Data Catalog Console

The Catalog console gives you a unified view of your organization's datasets, schemas, and table schemas.

---

## Structure

Loamz Data organizes tables into a 3-tier hierarchy:

1. **Catalogs**: The highest-level container (e.g. `nyc`, `sales`, `analytics`). Catalogs can be marked `local` (stored in your default workspace bucket) or external.
2. **Schemas**: Logical namespaces within a catalog (e.g. `main`, `raw`, `staging`).
3. **Tables**: Managed Apache Iceberg tables containing Parquet data files and versioned metadata snapshots.

---

## Managing Tables

When viewing a table in the console, you can inspect:
- **Columns & Data Types**: Column names, Iceberg physical types, and nullability.
- **Data Location**: The underlying object store URI (e.g. `s3://loamz-data/workspaces/<id>/nyc/main/trips/`).
- **Snapshots & History**: The table's commit history, timestamped snapshots, and row counts.
- **Permissions**: Which team members, groups, and service principals have `SELECT` or `MODIFY` grants on this table.

---

## Creating Catalogs and Schemas

Workspace administrators can create new catalogs and schemas directly in the console:
1. Click **+ Add Catalog** in the catalog tree.
2. Provide a name (letters, numbers, and underscores).
3. Under the catalog, click **+ Add Schema** to define namespaces for your data pipelines.
