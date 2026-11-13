from loamz.cli import main


def test_cli_whoami(base_url, ana_token, capsys, monkeypatch):
    monkeypatch.setenv("LOAMZ_URL", base_url)
    monkeypatch.setenv("LOAMZ_TOKEN", ana_token)

    exit_code = main(["whoami"])
    assert exit_code == 0
    captured = capsys.readouterr()
    assert "Logged in as: Ana Test" in captured.out
    assert "Workspaces:" in captured.out


def test_cli_whoami_json(base_url, ana_token, capsys, monkeypatch):
    monkeypatch.setenv("LOAMZ_URL", base_url)
    monkeypatch.setenv("LOAMZ_TOKEN", ana_token)

    exit_code = main(["whoami", "--json"])
    assert exit_code == 0
    captured = capsys.readouterr()
    assert '"email": "ana@acme.test"' in captured.out


def test_cli_ls(base_url, ana_token, capsys, monkeypatch):
    monkeypatch.setenv("LOAMZ_URL", base_url)
    monkeypatch.setenv("LOAMZ_TOKEN", ana_token)

    exit_code = main(["ls", "--workspace", "Main"])
    assert exit_code == 0
    captured = capsys.readouterr()
    assert "Catalog" in captured.out


def test_cli_ls_json(base_url, ana_token, capsys, monkeypatch):
    monkeypatch.setenv("LOAMZ_URL", base_url)
    monkeypatch.setenv("LOAMZ_TOKEN", ana_token)

    exit_code = main(["ls", "--workspace", "Main", "--json"])
    assert exit_code == 0
    captured = capsys.readouterr()
    assert '"kind": "catalog"' in captured.out
