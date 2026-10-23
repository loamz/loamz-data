"""End-to-end check of a local compose cell: Keycloak org + user, tenant provisioning, agent API, data API.

Run after `docker compose --profile agents up -d`:  uv run --package loamz-core python deploy/compose/e2e.py deploy/compose/.env
"""

import sys
import time
from pathlib import Path

import httpx

from loamz.core.tenancy import files_volume, tenant_alias

ENV = dict(
    line.split("=", 1)
    for line in Path(sys.argv[1]).read_text().splitlines()
    if "=" in line and not line.startswith("#")
)
KC = f"http://localhost:{ENV.get('PORT_KEYCLOAK', '8081')}"
REALM = f"{KC}/realms/loamz"
AGENT = "http://localhost:8000/api/v1"
DATA = f"http://localhost:{ENV.get('PORT_DATA_API', '8020')}/api/v1"
# Keycloak only answers for its configured hostname; browsers and containers know it as keycloak.localhost:8081.
KC_HOST = {
    "Host": ENV.get("KEYCLOAK_PUBLIC_URL", "http://keycloak.localhost:8081").split("://", 1)[1]
}
http = httpx.Client(timeout=60)


def step(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f"  {detail}" if detail else ""))
    if not ok:
        sys.exit(1)


def admin_token():
    r = http.post(
        f"{KC}/realms/master/protocol/openid-connect/token",
        headers=KC_HOST,
        data={
            "grant_type": "password",
            "client_id": "admin-cli",
            "username": ENV["KEYCLOAK_ADMIN_USER"],
            "password": ENV["KEYCLOAK_ADMIN_PASSWORD"],
        },
    )
    r.raise_for_status()
    return {"Authorization": f"Bearer {r.json()['access_token']}", **KC_HOST}


admin = admin_token()
api = f"{KC}/admin/realms/loamz"

# 1. organization + user + membership
alias = f"e2e{int(time.time())}"
r = http.post(
    f"{api}/organizations",
    headers=admin,
    json={"name": alias, "alias": alias, "domains": [{"name": f"{alias}.example.com"}]},
)
step("create organization", r.status_code == 201, str(r.status_code) + r.text[:200])
org_id = r.headers["Location"].rsplit("/", 1)[1]
r = http.post(
    f"{api}/users",
    headers=admin,
    json={
        "username": f"alice-{alias}",
        "email": f"alice@{alias}.example.com",
        "emailVerified": True,
        "enabled": True,
        "firstName": "Alice",
        "lastName": "E2E",
        "credentials": [{"type": "password", "value": "e2e-Passw0rd!", "temporary": False}],
    },
)
step("create user", r.status_code == 201, str(r.status_code) + r.text[:200])
user_id = r.headers["Location"].rsplit("/", 1)[1]
r = http.post(
    f"{api}/organizations/{org_id}/members",
    headers=admin,
    content=f'"{user_id}"',
)
r = (
    r
    if r.status_code < 400
    else http.post(
        f"{api}/organizations/{org_id}/members",
        headers={**admin, "Content-Type": "application/json"},
        content=f'"{user_id}"',
    )
)
step("add member", r.status_code in (201, 204), str(r.status_code) + r.text[:200])

# test-only: let the CLI client use the password grant so a script can sign in (the realm file is unchanged)
clients = http.get(f"{api}/clients", headers=admin, params={"clientId": "loamz-cli"}).json()
cli = clients[0]
http.put(
    f"{api}/clients/{cli['id']}",
    headers=admin,
    json={**cli, "directAccessGrantsEnabled": True},
).raise_for_status()


def user_token(scope):
    r = http.post(
        f"{REALM}/protocol/openid-connect/token",
        headers=KC_HOST,
        data={
            "grant_type": "password",
            "client_id": "loamz-cli",
            "username": f"alice-{alias}",
            "password": "e2e-Passw0rd!",
            "scope": scope,
        },
    )
    step(
        f"sign in ({scope})",
        r.status_code == 200,
        r.text[:300] if r.status_code != 200 else "",
    )
    return {"Authorization": f"Bearer {r.json()['access_token']}"}


