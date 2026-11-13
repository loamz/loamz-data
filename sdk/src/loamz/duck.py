from __future__ import annotations

from pathlib import Path
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
            f"ATTACH {_literal(self.catalog)} AS lz "
            f"(TYPE iceberg, ENDPOINT {_literal(endpoint)}, TOKEN {_literal(self.token)})"
        )
        con.execute(sql)
        return con

    def import_file(self, path: str | Path, *, format: str | None = None, append: bool = False) -> int:
        """Bring a CSV or Parquet file in as this table, and return how many rows it now holds from this file.

        The file is read on this machine and written straight to the table's storage with short-lived credentials from the
        catalog, so the data never passes through the server. The table is created from the file's columns, or, with
        ``append=True``, the rows are added to a table that exists. Needs permission to create (or modify) the table.
        """
        file = Path(path)
        if not file.is_file():
            raise FileNotFoundError(f"{file} is not a file")
        kind = (format or file.suffix.lstrip(".")).lower()
        reader = {"csv": "read_csv", "parquet": "read_parquet"}.get(kind)
        if reader is None:
            raise ValueError("format must be csv or parquet")
        con = self.duckdb()
        target = f"lz.{_ident(self.schema)}.{_ident(self.table_name)}"
        source = f"{reader}({_literal(str(file))})"
        if append:
            con.execute(f"INSERT INTO {target} SELECT * FROM {source}")
        else:
            con.execute(f"CREATE SCHEMA IF NOT EXISTS lz.{_ident(self.schema)}")
            con.execute(f"CREATE TABLE {target} AS SELECT * FROM {source}")
        row = con.execute(f"SELECT count(*) FROM {source}").fetchone()
        return int(row[0]) if row else 0


def _ident(name: str) -> str:
    """A name safe to put in SQL: quoted, with any quote doubled."""
    return '"' + name.replace('"', '""') + '"'


def _literal(text: str) -> str:
    """Text safe to put in SQL as a string value."""
    return "'" + text.replace("'", "''") + "'"
