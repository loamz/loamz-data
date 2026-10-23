# Tool servers

Agents get new abilities from **tool servers**: programs that speak the Model Context Protocol (MCP) and offer tools such
as "show my positions" or "summarise my mail". You connect a server to a deployment; you then decide, agent by agent,
which of its tools that agent may use. Anything risky waits for a person's approval.

## Connect a server
Set `LOAMZ_AGENTS_TOOLS__SERVERS` to a JSON list (in Compose, `TOOL_SERVERS` in `.env`; in Helm, `agents.extraEnv`, ideally
from a Secret because it holds the token):

```json
[{"name": "desk", "url": "http://solutions:8765/sse", "token": "…", "safe_tools": ["status", "explain"]}]
```

| Field | Meaning |
| --- | --- |
| `name` | lower-case letters, digits, underscores; prefixes the server's tools as `desk__status`, so servers cannot collide |
| `url` | the server's SSE endpoint |
| `token` | sent as `Authorization: Bearer <token>` |
| `safe_tools` | the server's own tool names that only read. **Only these run without approval.** Everything else asks a person first |
| `timeout`, `call_timeout` | seconds to connect, and for one tool call (a dead connection is detected by this, then replaced) |

The platform connects at start (best effort: a server that is down is skipped and retried every `refresh_seconds`, 60 by
default) and picks up tools a server adds later.

## Give an agent its tools
Create or edit an agent and list the tool names in `allowed_tools`, for example `["desk__status", "desk__place_order"]`.
`GET /api/v1/agents/tools` lists everything on offer, tool-server tools included. Names that do not exist are refused.

- An agent gets **only** the tool-server tools it names. An agent with `allowed_tools: null` ("every built-in tool") still gets
  none of them.
- A tool not in `safe_tools` pauses the run and asks for approval (the approval appears in the console and at
  `GET /api/v1/approvals`). The run sleeps, holding no resources, until a person approves, denies or edits the arguments with
  `POST /api/v1/chat/respond/{request_id}`. A denied call never reaches the server.
- What a server says about its own tools (including any "read only" hint) is never trusted. Only the operator's `safe_tools` is.

## Trust model
A tool server is code the platform does not control, configured by the **operator**, not by users. It receives the arguments
of approved calls and nothing else; it is not given the caller's identity, so a server acts for the whole deployment. Run a
tool server per person or per tenant, or make the server itself single-purpose, when callers must be kept apart. Put it on a
private network and require its token.

## Write one
Any MCP server works. With the Python SDK:

```python
from mcp.server.fastmcp import FastMCP
mcp = FastMCP("desk", host="0.0.0.0", port=8765)

@mcp.tool()
def status() -> str:
    """Where things stand."""
    return "all good"

mcp.run(transport="sse")
```

`loamz-solutions` ships ready-made servers for mail triage and paper trading.