# 2. provision the tenant in loamz-data (as the platform would, with cell-admin)
r = http.post(
    f"{REALM}/protocol/openid-connect/token",
    headers=KC_HOST,
    data={
        "grant_type": "client_credentials",
        "client_id": "cell-admin",
        "client_secret": ENV["CELL_ADMIN_CLIENT_SECRET"],
    },
)
step("cell-admin token", r.status_code == 200, r.text[:200])
cell = {"Authorization": f"Bearer {r.json()['access_token']}"}

r = http.post(
    f"{DATA}/tenants",
    headers=cell,
    json={"tenant_id": org_id, "alias": tenant_alias(org_id)},
)
step("provision tenant", r.status_code in (200, 201), str(r.status_code) + r.text[:300])

# the tenant's administrator group (the platform creates it per cell); Alice administers her tenant
r = http.post(f"{api}/groups", headers=admin, json={"name": "tenant-admins"})
if r.status_code == 409:
    found = http.get(
        f"{api}/groups", headers=admin, params={"search": "tenant-admins", "exact": "true"}
    ).json()
    group_id = found[0]["id"]
else:
    group_id = r.headers["Location"].rsplit("/", 1)[1]
r = http.put(f"{api}/users/{user_id}/groups/{group_id}", headers=admin)
step("make Alice the tenant administrator", r.status_code in (200, 204), str(r.status_code))

# 3. agent API as the user
user = user_token(f"openid organization:{alias}")
r = http.get(f"{AGENT}/threads", headers=user)
step("agent: list threads", r.status_code == 200, str(r.status_code) + r.text[:300])
r = http.post(
    f"{AGENT}/files/upload",
    headers=user,
    files={
        "file": (
            "notes.txt",
            b"Loamz cells keep data in the customer's cloud.\n",
            "text/plain",
        )
    },
)
step(
    "agent: upload a file (stored in loamz-data)",
    r.status_code in (200, 201),
    str(r.status_code) + r.text[:300],
)
file_id = r.json().get("file_id") or r.json().get("id")
# the bytes must be in the tenant's governed volume (Loamz Data), not on the agent server's own disk
listing = http.get(
    f"{DATA}/volumes/{files_volume(org_id)}/files", headers=user, params={"prefix": "users/"}
)
step(
    "data: the uploaded file is in the tenant's Loamz Data volume",
    listing.status_code == 200 and "notes.txt" in listing.text,
    str(listing.status_code) + listing.text[:300],
)
r = http.get(f"{AGENT}/files/{file_id}/download", headers=user)
step(
    "agent: download it back",
    r.status_code == 200 and b"customer's cloud" in r.content,
    str(r.status_code) + r.text[:200],
)

# 4. data API directly as the same user: the file is in the tenant's governed volume
r = http.get(f"{DATA}/search", headers=user, params={"q": "tenant_"})
step("data: search the catalog", r.status_code == 200, str(r.status_code) + r.text[:300])

# 5. negative: a token for another organization's user cannot read alice's file
r = http.post(
    f"{api}/users",
    headers=admin,
    json={
        "username": f"mallory-{alias}",
        "enabled": True,
        "emailVerified": True,
        "email": f"m@{alias}.org",
        "firstName": "M",
        "lastName": "X",
        "credentials": [{"type": "password", "value": "e2e-Passw0rd!", "temporary": False}],
    },
)
r = http.post(
    f"{REALM}/protocol/openid-connect/token",
    headers=KC_HOST,
    data={
        "grant_type": "password",
        "client_id": "loamz-cli",
        "username": f"mallory-{alias}",
        "password": "e2e-Passw0rd!",
        "scope": "openid",
    },
)
mallory = {"Authorization": f"Bearer {r.json()['access_token']}"}
r = http.get(f"{AGENT}/files/{file_id}/download", headers=mallory)
step(
    "isolation: an outsider cannot read the file",
    r.status_code in (401, 403, 404),
    str(r.status_code),
)

