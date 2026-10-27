"""/scim/v2: SCIM 2.0 provisioning of a tenant's people and groups from an enterprise identity provider.

Point the identity provider (Okta, Entra ID, OneLogin, JumpCloud ...) at `https://<data host>/scim/v2` with a bearer
token: a personal access token (`/api/v1/access-tokens`) created by the tenant's administrator. Everything is scoped to
that administrator's tenant. Users are the tenant's Keycloak organization members and groups are realm groups
prefixed with the tenant alias (see `loamz.identity.server.directory`).

Behaviour worth knowing: `active=false` disables the user's sign-in; `DELETE /Users/{id}` removes the person from this
tenant but keeps their identity (they may belong to other tenants). Responses and errors use the SCIM media type and
message formats.
"""

from __future__ import annotations

import re
from typing import Annotated, Any

from fastapi import APIRouter, Depends, Request, Response
from fastapi.responses import JSONResponse
from starlette.concurrency import run_in_threadpool

from loamz.core.tenancy import TENANT_ADMIN_GROUP
from loamz.engine.errors import (
    ForbiddenError,
    InvalidRequestError,
    ServiceUnavailableError,
)
from loamz.server.data.kernel.container import Container
from loamz.server.data.kernel.http import Caller, container
from loamz.identity.server.directory import Directory

MEDIA = "application/scim+json"
USER = "urn:ietf:params:scim:schemas:core:2.0:User"
GROUP = "urn:ietf:params:scim:schemas:core:2.0:Group"
LIST = "urn:ietf:params:scim:api:messages:2.0:ListResponse"
PATCH = "urn:ietf:params:scim:api:messages:2.0:PatchOp"
MAX_PAGE = 200


def directory(c: Container) -> Directory:
    s = c.settings
    if not s.directory_url:
        raise ServiceUnavailableError("SCIM provisioning is not configured on this server")
    found = Directory(
        s.directory_url,
        s.directory_realm,
        s.directory_client_id,
        s.directory_client_secret.get_secret_value(),
    )
    c.on_close(found.close)
    return found


# The path is fixed by the SCIM standard, so these routes stay out of the versioned OpenAPI contract.
router = APIRouter(prefix="/scim/v2", tags=["scim"], include_in_schema=False)


class Tenant:
    """The tenant of the calling administrator and the directory to manage it in."""

    def __init__(self, id: str, directory: Directory, base: str) -> None:
        self.id, self.directory, self.base = id, directory, base


def tenant_of(
    caller: Caller, request: Request, services: Annotated[Container, Depends(container)]
) -> Tenant:
    if not caller.tenant_id:
        raise ForbiddenError("SCIM needs a token that belongs to a tenant")
    if TENANT_ADMIN_GROUP not in caller.groups:
        raise ForbiddenError("Only the tenant's administrator may provision users and groups")
    if caller.scopes is not None:
        raise ForbiddenError("A delegated token may not provision users and groups")
    return Tenant(
        caller.tenant_id, services.get(directory), str(request.base_url).rstrip("/") + "/scim/v2"
    )


Scoped = Annotated[Tenant, Depends(tenant_of)]


def scim(body: Any, status: int = 200, location: str | None = None) -> JSONResponse:
    headers = {"Location": location} if location else None
    return JSONResponse(body, status_code=status, media_type=MEDIA, headers=headers)


