# SQL Editor Console

The SQL Editor provides an interactive query environment for exploring datasets, verifying queries, and running transformations.

---

## Running Queries

1. Open **SQL** from the workspace sidebar.
2. Select your target catalog context from the top selector or qualify your tables using `catalog.schema.table` syntax.
3. Type your SQL statement. The editor features:
   - Syntax highlighting and autocomplete for catalog tables and columns.
   - Query cancellation and timeout controls.
   - Result pagination up to 1,000 rows in the visual table.
4. Press `Cmd+Enter` (macOS) or `Ctrl+Enter` (Linux/Windows) to run.

---

## Query History

Every statement executed in the workspace is recorded with full auditability:
- **Timestamp & Duration**: `queued_ms`, execution `run_ms`, and finished time.
- **Requester**: Who executed the query (person email or service principal).
- **Execution Target**: Which compute runner or serverless worker processed the query.
- **Results & Errors**: Output rows or detailed compiler error diagnostics.

---

## Exporting Results

Query results can be exported directly from the results pane:
- **Download CSV**: For spreadsheet analysis.
- **Download JSON**: Structured rows with column type metadata.
- **Python Snippet**: Generates code using the `loamz` SDK or DuckDB connection to reproduce the query in scripts.
