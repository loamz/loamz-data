"""Personal access tokens: shown once, hashed at rest, expiring, revocable, rotatable and never administrators."""

from __future__ import annotations

import pytest

from loamz.core.exceptions import (
    ForbiddenError,
    HttpResponseError,
    NotFoundError,
    UnauthenticatedError,
)
from loamz.data import DataClient

A = "acme"


def person(docs, name="alice", *groups):
    return DataClient(
        docs.url,
        credential=lambda: docs.identity.token(name, groups, organization=[A]),
    )


def as_token(docs, token) -> DataClient:
    return DataClient(docs.url, credential=token, retry=None)


def test_a_token_works_as_its_owner_until_revoked(docs):
    with person(docs, "pat-owner") as me:
        created = me.request("POST", "/access-tokens", json={"name": "ci"})
        assert (
            created["token"].startswith("lpat_")
            and created["active"]
            and "secret_hash" not in created
        )
        with as_token(docs, created["token"]) as script:
            assert script.catalogs()  # the owner may browse
        listed = me.request("GET", "/access-tokens")["items"]
        assert [t["id"] for t in listed] == [created["id"]] and "token" not in listed[0]
        assert listed[0]["last_used_at"]
        me.request("DELETE", f"/access-tokens/{created['id']}")
        with as_token(docs, created["token"]) as script, pytest.raises(UnauthenticatedError):
            script.catalogs()
        assert me.request("GET", "/access-tokens")["items"] == []


def test_wrong_secrets_unknown_ids_and_expired_tokens_are_rejected(docs):
    with person(docs, "pat-expiry") as me:
        created = me.request("POST", "/access-tokens", json={"name": "short", "expires_in_days": 1})
        token = created["token"]
        for bad in (token[:-3] + "xyz", "lpat_nope.secret", "lpat_"):
            with as_token(docs, bad) as script, pytest.raises(UnauthenticatedError):
                script.catalogs()
        docs.app.state.container.store.update(
            "access_tokens", created["id"], {"expires_at": "2000-01-01T00:00:00+00:00"}
        )
        with as_token(docs, token) as script, pytest.raises(UnauthenticatedError):
            script.catalogs()
        assert me.request("GET", "/access-tokens")["items"] == []
        assert me.request("GET", "/access-tokens", params={"active": "false"})["items"]


def test_tokens_cannot_make_tokens_or_act_as_administrators(docs):
    with person(docs, "pat-admin", "data-admins") as admin:
        created = admin.request("POST", "/access-tokens", json={"name": "admin-ci"})
        with as_token(docs, created["token"]) as script:
            with pytest.raises(ForbiddenError):
                script.request("POST", "/access-tokens", json={"name": "child"})
            with pytest.raises(ForbiddenError):  # the token is not an administrator
                script.request("GET", "/audit-events")
        assert admin.request("GET", "/audit-events")["events"]
    with docs.client("agent-user", A) as agent, pytest.raises(HttpResponseError) as denied:
        agent.request("POST", "/access-tokens", json={"name": "from an agent"})
    assert denied.value.status in (401, 403)


def test_a_read_only_token_cannot_change_anything(docs):
    with person(docs, "pat-reader", "data-admins") as me:
        created = me.request("POST", "/access-tokens", json={"name": "ro", "read_only": True})
        with as_token(docs, created["token"]) as script:
            assert script.catalogs()
            with pytest.raises(ForbiddenError):
                script.request(
                    "POST",
                    "/legal-holds",
                    json={"scope": "tenant", "reason": "nope", "tenant_id": A},
                )


def test_rotation_replaces_the_token_and_only_the_owner_manages_it(docs):
    with person(docs, "pat-rotator") as me, person(docs, "pat-other") as other:
        first = me.request("POST", "/access-tokens", json={"name": "deploy"})
        second = me.request("POST", f"/access-tokens/{first['id']}:rotate")
        assert second["rotated_from"] == first["id"] and second["name"] == "deploy"
        with as_token(docs, first["token"]) as script, pytest.raises(UnauthenticatedError):
            script.catalogs()
        with as_token(docs, second["token"]) as script:
            assert script.catalogs()
        with pytest.raises(NotFoundError):
            other.request("DELETE", f"/access-tokens/{second['id']}")
        with pytest.raises(NotFoundError):
            other.request("POST", f"/access-tokens/{second['id']}:rotate")


def test_the_secret_is_stored_only_as_a_hash_and_events_are_audited(docs):
    with person(docs, "pat-audit", "data-admins") as me:
        created = me.request("POST", "/access-tokens", json={"name": "audited"})
        secret = created["token"].split(".", 1)[1]
        stored = docs.app.state.container.store.get("access_tokens", created["id"])
        assert secret not in str(stored) and stored["secret_hash"] != secret
        me.request("DELETE", f"/access-tokens/{created['id']}")
        events = me.request("GET", "/audit-events", params={"page_size": 100})["events"]
        assert sorted(e["action"] for e in events if e["securable"] == created["id"]) == [
            "access-token.create",
            "access-token.revoke",
        ]
        assert secret not in str(events)
