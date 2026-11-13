from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    import duckdb


class TableHandle:
    """Handle to a table securable, providing native DuckDB Iceberg integration."""

    def __init__(
        self,
        full_name: str,
        *,
        workspace_id: str,
        base_url: str,
        token: str,
    ) -> None:
        self.full_name = full_name
        self.workspace_id = workspace_id
        self.base_url = base_url.rstrip("/")
        self.token = token

        parts = full_name.split(".")
        self.catalog = parts[0] if parts else ""
        self.schema = parts[1] if len(parts) > 1 else ""
        self.table_name = parts[2] if len(parts) > 2 else ""

    def __repr__(self) -> str:
        return f"TableHandle({self.full_name!r})"

    def duckdb(self) -> duckdb.DuckDBPyConnection:
        """Connect to DuckDB with the Iceberg catalog attached."""
        try:
            import duckdb
        except ImportError as e:
            raise ImportError(
                "duckdb is required to use TableHandle.duckdb(). Install it with: pip install loamz[duckdb]"
            ) from e

        con = duckdb.connect()
        con.execute("INSTALL iceberg; INSTALL httpfs; LOAD iceberg; LOAD httpfs;")
        endpoint = f"{self.base_url}/api/v1/workspaces/{self.workspace_id}/iceberg"
        # Attach the Iceberg catalog as lz or the catalog name
        sql = (
            f"ATTACH '{self.catalog}' AS lz "
            f"(TYPE iceberg, ENDPOINT '{endpoint}', TOKEN '{self.token}')"
        )
        con.execute(sql)
        return con