# 5b. SCIM: the tenant administrator provisions people and groups with a personal access token
r = http.post(f"{DATA}/access-tokens", headers=user, json={"name": "e2e-scim"})
step(
    "create a personal access token",
    r.status_code == 201 and r.json()["token"].startswith("lpat_"),
    str(r.status_code),
)
pat = {"Authorization": "Bearer " + r.json()["token"]}
SCIM = DATA.removesuffix("/api/v1") + "/scim/v2"
r = http.get(f"{DATA}/catalogs", headers=pat)
step("the token acts as its owner", r.status_code == 200, str(r.status_code))
r = http.post(
    f"{SCIM}/Users",
    headers=pat,
    json={
        "userName": f"scim-{alias}",
        "name": {"givenName": "Sam", "familyName": "Scim"},
        "emails": [{"value": f"sam@{alias}.example.com", "primary": True}],
    },
)
step("SCIM: create a user", r.status_code == 201, str(r.status_code) + r.text[:300])
scim_user = r.json()["id"]
r = http.get(f"{SCIM}/Users", headers=pat, params={"filter": f'userName eq "scim-{alias}"'})
step("SCIM: find the user by filter", r.json().get("totalResults") == 1, r.text[:200])
r = http.get(f"{SCIM}/Users", headers=pat)
step("SCIM: list shows the tenant's people", r.json()["totalResults"] >= 2, r.text[:200])
r = http.patch(
    f"{SCIM}/Users/{scim_user}",
    headers=pat,
    json={"Operations": [{"op": "replace", "value": {"active": False}}]},
)
step(
    "SCIM: deactivate the user", r.status_code == 200 and r.json()["active"] is False, r.text[:200]
)
r = http.post(
    f"{SCIM}/Groups",
    headers=pat,
    json={"displayName": "analysts", "members": [{"value": scim_user}]},
)
step(
    "SCIM: create a group with a member",
    r.status_code == 201 and len(r.json()["members"]) == 1,
    r.text[:300],
)
scim_group = r.json()["id"]
r = http.get(f"{SCIM}/Groups", headers=pat, params={"filter": 'displayName eq "analysts"'})
step("SCIM: find the group", r.json().get("totalResults") == 1, r.text[:200])
r = http.delete(f"{SCIM}/Groups/{scim_group}", headers=pat)
step("SCIM: delete the group", r.status_code == 204, str(r.status_code))
r = http.delete(f"{SCIM}/Users/{scim_user}", headers=pat)
step("SCIM: remove the user from the tenant", r.status_code == 204, str(r.status_code))
r = http.get(f"{SCIM}/Users/{scim_user}", headers=pat)
step("SCIM: the removed user is gone", r.status_code == 404, str(r.status_code))
tokens = http.get(f"{DATA}/access-tokens", headers=user).json()["items"]
r = http.delete(f"{DATA}/access-tokens/{tokens[0]['id']}", headers=user)
step("revoke the token", r.status_code == 204, str(r.status_code))
r = http.get(f"{DATA}/catalogs", headers=pat)
step("a revoked token is refused", r.status_code == 401, str(r.status_code))

# 6. chat: the agent answers a question about the uploaded file (needs an LLM key in .env)
if (
    ENV.get("OPENAI_API_KEY")
    or ENV.get("GEMINI_API_KEY")
    or ENV.get("ANTHROPIC_API_KEY")
    or ENV.get("OPENROUTER_API_KEY")
):
    # The Python client library drives the agents API: a thread, then a streamed run.
    from loamz.agents import AgentsClient

    token = user["Authorization"].split(" ", 1)[1]
    with AgentsClient(AGENT.removesuffix("/api/v1"), token, timeout=180) as agents:
        thread = agents.create_thread(name="e2e")["id"]
        step("chat: create a thread (SDK)", bool(thread), thread)
        answer, failed, events = "", False, []
        prompt = "Read the attached file and tell me in one sentence where cells keep data."
        for event in agents.invoke_agent(thread, prompt, file_ids=[file_id]):
            events.append(event.type)
            answer += event.text
            if event.failed:
                failed, answer = True, str(event.data)
        step("chat: start a run", bool(events), ",".join(events[:3]))
    step("chat: the answer streams back", answer != "" and not failed, answer[-300:])
    step("chat: it read the file", "cloud" in answer.lower(), answer[-300:])
