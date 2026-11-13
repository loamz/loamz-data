# Agent API

Endpoints under `/api/v1/agent`.

## `POST` /api/v1/agent/lease

**An agent asks for its next piece of work.**

Waits up to 20 seconds (or `wait_s`, at most 25) for work. The answer carries a run and the token that run's files are read and written with: the person it runs for, narrowed to the tables and volumes it declared, for this workspace only, and dead when the run ends.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `wait_s` | `query` | No | `integer` |  |

### Responses

| Status | Description |
|---|---|
| `200` | A run and its token |
| `204` | Nothing is waiting |
| `401` | An RFC 9457 problem document. |

---

## `POST` /api/v1/agent/runs/{id}/heartbeat

**An agent says it is still working on a run, and hears whether it was cancelled.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | `cancel` says whether to stop |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/agent/runs/{id}/complete

**An agent says how a run ended, with its rows or output.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `id` | `path` | Yes | `string` |  |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `state` (`any`): 
  - `exit_code` (`integer`): 
  - `stdout` (`string`): 
  - `stderr` (`string`): 
  - `result` (`string`): A statement's rows, as JSON text.
  - `error` (`string`): 

### Responses

| Status | Description |
|---|---|
| `204` | Recorded |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---
