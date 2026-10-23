"""/api/v1/access-tokens: personal access tokens for scripts, notebooks and CI.

Create, list, rotate and revoke your own tokens (an administrator may list and revoke anyone's). The token is returned
once, on creation or rotation. A token cannot create tokens, and neither can a delegated (agent) token: sign in as the
person. See `loamz.server.data.kernel.tokens` for what a token may do.
"""

from __future__ import annotations

from typing import Annotated

from fastapi import APIRouter, Query, Request
from pydantic import BaseModel, ConfigDict, Field

from loamz.server.data.kernel import tokens
from loamz.server.data.kernel.errors import forbidden
from loamz.server.data.kernel.http import Caller, Svc

router = APIRouter(prefix="/api/v1", tags=["access tokens"])


class TokenRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    name: Annotated[str, Field(min_length=1, max_length=100)]
    expires_in_days: Annotated[int, Field(ge=1, le=tokens.MAX_DAYS)] = tokens.DEFAULT_DAYS
    read_only: bool = False


def require_person(caller) -> None:
    if caller.scopes is not None or caller.context.get("auth") == "access-token":
        raise forbidden(
            "Sign in as yourself to manage access tokens: a delegated or access token cannot"
        )


@router.post("/access-tokens", status_code=201)
def create_token(body: TokenRequest, svc: Svc, caller: Caller, request: Request):
    require_person(caller)
    record, token = tokens.issue(
        svc.store,
        caller,
        name=body.name,
        expires_in_days=body.expires_in_days,
        read_only=body.read_only,
        admin_group=svc.settings.admin_group,
    )
    svc.store.audit(
        caller,
        "access-token.create",
        record["id"],
        "success",
        request.state.request_id,
        name=body.name,
    )
    return {**record, "token": token}


@router.get("/access-tokens")
def list_tokens(
    svc: Svc,
    caller: Caller,
    all: bool = False,
    active: bool = True,
    page_size: int = Query(50, ge=1, le=200),
    page_token: str | None = None,
):
    if all and not caller.is_admin:
        raise forbidden("Only administrators may list everyone's tokens")
    rows = tokens.list_for(
        svc.store, None if all else caller.subject, active, page_token, page_size + 1
    )
    more = len(rows) > page_size
    rows = rows[:page_size]
    token = f"{rows[-1]['created_at']}|{rows[-1]['id']}" if more else None
    return {"items": [tokens.public(r) for r in rows], "next_page_token": token}


@router.delete("/access-tokens/{token_id}", status_code=204)
def revoke_token(token_id: str, svc: Svc, caller: Caller, request: Request):
    tokens.owned(svc.store, caller, token_id)
    tokens.revoke(svc.store, token_id)
    svc.store.audit(caller, "access-token.revoke", token_id, "success", request.state.request_id)


@router.post("/access-tokens/{token_id}:rotate", status_code=201)
def rotate_token(token_id: str, svc: Svc, caller: Caller, request: Request):
    """A new token with the same name and settings and a fresh expiry; the old one stops working at once."""
    require_person(caller)
    old = tokens.owned(svc.store, caller, token_id)
    if old["subject"] != caller.subject:
        raise forbidden("Only the owner may rotate a token")
    tokens.revoke(svc.store, token_id)
    record, token = tokens.issue(
        svc.store,
        caller,
        name=old["name"],
        expires_in_days=tokens.DEFAULT_DAYS,
        read_only=bool(old["read_only"]),
        admin_group=svc.settings.admin_group,
        rotated_from=token_id,
    )
    svc.store.audit(
        caller,
        "access-token.rotate",
        record["id"],
        "success",
        request.state.request_id,
        previous=token_id,
    )
    return {**record, "token": token}
