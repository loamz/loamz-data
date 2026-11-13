# Me API

Endpoints under `/api/v1/me`.

## `GET` /api/v1/me

**Who the token says the caller is, their organizations (including their own space) and the workspaces they can open.**

A person's own space and a workspace in it are made on first use.

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |

---
