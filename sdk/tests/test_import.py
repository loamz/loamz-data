import sys
from types import SimpleNamespace

from loamz.duck import TableHandle


def test_duckdb_attach_escapes_catalog_endpoint_and_token(monkeypatch):
    statements = []

    class Connection:
        def execute(self, statement):
            statements.append(statement)
            return self

    connection = Connection()
    fake_duckdb = SimpleNamespace(connect=lambda: connection)
    monkeypatch.setitem(sys.modules, "duckdb", fake_duckdb)

    handle = TableHandle(
        "sales'; SELECT 'injected'; --.raw.orders",
        workspace_id="workspace",
        base_url="https://data.example'; SELECT 'endpoint'; --",
        token="token'; SELECT 'token'; --",
    )
    handle.duckdb()

    assert statements[-1] == (
        "ATTACH 'sales''; SELECT ''injected''; --' AS lz "
        "(TYPE iceberg, ENDPOINT 'https://data.example''; SELECT ''endpoint''; --/api/v1/workspaces/workspace/iceberg', "
        "TOKEN 'token''; SELECT ''token''; --')"
    )


def test_import_escapes_file_path_and_target_identifiers(monkeypatch, tmp_path):
    statements = []

    class Connection:
        def execute(self, statement):
            statements.append(statement)
            return self

        def fetchone(self):
            return (1,)

    connection = Connection()
    monkeypatch.setattr(TableHandle, "duckdb", lambda self: connection)
    path = tmp_path / "data'; SELECT 'injected'; --.csv"
    path.write_text("id\n1\n")
    handle = TableHandle(
        'catalog.sch"ema; --.ta"ble',
        workspace_id="workspace",
        base_url="https://data.example",
        token="token",
    )

    assert handle.import_file(path) == 1

    assert statements[1].startswith('CREATE TABLE lz."sch""ema; --"."ta""ble" AS SELECT ')
    assert "'data''; SELECT ''injected''; --.csv'" in statements[1]
