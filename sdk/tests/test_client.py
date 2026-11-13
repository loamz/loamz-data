import pytest
from loamz.client import Client
from loamz.errors import LoamzError


def test_client_me_and_workspaces(base_url, ana_token):
    client = Client(base_url, token=ana_token)
    me = client.me()
    assert me.email == "ana@acme.test"
    assert me.kind == "person"
    assert len(me.workspaces) > 0

    workspaces = client.workspaces()
    assert any(w.name == "Main" for w in workspaces)


def test_client_workspace_selection(base_url, ana_token):
    client = Client(base_url, token=ana_token)
    ws = client.workspace("Main")
    assert ws.name == "Main"
    assert ws.id

    # Test by ID
    ws_by_id = client.workspace(ws.id)
    assert ws_by_id.id == ws.id

    # Test not found
    with pytest.raises(LoamzError) as exc_info:
        client.workspace("NonExistentWorkspace12345")
    assert exc_info.value.status == 404


def test_catalogs_and_tables(base_url, ana_token):
    client = Client(base_url, token=ana_token)
    ws = client.workspace("Main")

    catalogs = ws.catalogs()
    assert isinstance(catalogs, list)
    nyc_cat = next((c for c in catalogs if c.name == "nyc"), None)
    if nyc_cat:
        tables = ws.tables("nyc", "main")
        assert isinstance(tables, list)
        table_names = [t.name for t in tables]
        assert "trips" in table_names or "zones" in table_names


def test_table_handle(base_url, ana_token):
    client = Client(base_url, token=ana_token)
    ws = client.workspace("Main")
    handle = ws.table("nyc.main.trips")
    assert handle.catalog == "nyc"
    assert handle.schema == "main"
    assert handle.table_name == "trips"


def test_workspace_sql_real(base_url, ana_token):
    client = Client(base_url, token=ana_token)
    ws = client.workspace("Main")
    rows = ws.sql("SELECT count(*) FROM nyc.main.trips")
    assert len(rows.columns) == 1
    assert len(rows.rows) == 1
    assert rows.rows[0][0] > 0

    # Also verify lazy arrow and pandas exports
    arrow_table = rows.to_arrow()
    assert arrow_table.num_rows == 1
    df = rows.to_pandas()
    assert len(df) == 1


def test_workspace_sql_failing(base_url, ana_token):
    client = Client(base_url, token=ana_token)
    ws = client.workspace("Main")

    # Table not found (raises 404 LoamzError)
    with pytest.raises(LoamzError) as exc_info:
        ws.sql("SELECT * FROM nyc.main.non_existent_table_999")
    assert exc_info.value.status == 404

    # Execution failure: bad syntax or missing column (raises 400 LoamzError)
    with pytest.raises(LoamzError) as exc_info:
        ws.sql("SELECT * FROM nyc.main.trips WHERE non_existent_col == 1")
    assert exc_info.value.status == 400
    assert "not found" in exc_info.value.detail.lower() or "binder error" in exc_info.value.detail.lower()


def test_table_handle_duckdb(base_url, ana_token):
    duckdb = pytest.importorskip("duckdb")
    client = Client(base_url, token=ana_token)
    ws = client.workspace("Main")
    handle = ws.table("nyc.main.trips")

    con = handle.duckdb()
    result = con.execute("SELECT count(*) FROM lz.main.trips").fetchone()
    assert result is not None
    assert result[0] > 0
