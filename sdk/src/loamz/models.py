from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class Workspace:
    id: str
    name: str
    org_id: str
    org_alias: str
    admin: bool = False

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Workspace:
        return cls(
            id=data["id"],
            name=data["name"],
            org_id=data.get("org_id", ""),
            org_alias=data.get("org_alias", ""),
            admin=bool(data.get("admin", False)),
        )


@dataclass
class Me:
    subject: str
    email: str
    name: str
    kind: str
    workspaces: list[Workspace] = field(default_factory=list)
    orgs: list[dict[str, Any]] = field(default_factory=list)
    groups: list[str] = field(default_factory=list)
    platform_admin: bool = False

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Me:
        workspaces = [Workspace.from_dict(w) for w in data.get("workspaces", [])]
        return cls(
            subject=data.get("subject", ""),
            email=data.get("email", ""),
            name=data.get("name", ""),
            kind=data.get("kind", "person"),
            workspaces=workspaces,
            orgs=data.get("orgs", []),
            groups=data.get("groups", []),
            platform_admin=bool(data.get("platform_admin", False)),
        )


@dataclass
class Securable:
    id: str
    name: str
    kind: str
    parent_id: str
    owner: str = ""
    classification: str = "normal"
    consistency: str = "transaction"
    placement: str = "local"
    created_ms: int = 0

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Securable:
        return cls(
            id=data["id"],
            name=data["name"],
            kind=data["kind"],
            parent_id=data.get("parent_id", ""),
            owner=data.get("owner", ""),
            classification=data.get("classification", "normal"),
            consistency=data.get("consistency", "transaction"),
            placement=data.get("placement", "local"),
            created_ms=int(data.get("created_ms", 0)),
        )