else:
    print("SKIP chat (no LLM key in .env)")


# 7. tool servers: an agent that uses the solutions' tools (mail triage, paper trading), with a person approving trades
if ENV.get("TOOL_SERVERS") and ENV.get("OPENAI_API_KEY"):
    import threading

    from loamz.agents import AgentsClient

    base = AGENT.removesuffix("/api/v1")
    token = user["Authorization"].split(" ", 1)[1]
    with (
        AgentsClient(base, token, timeout=240) as agents,
        AgentsClient(base, token, timeout=60) as watcher,
    ):
        offered = {t["name"] for t in agents.list_tools()}
        step(
            "tool servers: their tools are offered",
            {"solutions__mail_digest", "solutions__trade_place_order"} <= offered,
            ",".join(sorted(n for n in offered if n.startswith("solutions__")))[:200],
        )
        agent = agents.create_agent(
            "Concierge",
            instructions="You are the owner's concierge. Use your tools; never invent mail or account data. Be brief.",
            allowed_tools=[
                "solutions__mail_digest",
                "solutions__mail_ask",
                "solutions__trade_status",
                "solutions__trade_place_order",
            ],
        )
        thread = agents.create_thread(agent_id=agent["id"])["id"]

        # a read-only tool runs without asking
        events = list(
            agents.invoke_agent(
                thread, "Use your mail digest tool and tell me in one sentence what is most urgent."
            )
        )
        calls = [e.data.get("tool_name") for e in events if e.type == "tool.call"]
        answer = "".join(e.text for e in events)
        step(
            "tool servers: a read-only tool runs unasked",
            "solutions__mail_digest" in calls,
            ",".join(map(str, calls)),
        )
        step(
            "tool servers: the answer comes from the mail",
            "contract" in answer.lower() or "clause" in answer.lower(),
            answer[-200:],
        )
        step(
            "tool servers: no approval was needed for it",
            not any(e.type == "approval.requested" for e in events),
        )

        # an order waits for a person
        found: list[dict] = []
        run_events: list = []

        def converse() -> None:
            run_events.extend(
                agents.invoke_agent(
                    thread,
                    "Buy 3 shares of AAA with your place order tool, then tell me what happened.",
                )
            )

        worker = threading.Thread(target=converse)
        worker.start()
        deadline = time.time() + 120
        while worker.is_alive() and time.time() < deadline and not found:
            found = [
                a
                for a in watcher.list_approvals()
                if a.get("tool_name") == "solutions__trade_place_order"
            ]
            time.sleep(0.5)
        step("tool servers: placing an order asks a person first", bool(found), str(found)[:200])
        watcher.respond_to_approval(found[0]["request_id"], "approve")
        worker.join(180)
        placed = [
            e
            for e in run_events
            if e.type == "tool.result" and e.data.get("tool_name") == "solutions__trade_place_order"
        ]
        step(
            "tool servers: the approved order went through the risk limits and was placed",
            bool(placed) and "traded" in placed[0].data.get("output", ""),
            (placed[0].data.get("output", "") if placed else str([e.type for e in run_events]))[
                :200
            ],
        )

        # the same request, denied: nothing happens
        found, run_events = [], []
        thread = agents.create_thread(agent_id=agent["id"])["id"]
        worker = threading.Thread(target=converse)
        worker.start()
        deadline = time.time() + 120
        while worker.is_alive() and time.time() < deadline and not found:
            found = [
                a
                for a in watcher.list_approvals()
                if a.get("tool_name") == "solutions__trade_place_order"
            ]
            time.sleep(0.5)
        watcher.respond_to_approval(found[0]["request_id"], "deny")
        worker.join(180)
        refused = [
            e
            for e in run_events
            if e.type == "tool.result" and "traded" in e.data.get("output", "")
        ]
        step(
            "tool servers: a denied order is not placed",
            not refused,
            str([e.type for e in run_events])[:200],
        )
