"""security.txt: RFC 9116 fields, operator overrides."""

from fastapi import FastAPI
from fastapi.testclient import TestClient

from loamz.server.securitytxt import DEFAULT_CONTACT, install


def get(monkeypatch=None, **env):
    app = FastAPI()
    install(app)
    return TestClient(app).get("/.well-known/security.txt")


def test_the_default_names_the_projects_private_reporting():
    answer = get()
    assert answer.status_code == 200 and answer.headers["content-type"].startswith("text/plain")
    lines = dict(line.split(": ", 1) for line in answer.text.splitlines())
    assert (
        lines["Contact"] == DEFAULT_CONTACT and lines["Expires"].endswith("Z") and "Policy" in lines
    )


def test_an_operator_points_it_at_their_own_contact(monkeypatch):
    monkeypatch.setenv("LOAMZ_SECURITY_CONTACT", "mailto:security@example.com")
    assert "Contact: mailto:security@example.com" in get().text
