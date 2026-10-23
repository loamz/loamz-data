"""/api/v1/legal-holds: keep data a legal matter needs.

While a hold is active, the data it covers cannot be erased (`/erasure-requests` answers `423 legal-hold`), table snapshots
cannot be expired, and no audit history is purged. A hold names a tenant, one subject of a tenant, or one table. Creating
and releasing a hold are audited. Who may: a platform administrator, or the tenant's own administrator for holds on that
tenant. Delegated (agent) tokens declare no scope here, so they can never place or lift a hold.
"""

from __future__ import annotations

from typing import Annotated, Literal

from fastapi import APIRouter, Query, Request
from pydantic import BaseModel, ConfigDict, Field

from loamz.core import tenancy
from loamz.core.tenancy import TENANT_ADMIN_GROUP
from loamz.engine.errors import InvalidRequestError
from loamz.server.data.kernel.errors import forbidden
from loamz.server.data.kernel.http import Caller, Svc
from loamz.server.data.kernel.principal import Principal

router = APIRouter(prefix="/api/v1", tags=["legal holds"])
Id = Annotated[str, Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._@+-]{0,199}$")]


class HoldRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    scope: Literal["tenant", "user", "table"]
    reason: Annotated[str, Field(min_length=3, max_length=500)]
    case_ref: Annotated[str | None, Field(max_length=200)] = None
    tenant_id: Annotated[str | None, Field(pattern=tenancy.TENANT_ID_PATTERN)] = None
    subject: Id | None = None
    table_id: Id | None = None


def may_manage(caller: Principal, tenant_id: str | None) -> bool:
    if caller.is_admin:
        return True
    return bool(tenant_id) and TENANT_ADMIN_GROUP in caller.groups and caller.tenant_id == tenant_id


def public(hold: dict) -> dict:
    return {**hold, "active": hold["released_at"] is None}


@router.post("/legal-holds", status_code=201)
def place_hold(body: HoldRequest, svc: Svc, caller: Caller, request: Request):
    tenant = body.tenant_id or caller.tenant_id
    if body.scope == "tenant" and body.subject or body.scope != "table" and body.table_id:
        raise InvalidRequestError("This scope does not take that field")
    if body.scope in {"tenant", "user"} and not tenant:
        raise InvalidRequestError("Name the tenant (tenant_id)")
    if body.scope == "user" and not body.subject:
        raise InvalidRequestError("scope user needs a subject")
    if body.scope == "table":
        if not body.table_id:
            raise InvalidRequestError("scope table needs a table_id")
        record = svc.store.table_by_id(body.table_id)
        tenant = None
        if not caller.is_admin:
            raise forbidden("Only a platform administrator may hold a table")
        svc.store.require(caller, "table", record, "MODIFY")
    elif not may_manage(caller, tenant):
        raise forbidden(
            "Only a platform administrator or the tenant's administrator may place a hold"
        )
    hold = svc.store.create_hold(
        {
            "scope": body.scope,
            "tenant_id": tenant,
            "subject": body.subject,
            "table_id": body.table_id,
            "reason": body.reason,
            "case_ref": body.case_ref,
            "created_by": caller.id,
            "released_by": None,
            "released_at": None,
        }
    )
    svc.store.audit(
        caller,
        "legal-hold.place",
        hold["id"],
        "success",
        request.state.request_id,
        scope=body.scope,
        tenant_id=tenant,
        case_ref=body.case_ref,
    )
    return public(hold)


@router.get("/legal-holds")
def list_holds(
    svc: Svc,
    caller: Caller,
    tenant_id: str | None = None,
    active: bool = True,
    page_size: int = Query(50, ge=1, le=200),
    page_token: str | None = None,
):
    tenant = tenant_id or (None if caller.is_admin else caller.tenant_id)
    if not may_manage(caller, tenant) and not caller.is_admin:
        raise forbidden("Only administrators may list legal holds")
    rows = svc.store.list_holds(tenant, active, page_token, page_size + 1)
    more = len(rows) > page_size
    rows = rows[:page_size]
    token = f"{rows[-1]['created_at']}|{rows[-1]['id']}" if more else None
    return {"items": [public(r) for r in rows], "next_page_token": token}


@router.post("/legal-holds/{hold_id}:release")
def release_hold(hold_id: str, svc: Svc, caller: Caller, request: Request):
    hold = svc.store.get("legal_holds", hold_id)
    if not may_manage(caller, hold["tenant_id"]):
        raise forbidden(
            "Only a platform administrator or the tenant's administrator may release a hold"
        )
    released = svc.store.release_hold(hold_id, caller.id)
    svc.store.audit(
        caller,
        "legal-hold.release",
        hold_id,
        "success",
        request.state.request_id,
        scope=hold["scope"],
        tenant_id=hold["tenant_id"],
    )
    return public(released)
