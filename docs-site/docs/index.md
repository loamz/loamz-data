# Loamz Data Documentation

Loamz Data is a governed data platform designed for modern analytics and AI. Built on open standards including Apache Iceberg and Parquet, it decouples storage from compute so your data stays in your own S3-compatible object storage while any query engine—from DuckDB to Apache Spark—reads and writes through a centralized, secure REST catalog.

With fine-grained access control, tamper-evident audit logging, and automated credential vending, teams can collaborate safely across shared workspaces. AI agents, automated pipelines, and BI dashboards execute queries seamlessly under strict human-in-the-loop policies without locking your organization into proprietary storage formats.

---

## Quick Links

- [First Steps](start/first-steps.md): Sign in, explore the catalog, and query NYC taxi trips.
- [Connect DuckDB](connect/duckdb.md): Attach Iceberg tables directly to local DuckDB sessions.
- [Python SDK & CLI](connect/python.md): Programmatic workspace operations and SQL automation.
- [API Reference](api/index.md): Complete specification for the versioned REST API.
- [Access & Governance](access/permissions.md): Permissions, service principals, and audit controls.