else:
    print("SKIP tool servers (no TOOL_SERVERS in .env)")

# 8. a client that walks away mid-run must not stop the server from running the next one
if (
    ENV.get("OPENAI_API_KEY")
    or ENV.get("GEMINI_API_KEY")
    or ENV.get("ANTHROPIC_API_KEY")
    or ENV.get("OPENROUTER_API_KEY")
):
    from loamz.agents import AgentsClient

    token = user["Authorization"].split(" ", 1)[1]
    with AgentsClient(AGENT.removesuffix("/api/v1"), token, timeout=90) as agents:
        abandoned = agents.create_thread(name="abandoned")["id"]
        stream = agents.invoke_agent(
            abandoned, "Write a long, detailed essay about the history of the abacus."
        )
        first = next(stream)
        stream.close()  # the client disconnects while the run is going
        step("a client can walk away mid-run", first is not None, first.type)
        again = agents.create_thread(name="after")["id"]
        reply = "".join(
            e.text for e in agents.invoke_agent(again, "Reply with the single word: ready")
        )
        step("the next run still completes", "ready" in reply.lower(), reply[-100:])

# 9. conversations are separate: what is said in one thread is unknown in another
if (
    ENV.get("OPENAI_API_KEY")
    or ENV.get("GEMINI_API_KEY")
    or ENV.get("ANTHROPIC_API_KEY")
    or ENV.get("OPENROUTER_API_KEY")
):
    from loamz.agents import AgentsClient

    token = user["Authorization"].split(" ", 1)[1]
    with AgentsClient(AGENT.removesuffix("/api/v1"), token, timeout=90) as agents:
        first = agents.create_thread(name="secret")["id"]
        told = "".join(
            e.text
            for e in agents.invoke_agent(
                first, "Remember this code word: zebra42. Reply only: noted"
            )
        )
        second = agents.create_thread(name="other")["id"]
        asked = "".join(
            e.text
            for e in agents.invoke_agent(
                second, "What code word did I tell you earlier? If you do not know, say: unknown"
            )
        )
        step(
            "threads are isolated: another thread does not know the code word",
            "zebra42" not in asked.lower(),
            asked[-160:],
        )
        again = "".join(
            e.text for e in agents.invoke_agent(first, "What was the code word? One word.")
        )
        step("a thread remembers its own conversation", "zebra42" in again.lower(), again[-160:])

# 10. two conversations running at the same time stay separate (one must not be folded into the other's run)
if (
    ENV.get("OPENAI_API_KEY")
    or ENV.get("GEMINI_API_KEY")
    or ENV.get("ANTHROPIC_API_KEY")
    or ENV.get("OPENROUTER_API_KEY")
):
    import threading

    from loamz.agents import AgentsClient

    token = user["Authorization"].split(" ", 1)[1]
    with (
        AgentsClient(AGENT.removesuffix("/api/v1"), token, timeout=120) as long_one,
        AgentsClient(AGENT.removesuffix("/api/v1"), token, timeout=120) as short_one,
    ):
        slow, quick = (
            long_one.create_thread(name="long")["id"],
            short_one.create_thread(name="short")["id"],
        )
        results: dict[str, str] = {}

        def essay() -> None:
            results["long"] = "".join(
                e.text
                for e in long_one.invoke_agent(slow, "Write a 300 word essay about the abacus.")
            )

        worker = threading.Thread(target=essay)
        worker.start()
        time.sleep(3)  # the essay is being written
        results["short"] = "".join(
            e.text for e in short_one.invoke_agent(quick, "Reply with the single word: ready")
        )
        worker.join(120)
        step(
            "overlapping conversations stay separate",
            "ready" in results["short"].lower()
            and len(results["short"]) < 80
            and len(results["long"]) > 800,
            f"short={results['short'][:60]!r} long={len(results['long'])} chars",
        )

print("ALL PASSED")