# ------------------------------------------------------------------ mapping
def user_out(t: Tenant, user: dict[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {
        "schemas": [USER],
        "id": user["id"],
        "userName": user["username"],
        "active": bool(user.get("enabled", True)),
        "meta": {"resourceType": "User", "location": f"{t.base}/Users/{user['id']}"},
    }
    first, last = user.get("firstName"), user.get("lastName")
    if first or last:
        out["name"] = {
            "givenName": first or "",
            "familyName": last or "",
            "formatted": " ".join(p for p in (first, last) if p),
        }
        out["displayName"] = out["name"]["formatted"]
    if user.get("email"):
        out["emails"] = [{"value": user["email"], "primary": True, "type": "work"}]
    if (user.get("attributes") or {}).get("externalId"):
        out["externalId"] = user["attributes"]["externalId"][0]
    return out


def user_in(body: dict[str, Any]) -> dict[str, Any]:
    """SCIM user attributes -> Keycloak user fields (only those present)."""
    values: dict[str, Any] = {}
    if "userName" in body:
        values["username"] = body["userName"]
    name = body.get("name") or {}
    if "givenName" in name:
        values["firstName"] = name["givenName"]
    if "familyName" in name:
        values["lastName"] = name["familyName"]
    emails = body.get("emails") or []
    primary = next((e for e in emails if e.get("primary")), emails[0] if emails else None)
    if primary:
        values["email"] = primary["value"]
    if "active" in body:
        values["enabled"] = bool(body["active"])
    if "externalId" in body:
        values["attributes"] = {"externalId": [str(body["externalId"])]}
    return values


def group_out(
    t: Tenant, group: dict[str, Any], members: list[dict[str, Any]] | None = None
) -> dict[str, Any]:
    out: dict[str, Any] = {
        "schemas": [GROUP],
        "id": group["id"],
        "displayName": group["name"].split(":", 1)[1],
        "meta": {"resourceType": "Group", "location": f"{t.base}/Groups/{group['id']}"},
    }
    if members is not None:
        out["members"] = [
            {
                "value": m["id"],
                "display": m.get("username", ""),
                "$ref": f"{t.base}/Users/{m['id']}",
            }
            for m in members
        ]
    return out


FILTER = re.compile(
    r'^\s*(userName|externalId|displayName|emails\.value)\s+eq\s+"([^"]*)"\s*$', re.I
)


def parse_filter(text: str | None) -> tuple[str, str] | None:
    if not text:
        return None
    found = FILTER.match(text)
    if not found:
        raise InvalidRequestError('Only `<attribute> eq "value"` filters are supported')
    return found.group(1).lower(), found.group(2)


def page_window(start: int, count: int) -> tuple[int, int]:
    start = max(start, 1)
    return start, min(max(count, 0), MAX_PAGE)


def listing(items: list[dict[str, Any]], total: int, start: int) -> JSONResponse:
    return scim(
        {
            "schemas": [LIST],
            "totalResults": total,
            "startIndex": start,
            "itemsPerPage": len(items),
            "Resources": items,
        }
    )


# ------------------------------------------------------------------ discovery
@router.get("/ServiceProviderConfig")
def service_provider_config(_: Scoped):
    return scim(
        {
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
            "patch": {"supported": True},
            "bulk": {"supported": False, "maxOperations": 0, "maxPayloadSize": 0},
            "filter": {"supported": True, "maxResults": MAX_PAGE},
            "changePassword": {"supported": False},
            "sort": {"supported": False},
            "etag": {"supported": False},
            "authenticationSchemes": [
                {
                    "type": "oauthbearertoken",
                    "name": "Access token",
                    "description": "A personal access token",
                }
            ],
        }
    )


@router.get("/ResourceTypes")
def resource_types(_: Scoped):
    return scim(
        [
            {
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
                "id": "User",
                "name": "User",
                "endpoint": "/Users",
                "schema": USER,
            },
            {
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
                "id": "Group",
                "name": "Group",
                "endpoint": "/Groups",
                "schema": GROUP,
            },
        ]
    )


# ------------------------------------------------------------------ users
@router.get("/Users")
async def list_users(t: Scoped, filter: str | None = None, startIndex: int = 1, count: int = 100):
    wanted = parse_filter(filter)
    start, size = page_window(startIndex, count)

    def run() -> JSONResponse:
        if wanted and wanted[0] in {"username", "emails.value"}:
            found = t.directory.members(t.id, first=0, count=2, username=wanted[1])
            found = [
                u
                for u in found
                if u["username"].lower() == wanted[1].lower() or u.get("email") == wanted[1]
            ]
            return listing([user_out(t, u) for u in found], len(found), 1)
        if wanted:
            everyone = t.directory.members(t.id, first=0, count=10_000)
            key = {
                "externalid": lambda u: ((u.get("attributes") or {}).get("externalId") or [""])[0]
            }.get(wanted[0], lambda u: u.get("username", ""))
            found = [u for u in everyone if key(u) == wanted[1]]
            return listing([user_out(t, u) for u in found[:size]], len(found), 1)
        page = t.directory.members(t.id, first=start - 1, count=size)
        return listing([user_out(t, u) for u in page], t.directory.count_members(t.id), start)

    return await run_in_threadpool(run)


@router.get("/Users/{user_id}")
async def get_user(user_id: str, t: Scoped):
    return scim(user_out(t, await run_in_threadpool(t.directory.user, t.id, user_id)))


@router.post("/Users")
async def create_user(body: dict[str, Any], t: Scoped):
    values = user_in(body)
    if not values.get("username"):
        raise InvalidRequestError("userName is required")
    values.setdefault("enabled", True)
    created = await run_in_threadpool(t.directory.create_user, t.id, values)
    return scim(user_out(t, created), 201, f"{t.base}/Users/{created['id']}")


@router.put("/Users/{user_id}")
async def replace_user(user_id: str, body: dict[str, Any], t: Scoped):
    values = user_in(body)
    values.setdefault("enabled", True)
    return scim(
        user_out(t, await run_in_threadpool(t.directory.update_user, t.id, user_id, values))
    )


def flatten(operations: list[dict[str, Any]]) -> dict[str, Any]:
    """PATCH operations on a user -> one SCIM attribute document (add and replace only)."""
    document: dict[str, Any] = {}
    for op in operations:
        verb = str(op.get("op", "")).lower()
        if verb not in {"add", "replace"}:
            raise InvalidRequestError(f"Operation {verb!r} is not supported on users")
        path, value = op.get("path"), op.get("value")
        if path is None and isinstance(value, dict):
            document.update(value)
        elif path:
            head, _, tail = path.partition(".")
            if tail:
                document.setdefault(head, {})[tail] = value
            elif path.startswith("emails"):
                document["emails"] = (
                    [{"value": value, "primary": True}] if isinstance(value, str) else value
                )
            else:
                document[path] = value
    return document


@router.patch("/Users/{user_id}")
async def patch_user(user_id: str, body: dict[str, Any], t: Scoped):
    values = user_in(flatten(body.get("Operations", [])))
    updated = await run_in_threadpool(t.directory.update_user, t.id, user_id, values)
    return scim(user_out(t, updated))


@router.delete("/Users/{user_id}")
async def delete_user(user_id: str, t: Scoped):
    await run_in_threadpool(t.directory.remove_user, t.id, user_id)
    return Response(status_code=204)


# ------------------------------------------------------------------ groups
@router.get("/Groups")
async def list_groups(t: Scoped, filter: str | None = None, startIndex: int = 1, count: int = 100):
    wanted = parse_filter(filter)
    start, size = page_window(startIndex, count)

    def run() -> JSONResponse:
        found = t.directory.groups(t.id)
        if wanted:
            found = [g for g in found if g["name"].split(":", 1)[1] == wanted[1]]
        total = len(found)
        window = found[start - 1 : start - 1 + size]
        return listing(
            [group_out(t, g, t.directory.group_members(t.id, g["id"])) for g in window],
            total,
            start,
        )

    return await run_in_threadpool(run)


@router.get("/Groups/{group_id}")
async def get_group(group_id: str, t: Scoped):
    def run():
        return group_out(
            t, t.directory.group(t.id, group_id), t.directory.group_members(t.id, group_id)
        )

    return scim(await run_in_threadpool(run))


def sync_members(t: Tenant, group_id: str, wanted: list[str]) -> None:
    current = {m["id"] for m in t.directory.group_members(t.id, group_id)}
    for user_id in set(wanted) - current:
        t.directory.add_to_group(t.id, group_id, user_id)
    for user_id in current - set(wanted):
        t.directory.remove_from_group(t.id, group_id, user_id)


@router.post("/Groups")
async def create_group(body: dict[str, Any], t: Scoped):
    name = body.get("displayName")
    if not name or ":" in name:
        raise InvalidRequestError("displayName is required and may not contain ':'")

    def run():
        created = t.directory.create_group(t.id, name)
        sync_members(t, created["id"], [m["value"] for m in body.get("members", [])])
        return group_out(t, created, t.directory.group_members(t.id, created["id"]))

    out = await run_in_threadpool(run)
    return scim(out, 201, f"{t.base}/Groups/{out['id']}")


@router.put("/Groups/{group_id}")
async def replace_group(group_id: str, body: dict[str, Any], t: Scoped):
    def run():
        if body.get("displayName"):
            t.directory.rename_group(t.id, group_id, body["displayName"])
        sync_members(t, group_id, [m["value"] for m in body.get("members", [])])
        return group_out(
            t, t.directory.group(t.id, group_id), t.directory.group_members(t.id, group_id)
        )

    return scim(await run_in_threadpool(run))


@router.patch("/Groups/{group_id}")
async def patch_group(group_id: str, body: dict[str, Any], t: Scoped):
    def run():
        for op in body.get("Operations", []):
            verb, path, value = str(op.get("op", "")).lower(), op.get("path") or "", op.get("value")
            if path == "displayName" or (
                not path and isinstance(value, dict) and "displayName" in value
            ):
                t.directory.rename_group(
                    t.id, group_id, value if isinstance(value, str) else value["displayName"]
                )
            elif path.startswith("members"):
                if verb in {"add", "replace"} and path == "members":
                    wanted = [m["value"] for m in value or []]
                    if verb == "replace":
                        sync_members(t, group_id, wanted)
                    else:
                        for user_id in wanted:
                            t.directory.add_to_group(t.id, group_id, user_id)
                elif verb == "remove":
                    selected = re.search(r'value\s+eq\s+"([^"]+)"', path)
                    targets = [selected.group(1)] if selected else [m["value"] for m in value or []]
                    if not targets:  # `remove members` with no value empties the group
                        targets = [m["id"] for m in t.directory.group_members(t.id, group_id)]
                    for user_id in targets:
                        t.directory.remove_from_group(t.id, group_id, user_id)
                else:
                    raise InvalidRequestError(f"Operation {verb!r} is not supported on members")
            elif not path and isinstance(value, dict) and "members" in value:
                for member in value["members"]:
                    t.directory.add_to_group(t.id, group_id, member["value"])
            else:
                raise InvalidRequestError(f"Unsupported patch path {path!r}")
        return group_out(
            t, t.directory.group(t.id, group_id), t.directory.group_members(t.id, group_id)
        )

    return scim(await run_in_threadpool(run))


@router.delete("/Groups/{group_id}")
async def delete_group(group_id: str, t: Scoped):
    await run_in_threadpool(t.directory.delete_group, t.id, group_id)
    return Response(status_code=204)
