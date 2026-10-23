"""SCIM 2.0 provisioning through a tenant administrator's access token, against an in-memory Keycloak."""

from __future__ import annotations

import httpx
import pytest

from loamz.core import tenancy
from loamz.core.tenancy import TENANT_ADMIN_GROUP
from loamz.data import DataClient
from loamz.server.data.features.scim.api import directory
from loamz.server.identity.directory import Directory
from tests.data.fake_keycloak import FakeKeycloak

A, B = "acme", "globex"
USER = "urn:ietf:params:scim:schemas:core:2.0:User"


@pytest.fixture
def kc(docs):
    fake = FakeKeycloak()
    fake.add_org(A)
    fake.add_org(B)
    docs.app.state.container.put(
        directory,
        Directory("http://kc", "loamz", "agent-membership", "s", transport=fake.transport()),
    )
    return fake


@pytest.fixture
def scim(docs, kc):
    """An HTTP client holding a tenant administrator's personal access token."""
    with DataClient(
        docs.url,
        credential=lambda: docs.identity.token(
            "scim-admin", (TENANT_ADMIN_GROUP,), organization=[A]
        ),
    ) as admin:
        token = admin.request("POST", "/access-tokens", json={"name": "okta"})["token"]
    with httpx.Client(
        base_url=docs.url + "/scim/v2", headers={"Authorization": f"Bearer {token}"}
    ) as http:
        yield http


def new_user(http, user_name, **extra):
    return http.post(
        "/Users",
        json={
            "schemas": [USER],
            "userName": user_name,
            "emails": [{"value": f"{user_name}@x.io", "primary": True}],
            **extra,
        },
    )


def test_only_the_tenants_administrator_may_provision(docs, kc):
    anonymous = httpx.get(docs.url + "/scim/v2/Users")
    assert anonymous.status_code == 401 and anonymous.headers["content-type"].startswith(
        "application/scim+json"
    )
    with DataClient(
        docs.url, credential=lambda: docs.identity.token("bob", (), organization=[A])
    ) as bob:
        token = bob.request("POST", "/access-tokens", json={"name": "plain"})["token"]
    denied = httpx.get(docs.url + "/scim/v2/Users", headers={"Authorization": f"Bearer {token}"})
    assert denied.status_code == 403 and denied.json()["schemas"] == [
        "urn:ietf:params:scim:api:messages:2.0:Error"
    ]


def test_users_are_created_read_filtered_changed_and_removed(scim, kc):
    created = new_user(
        scim, "ada", name={"givenName": "Ada", "familyName": "Lovelace"}, externalId="ext-1"
    )
    assert created.status_code == 201 and created.headers["location"].endswith(
        f"/Users/{created.json()['id']}"
    )
    ada = created.json()
    assert ada["userName"] == "ada" and ada["active"] and ada["name"]["formatted"] == "Ada Lovelace"
    assert ada["emails"][0]["value"] == "ada@x.io" and ada["externalId"] == "ext-1"
    assert ada["id"] in kc.orgs[A]
    assert new_user(scim, "ada").status_code == 409
    assert new_user(scim, "ada").json()["scimType"] == "uniqueness"

    assert scim.get(f"/Users/{ada['id']}").json()["userName"] == "ada"
    hit = scim.get("/Users", params={"filter": 'userName eq "ada"'}).json()
    assert hit["totalResults"] == 1 and hit["Resources"][0]["id"] == ada["id"]
    assert scim.get("/Users", params={"filter": 'userName eq "nobody"'}).json()["totalResults"] == 0
    assert (
        scim.get("/Users", params={"filter": 'externalId eq "ext-1"'}).json()["totalResults"] == 1
    )
    assert scim.get("/Users", params={"filter": "userName co ada"}).status_code == 400

    off = scim.patch(
        f"/Users/{ada['id']}",
        json={"Operations": [{"op": "replace", "value": {"active": False}}]},
    ).json()
    assert off["active"] is False and kc.users[ada["id"]]["enabled"] is False
    renamed = scim.patch(
        f"/Users/{ada['id']}",
        json={"Operations": [{"op": "Replace", "path": "name.givenName", "value": "Augusta"}]},
    ).json()
    assert renamed["name"]["givenName"] == "Augusta"
    replaced = scim.put(
        f"/Users/{ada['id']}",
        json={"userName": "ada", "active": True, "emails": [{"value": "a@y.io"}]},
    )
    assert replaced.json()["active"] and replaced.json()["emails"][0]["value"] == "a@y.io"

    assert scim.delete(f"/Users/{ada['id']}").status_code == 204
    assert scim.get(f"/Users/{ada['id']}").status_code == 404
    assert ada["id"] in kc.users  # the identity is kept; only the tenant membership went


