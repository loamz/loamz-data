from __future__ import annotations

from typing import Any


class Rows:
    """Tabular query results returned by executing a SQL statement."""

    def __init__(self, columns: list[str], rows: list[list[Any]]) -> None:
        self.columns = columns
        self.rows = rows

    def __repr__(self) -> str:
        return f"Rows(columns={self.columns!r}, count={len(self.rows)})"

    def __len__(self) -> int:
        return len(self.rows)

    def __iter__(self):
        return iter(self.rows)

    def to_arrow(self):
        """Convert rows to an Apache Arrow Table."""
        try:
            import pyarrow as pa
        except ImportError as e:
            raise ImportError(
                "pyarrow is required to convert Rows to Arrow. Install it with: pip install loamz[arrow]"
            ) from e

        pydict = {col: [row[i] for row in self.rows] for i, col in enumerate(self.columns)}
        return pa.Table.from_pydict(pydict)

    def to_pandas(self):
        """Convert rows to a pandas DataFrame."""
        try:
            import pandas as pd
        except ImportError as e:
            raise ImportError(
                "pandas is required to convert Rows to DataFrame. Install it with: pip install loamz[pandas]"
            ) from e

        return pd.DataFrame(self.rows, columns=self.columns)
