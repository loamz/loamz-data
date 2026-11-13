# Orgs API

Endpoints under `/api/v1/orgs`.

## `POST` /api/v1/orgs

**Make an organization and its first admin. Only the installation's admin (the Keycloak role `platform-admin`) can.**

The first admin is made in the directory if new, with a temporary password shown once in the answer; they must change it at their first sign-in. Asking again for an organization that exists is a conflict.

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `alias` (`string`): 2 to 40 lower-case letters, digits or dashes.
  - `admin_email` (`string`): 
  - `admin_name` (`string`): 

### Responses

| Status | Description |
|---|---|
| `201` | Created |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |
| `503` | An RFC 9457 problem document. |

---

## `POST` /api/v1/orgs/{org}/account

**Create the Data account of an organization the caller belongs to. Needs the Keycloak role `account-admin`; the creator becomes its first admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Responses

| Status | Description |
|---|---|
| `201` | Created |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `GET` /api/v1/orgs/{org}/admins

**Who manages the account (account admins).**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/orgs/{org}/admins

**Make a principal an account admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `principal` (`string`): 

### Responses

| Status | Description |
|---|---|
| `204` | Added |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/orgs/{org}/admins/{principal}

**Take away admin rights. An account always keeps one admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |
| `principal` | `path` | Yes | `string` | user:<id>, group:<path> or sp:<id>, percent-encoded. |

### Responses

| Status | Description |
|---|---|
| `204` | Removed |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `POST` /api/v1/orgs/{org}/people

**Add a person to the organization. Account admins only.**

A person who already exists is only added. A new one is made with a temporary password, shown once in the answer.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `email` (`string`): 
  - `name` (`string`): 

### Responses

| Status | Description |
|---|---|
| `201` | Added |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `503` | An RFC 9457 problem document. |

---

## `GET` /api/v1/orgs/{org}/workspaces

**The workspaces of the organization the caller can open.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/orgs/{org}/workspaces

**Create a workspace. Account admins only; the creator becomes its admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `name` (`string`): 

### Responses

| Status | Description |
|---|---|
| `201` | Created |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `POST` /api/v1/orgs/{org}/erase

**Erase a principal's data in this organization. Account admins.**

Queues a durable task that removes everything the principal owns, their files, grants, workspace assignments and admin rights, and checks it is gone. The audit trail is kept.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `subject` (`string`): A typed principal such as user:<id>.

### Responses

| Status | Description |
|---|---|
| `202` | An erase task was queued |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/orgs/{org}/tasks/{id}

**The state of a drop or erase task. Account admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/orgs/{org}/audit

**The organization's recent audit entries, newest first. Account admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |
| `limit` | `query` | No | `integer` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/orgs/{org}/audit/verify

**Walk the organization's whole hash chain. Account admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` | An organization id: the Keycloak organization id, or personal:<subject> for your own space. |

### Responses

| Status | Description |
|---|---|
| `200` | `intact`, the entry count, and the first broken entry if any |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---
