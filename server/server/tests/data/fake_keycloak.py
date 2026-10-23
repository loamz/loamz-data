"""An in-memory stand-in for the parts of Keycloak's admin API the directory uses (organizations, users, groups)."""

from __future__ import annotations

import json
from uuid import uuid4

import httpx

REALM = "/admin/realms/loamz"


class FakeKeycloak:
    def __init__(self) -> None:
        self.users: dict[str, dict] = {}
        self.orgs: dict[str, set[str]] = {}
        self.groups: dict[str, dict] = {}
        self.group_members: dict[str, set[str]] = {}
        self.requests: list[httpx.Request] = []

    def add_org(self, org: str) -> None:
        self.orgs.setdefault(org, set())

    def transport(self) -> httpx.MockTransport:
        return httpx.MockTransport(self.handle)

    def respond(self, status: int, body=None, location: str | None = None) -> httpx.Response:
        headers = {"location": location} if location else {}
        return (
            httpx.Response(status, json=body, headers=headers)
            if body is not None
            else httpx.Response(status, headers=headers)
        )

    def handle(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        path, method = request.url.path, request.method
        if path.endswith("/protocol/openid-connect/token"):
            return self.respond(200, {"access_token": "admin-token", "expires_in": 300})
        if request.headers.get("authorization") != "Bearer admin-token":
            return self.respond(401)
        rest = path.removeprefix(REALM)
        body = json.loads(request.content) if request.content else None
        params = request.url.params
        parts = rest.strip("/").split("/")
        if parts[0] == "organizations":
            org = parts[1]
            members = self.orgs.get(org)
            if members is None:
                return self.respond(404)
            if len(parts) == 3 and parts[2] == "members" and method == "GET":
                found = [self.users[u] for u in sorted(members)]
                if params.get("search"):
                    found = [
                        u
                        for u in found
                        if u["username"] == params["search"] or u.get("email") == params["search"]
                    ]
                first, count = int(params.get("first", 0)), int(params.get("max", 100))
                return self.respond(200, found[first : first + count])
            if len(parts) == 3 and parts[2] == "members" and method == "POST":
                members.add(body)
                return self.respond(201)
            if parts[2:] == ["members", "count"]:
                return httpx.Response(200, text=str(len(members)))
            if len(parts) == 4 and parts[2] == "members":
                uid = parts[3]
                if uid not in members:
                    return self.respond(404)
                if method == "DELETE":
                    members.discard(uid)
                    return self.respond(204)
                return self.respond(200, self.users[uid])
        if parts[0] == "users":
            if len(parts) == 1 and method == "POST":
                if any(u["username"] == body["username"] for u in self.users.values()):
                    return self.respond(409)
                uid = uuid4().hex
                self.users[uid] = {"id": uid, "enabled": True, **body}
                return self.respond(201, location=f"http://kc{REALM}/users/{uid}")
            uid = parts[1]
            if uid not in self.users:
                return self.respond(404)
            if len(parts) == 2 and method == "GET":
                return self.respond(200, self.users[uid])
            if len(parts) == 2 and method == "PUT":
                self.users[uid] = body
                return self.respond(204)
            if len(parts) == 4 and parts[2] == "groups":
                gid = parts[3]
                (
                    self.group_members[gid].add
                    if method == "PUT"
                    else self.group_members[gid].discard
                )(uid)
                return self.respond(204)
        if parts[0] == "groups":
            if len(parts) == 1 and method == "GET":
                found = [g for g in self.groups.values() if params.get("search", "") in g["name"]]
                return self.respond(200, found)
            if len(parts) == 1 and method == "POST":
                gid = uuid4().hex
                self.groups[gid] = {"id": gid, **body}
                self.group_members[gid] = set()
                return self.respond(201, location=f"http://kc{REALM}/groups/{gid}")
            gid = parts[1]
            if gid not in self.groups:
                return self.respond(404)
            if len(parts) == 2 and method == "GET":
                return self.respond(200, self.groups[gid])
            if len(parts) == 2 and method == "PUT":
                self.groups[gid] = body
                return self.respond(204)
            if len(parts) == 2 and method == "DELETE":
                del self.groups[gid]
                return self.respond(204)
            if parts[2:] == ["members"]:
                return self.respond(200, [self.users[u] for u in sorted(self.group_members[gid])])
        return self.respond(501, {"unhandled": f"{method} {path}"})
