"""Legal holds: erasure is refused with 423 while a hold covers the data, and everything is audited."""

from __future__ import annotations

import pytest

from loamz.core.exceptions import HttpResponseError

A = "acme"


def hold(admin, **body):
    return admin.request("POST", "/legal-holds", json={"reason": "litigation 42", **body})


def test_a_tenant_hold_blocks_erasing_anyone_in_the_tenant_until_released(docs):
    with docs.admin(A) as admin:
        placed = hold(admin, scope="tenant", tenant_id=A, case_ref="CASE-42")
        assert placed["active"] and placed["scope"] == "tenant" and placed["tenant_id"] == A
        for target in ({"scope": "tenant"}, {"scope": "user", "subject": "gina"}):
            with pytest.raises(HttpResponseError) as refused:
                admin.request("POST", "/erasure-requests", json={"tenant_id": A, **target})
            assert refused.value.status == 423 and refused.value.code == "legal-hold"
            assert "CASE-42" in str(refused.value.detail)
        listed = admin.request("GET", "/legal-holds", params={"tenant_id": A})["items"]
        assert [h["id"] for h in listed] == [placed["id"]]
        released = admin.request("POST", f"/legal-holds/{placed['id']}:release")
        assert not released["active"] and released["released_by"]
        assert admin.request("GET", "/legal-holds", params={"tenant_id": A})["items"] == []
        done = admin.request(
            "POST", "/erasure-requests", json={"tenant_id": A, "scope": "user", "subject": "gina"}
        )
        assert done


def test_a_user_hold_blocks_that_user_and_whole_tenant_erasure_but_not_others(docs):
    tenant = "initech"
    with docs.admin(tenant) as admin:
        placed = hold(admin, scope="user", subject="gina", tenant_id=tenant)
        base = {"tenant_id": tenant}
        with pytest.raises(HttpResponseError) as refused:
            admin.request(
                "POST", "/erasure-requests", json={**base, "scope": "user", "subject": "gina"}
            )
        assert refused.value.status == 423
        with pytest.raises(HttpResponseError):
            admin.request("POST", "/erasure-requests", json={**base, "scope": "tenant"})
        assert admin.request(
            "POST", "/erasure-requests", json={**base, "scope": "user", "subject": "hal"}
        )
        admin.request("POST", f"/legal-holds/{placed['id']}:release")


def test_only_administrators_place_holds_and_every_change_is_audited(docs):
    with docs.client("mallory", A) as outsider, pytest.raises(HttpResponseError) as denied:
        hold(outsider, scope="tenant", tenant_id=A)
    assert denied.value.status in (401, 403)
    with docs.admin(A) as admin:
        placed = hold(admin, scope="tenant", tenant_id=A)
        admin.request("POST", f"/legal-holds/{placed['id']}:release")
        events = admin.request("GET", "/audit-events", params={"page_size": 50})["events"]
        actions = [e["action"] for e in events if e["securable"] == placed["id"]]
        assert sorted(actions) == ["legal-hold.place", "legal-hold.release"]
        assert admin.request("GET", "/audit-events:verify")["ok"] is True


def test_history_is_not_purged_while_a_hold_is_active(docs):
    store = docs.app.state.container.store
    with docs.admin("hooli") as admin:
        placed = hold(admin, scope="tenant", tenant_id="hooli")
        assert store.purge_history(1) == {"audit_events": 0, "statements": 0}
        admin.request("POST", f"/legal-holds/{placed['id']}:release")
        assert store.any_hold() is None
