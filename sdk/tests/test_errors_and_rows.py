import httpx
from loamz.errors import LoamzError
from loamz.sql import Rows


def test_loamz_error_rfc9457():
    req = httpx.Request("GET", "http://example.test")
    resp = httpx.Response(
        404,
        request=req,
        headers={"content-type": "application/problem+json"},
        json={"status": 404, "title": "Not Found", "detail": "Workspace not found"},
    )
    err = LoamzError.from_response(resp)
    assert err.status == 404
    assert err.title == "Not Found"
    assert err.detail == "Workspace not found"
    assert "Workspace not found" in str(err)


def test_loamz_error_fallback():
    req = httpx.Request("GET", "http://example.test")
    resp = httpx.Response(500, request=req, text="Server crashed")
    err = LoamzError.from_response(resp)
    assert err.status == 500
    assert err.detail == "Server crashed"


def test_rows_conversion():
    cols = ["id", "name", "active"]
    data = [[1, "apple", True], [2, "banana", False]]
    rows = Rows(columns=cols, rows=data)
    assert len(rows) == 2
    assert list(rows) == data

    # Test pandas
    try:
        import pandas as pd
        df = rows.to_pandas()
        assert isinstance(df, pd.DataFrame)
        assert list(df.columns) == cols
        assert len(df) == 2
    except ImportError:
        pass

    # Test arrow
    try:
        import pyarrow as pa
        table = rows.to_arrow()
        assert isinstance(table, pa.Table)
        assert table.column_names == cols
        assert table.num_rows == 2
    except ImportError:
        pass