def test_listing_pages_and_never_shows_another_tenant(scim, kc):
    for name in ("u1", "u2", "u3"):
        assert new_user(scim, name).status_code == 201
    kc.users["foreign"] = {"id": "foreign", "username": "mallory", "enabled": True}
    kc.orgs[B].add("foreign")
    first = scim.get("/Users", params={"startIndex": 1, "count": 2}).json()
    rest = scim.get("/Users", params={"startIndex": 3, "count": 2}).json()
    assert (
        first["totalResults"] == 3 and len(first["Resources"]) == 2 and len(rest["Resources"]) == 1
    )
    assert "mallory" not in str(first) + str(rest)
    assert scim.get("/Users/foreign").status_code == 404
    assert scim.delete("/Users/foreign").status_code == 404
    assert (
        scim.patch(
            "/Users/foreign", json={"Operations": [{"op": "replace", "value": {"active": False}}]}
        ).status_code
        == 404
    )
    assert kc.users["foreign"]["enabled"] is True


def test_groups_hold_members_and_belong_to_one_tenant(scim, kc):
    ada = new_user(scim, "ada").json()["id"]
    bob = new_user(scim, "bob").json()["id"]
    alias = tenancy.tenant_alias(A)
    made = scim.post("/Groups", json={"displayName": "analysts", "members": [{"value": ada}]})
    assert made.status_code == 201
    group = made.json()
    assert group["displayName"] == "analysts" and [m["value"] for m in group["members"]] == [ada]
    assert [g["name"] for g in kc.groups.values()] == [f"{alias}:analysts"]

    scim.patch(
        f"/Groups/{group['id']}",
        json={"Operations": [{"op": "add", "path": "members", "value": [{"value": bob}]}]},
    )
    assert {m["value"] for m in scim.get(f"/Groups/{group['id']}").json()["members"]} == {ada, bob}
    scim.patch(
        f"/Groups/{group['id']}",
        json={"Operations": [{"op": "remove", "path": f'members[value eq "{ada}"]'}]},
    )
    assert [m["value"] for m in scim.get(f"/Groups/{group['id']}").json()["members"]] == [bob]
    scim.put(
        f"/Groups/{group['id']}", json={"displayName": "scientists", "members": [{"value": ada}]}
    )
    assert scim.get(f"/Groups/{group['id']}").json()["displayName"] == "scientists"
    assert {m["value"] for m in scim.get(f"/Groups/{group['id']}").json()["members"]} == {ada}
    assert (
        scim.get("/Groups", params={"filter": 'displayName eq "scientists"'}).json()["totalResults"]
        == 1
    )

    kc.groups["other"] = {"id": "other", "name": f"{tenancy.tenant_alias(B)}:secret"}
    kc.group_members["other"] = set()
    assert scim.get("/Groups").json()["totalResults"] == 1
    assert scim.get("/Groups/other").status_code == 404
    assert scim.delete("/Groups/other").status_code == 404
    assert scim.post("/Groups", json={"displayName": "bad:name"}).status_code == 400
    kc.users["foreign"] = {"id": "foreign", "username": "mallory"}
    assert (
        scim.patch(
            f"/Groups/{group['id']}",
            json={
                "Operations": [{"op": "add", "path": "members", "value": [{"value": "foreign"}]}]
            },
        ).status_code
        == 404
    )

    assert scim.delete(f"/Groups/{group['id']}").status_code == 204
    assert scim.get("/Groups").json()["totalResults"] == 0


def test_discovery_and_an_unconfigured_server(scim, docs):
    config = scim.get("/ServiceProviderConfig").json()
    assert config["patch"]["supported"] and not config["bulk"]["supported"]
    assert {r["id"] for r in scim.get("/ResourceTypes").json()} == {"User", "Group"}


def test_scim_is_off_until_a_directory_is_configured(docs):
    docs.app.state.container._built.pop(directory, None)
    with DataClient(
        docs.url,
        credential=lambda: docs.identity.token(
            "off-admin", (TENANT_ADMIN_GROUP,), organization=[A]
        ),
    ) as admin:
        token = admin.request("POST", "/access-tokens", json={"name": "t"})["token"]
    answer = httpx.get(docs.url + "/scim/v2/Users", headers={"Authorization": f"Bearer {token}"})
    assert answer.status_code == 503 and answer.json()["schemas"][0].endswith("Error")
