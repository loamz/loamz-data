"""Personal access tokens: long-lived credentials a person creates for scripts, notebooks and CI.

A token looks like `lpat_<id>.<secret>`. Only a SHA-256 of the secret is stored, so a database leak yields no usable
token. Tokens expire (90 days by default, one year at most), can be revoked or rotated, and record when they were last
used. A token acts as its owner with the owner's groups at creation, never as a platform administrator, and cannot create
tokens; delegated (agent) tokens cannot create them either.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import secrets
from datetime import UTC, datetime, timedelta
from typing import Any
from uuid import uuid4

from loamz.engine.errors import UnauthenticatedError
from loamz.server.data.kernel.errors import not_found
from loamz.server.data.kernel.metadata import CatalogStore, now
from loamz.server.data.kernel.principal import Principal

PREFIX = "lpat_"
DEFAULT_DAYS = 90
MAX_DAYS = 365
GROUPS_JSON = ("groups",)


def digest(secret: str) -> str:
    return hashlib.sha256(secret.encode()).hexdigest()


def is_token(authorization: str | None) -> bool:
    return bool(authorization) and authorization.removeprefix("Bearer ").strip().startswith(PREFIX)  # type: ignore[union-attr]


def public(row: dict[str, Any]) -> dict[str, Any]:
    keep = (
        "id",
        "name",
        "created_at",
        "expires_at",
        "last_used_at",
        "revoked_at",
        "read_only",
        "rotated_from",
    )
    value = {k: row[k] for k in keep}
    value["read_only"] = bool(value["read_only"])
    value["active"] = row["revoked_at"] is None and row["expires_at"] > now()
    return value


def issue(
    store: CatalogStore,
    owner: Principal,
    *,
    name: str,
    expires_in_days: int,
    read_only: bool,
    admin_group: str,
    rotated_from: str | None = None,
) -> tuple[dict[str, Any], str]:
    """A new token for `owner`. Returns (its public record, the token itself, shown once)."""
    token_id = uuid4().hex
    secret = secrets.token_urlsafe(32)
    row = {
        "id": token_id,
        "name": name,
        "secret_hash": digest(secret),
        "subject": owner.subject,
        "username": owner.name,
        "email": owner.email,
        "tenant_id": owner.tenant_id,
        "groups": sorted(g for g in owner.groups if g != admin_group),
        "read_only": 1 if read_only else 0,
        "created_by": owner.id,
        "created_at": now(),
        "expires_at": (datetime.now(UTC) + timedelta(days=expires_in_days)).isoformat(),
        "last_used_at": None,
        "revoked_at": None,
        "rotated_from": rotated_from,
    }
    with store.connection() as db:
        db.execute(
            f"INSERT INTO access_tokens ({', '.join(row)}) VALUES ({', '.join('?' * len(row))})",
            [json.dumps(v) if isinstance(v, list) else v for v in row.values()],
        )
    return public(row), f"{PREFIX}{token_id}.{secret}"


def authenticate(store: CatalogStore, authorization: str) -> Principal:
    bearer = authorization.removeprefix("Bearer ").strip()
    token_id, _, secret = bearer.removeprefix(PREFIX).partition(".")
    with store.connection() as db:
        row = db.execute("SELECT * FROM access_tokens WHERE id=?", (token_id,)).fetchone()
    # One constant-time comparison whether or not the id exists, so timing does not reveal ids.
    expected = row["secret_hash"] if row else digest("absent")
    matches = hmac.compare_digest(expected, digest(secret))
    if row is None or not matches or row["revoked_at"] or row["expires_at"] <= now():
        raise UnauthenticatedError("Access token rejected")
    with store.connection() as db:
        db.execute("UPDATE access_tokens SET last_used_at=? WHERE id=?", (now(), token_id))
    return Principal(
        subject=row["subject"],
        name=row["username"],
        kind="user",
        groups=frozenset(json.loads(row["groups"])),
        email=row["email"],
        is_admin=False,
        read_only=bool(row["read_only"]),
        scopes=None,
        tenant_id=row["tenant_id"],
        context={"auth": "access-token", "access_token": token_id},
    )


def owned(store: CatalogStore, owner: Principal, token_id: str) -> dict[str, Any]:
    row = store.get("access_tokens", token_id)
    if row["subject"] != owner.subject and not owner.is_admin:
        raise not_found("Access token")
    return row


def revoke(store: CatalogStore, token_id: str) -> dict[str, Any]:
    with store.connection() as db:
        db.execute(
            "UPDATE access_tokens SET revoked_at=? WHERE id=? AND revoked_at IS NULL",
            (now(), token_id),
        )
    return store.get("access_tokens", token_id)


def list_for(
    store: CatalogStore, subject: str | None, active_only: bool, before: str | None, limit: int
) -> list[dict[str, Any]]:
    clauses, params = ["1=1"], []
    if subject:
        clauses.append("subject=?")
        params.append(subject)
    if active_only:
        clauses.append("revoked_at IS NULL AND expires_at>?")
        params.append(now())
    return store.page("access_tokens", " AND ".join(clauses), params, before, limit)
