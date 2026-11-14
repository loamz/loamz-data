# Workspaces API

Endpoints under `/api/v1/workspaces`.

## `GET` /api/v1/workspaces/{ws}

**One workspace, and the caller's standing in it.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}

**Rename a workspace (account admins).**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `name` (`string`): 

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}

**Delete a workspace and its assignments. A workspace that still owns catalogs cannot be deleted.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `204` | Deleted |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/members

**Who is assigned to the workspace, and as what. Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/members

**Assign a person, group or service principal as `admin` or `user`, or change their role.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `principal` (`string`): 
  - `role` (`any`): 

### Responses

| Status | Description |
|---|---|
| `204` | Assigned |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/members/{principal}

**Take a principal out of the workspace. A workspace keeps one admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
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

## `GET` /api/v1/workspaces/{ws}/service-principals

**The workspace's service principals (never their tokens). Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/service-principals

**Make a service principal (a BI tool, an ML app, a pipeline) of this workspace. Workspace admins.**

Its token is in the answer, once. It starts with `lsp_`, is sent as a bearer token like any other, and opens only this workspace. The principal is `sp:<id>`; give it grants and entitlements like anyone's.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

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

## `DELETE` /api/v1/workspaces/{ws}/service-principals/{id}

**Remove a service principal; its token stops working at once.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Removed |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/service-principals/{id}/token

**Give a service principal a new token (in the answer, once); the old one stops working at once.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/storage/credentials

**How the server reaches stores for this workspace (references into the vault, never secrets). Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/storage/credentials

**Add a storage credential. The vault must hold the secrets it names. Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `name` (`string`): 
  - `endpoint` (`string`): The store's S3 address, https (or http on this computer).
  - `region` (`string`): 
  - `key_id_ref` (`string`): A reference into the vault, like loamz-data/bucket#key_id.
  - `secret_ref` (`string`): 
  - `vending` (`object`): 

### Responses

| Status | Description |
|---|---|
| `201` | Created |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |
| `503` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/storage/credentials/{id}

**Remove a credential no location uses.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/storage/credentials/{id}

**Read non-secret credential metadata.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Credential ID. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/storage/credentials/{id}/check

**Check a location without returning the credential.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Credential ID. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `location_id` (`string`): A registered storage location that uses this credential.

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `413` | An RFC 9457 problem document. |
| `422` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/storage/credentials/{id}/secret

**Replace the complete secret if the generation matches.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Credential ID. |
| `If-Match` | `header` | Yes | `string` | The current generation (integer string). |

### Request Body

- **Content-Type**: `application/json` (`object`, write-only provider secret fields)

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `412` | Precondition Failed |
| `413` | An RFC 9457 problem document. |
| `503` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/storage/locations

**The buckets and prefixes this workspace may use. Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/storage/locations

**Approve a bucket or a prefix of one. It may overlap no other location of the installation. Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `name` (`string`): 
  - `credential_id` (`string`): 
  - `bucket` (`string`): 
  - `prefix` (`string`): A folder in the bucket, with no leading or trailing slash; empty is the whole bucket.

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

## `DELETE` /api/v1/workspaces/{ws}/storage/locations/{id}

**Remove a location.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/entitlements

**What assigned principals are entitled to.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/entitlements

**Give an entitlement (`sql-access`, `compute-create`, `sandbox-access`) to an assigned principal.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `principal` (`string`): 
  - `entitlement` (`any`): 

### Responses

| Status | Description |
|---|---|
| `204` | Given |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/entitlements/{principal}/{entitlement}

**Take an entitlement away.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `principal` | `path` | Yes | `string` | user:<id>, group:<path> or sp:<id>, percent-encoded. |
| `entitlement` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Removed |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/securables

**List what is directly inside a parent (the metastore when `parent_id` is empty) that the caller may see from this workspace.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `parent_id` | `query` | No | `string` | Empty for catalogs. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/securables

**Create a catalog, schema, table, volume or view. Needs the matching CREATE privilege on the parent (CREATE CATALOG on the metastore). The creator owns it.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `kind` (`any`): 
  - `parent_id` (`string`): 
  - `name` (`string`): Lower case letters, digits and underscores, starting with a letter.
  - `classification` (`any`): 
  - `placement` (`any`): 
  - `consistency` (`any`): A volume can only promise snapshot.
  - `location` (`string`): A catalog only. The name of one of the workspace's storage locations (in the installation's own bucket) to keep its data in. Leave empty for the workspace's default storage. Workspace admins only.

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

## `GET` /api/v1/workspaces/{ws}/securables/{id}

**One entry.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/securables/{id}

**Drop an entry and everything inside it. Needs MANAGE. The entry disappears at once; the bytes are removed by a durable task.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Responses

| Status | Description |
|---|---|
| `202` | A drop task was queued |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/securables/{id}/owner

**Hand an entry to a new owner. Needs MANAGE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `owner` (`string`): 

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/securables/{id}/policy

**The row filter and column masks of a table (empty when it has none). Needs MANAGE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Responses

| Status | Description |
|---|---|
| `200` | {row_filter, masks} |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/securables/{id}/policy

**Set the rules of a table, replacing any before them; an empty body removes them. `row_filter` keeps only the rows where it is true; `masks` shows an expression in place of a column. A rule may use columns, values, a few plain functions, `current_principal()` and `in_group('name')`; anything else is refused. Whoever manages the table sees it whole; everyone else reads it only through SQL on Loamz compute, where the rules are applied (a job or an Iceberg client is refused). Needs MANAGE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Request Body

- **Content-Type**: `application/json` (`object`)

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/securables/{id}/grants

**Who holds what directly here. Needs MANAGE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/securables/{id}/grants

**Give a principal a role (reader, contributor, owner) here. The USE privileges are placed on the catalog and schema above. Needs MANAGE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `principal` (`string`): 
  - `role` (`any`): 

### Responses

| Status | Description |
|---|---|
| `204` | Granted |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/securables/{id}/grants/{principal}

**Take away everything a principal holds directly here. Needs MANAGE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |
| `principal` | `path` | Yes | `string` | user:<id>, group:<path> or sp:<id>, percent-encoded. |

### Responses

| Status | Description |
|---|---|
| `204` | Revoked |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/volumes/{id}/files

**List the files of a volume. Needs READ VOLUME.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/volumes/{id}/files/{path}

**Download a file, always as an attachment. Needs READ VOLUME.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |
| `path` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/volumes/{id}/files/{path}

**Upload a file (the request body is its bytes). Needs WRITE VOLUME.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |
| `path` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `201` | Stored |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `413` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/volumes/{id}/files/{path}

**Delete one file. Needs WRITE VOLUME.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | A securable id, or `metastore` where grants are concerned. |
| `path` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Deleted |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/compute/targets/{id}/start

**Start serverless compute now (it also starts when a statement arrives). Workspace admins. A machine is started on the machine itself.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` |  |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `503` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/compute/targets/{id}/stop

**Stop serverless compute now (it also stops a minute after its last statement). Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `org` | `path` | Yes | `string` |  |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `503` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/catalogs/{catalog}/compute

**Which of the workspace's compute may read this catalog. An empty list means any of it.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `catalog` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | The compute ids |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/catalogs/{catalog}/compute

**Say which of the workspace's compute may read this catalog (an empty list means any). Needs MANAGE on the catalog.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `catalog` | `path` | Yes | `string` |  |

### Request Body

- **Content-Type**: `application/json` (`object`)

### Responses

| Status | Description |
|---|---|
| `204` | Set |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/quota

**The workspace's own cap on queued and running runs (null means none besides the organization's).**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | The cap |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/quota

**Set the workspace's cap on queued and running runs, or null to remove it. Account admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

### Responses

| Status | Description |
|---|---|
| `204` | Set |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/compute/providers

**The kinds of compute this server can create (a serverless SQL warehouse, a machine of your own) and whether each is available.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/iceberg/v1/config

**The Iceberg REST catalog's configuration. The warehouse is the name of a catalog.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `warehouse` | `query` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/namespaces

**List the schemas of a catalog (Iceberg namespaces).**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/namespaces

**Create a schema. Needs CREATE SCHEMA.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/namespaces/{ns}

**Whether a schema is there for the caller.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |
| `ns` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/namespaces/{ns}/tables

**List the tables of a schema.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |
| `ns` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/namespaces/{ns}/tables

**Create an Iceberg table, or stage one (stage-create) for a commit that completes it. Needs CREATE TABLE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |
| `ns` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/namespaces/{ns}/tables/{t}

**Load a table, with its metadata and credentials for its folder only (read-only unless the caller has MODIFY). Needs SELECT.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |
| `ns` | `path` | Yes | `string` |  |
| `t` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/namespaces/{ns}/tables/{t}

**Commit a change to a table (or finish creating it). Needs MODIFY. A change made on an older version is a 409.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |
| `ns` | `path` | Yes | `string` |  |
| `t` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/iceberg/v1/{prefix}/transactions/commit

**Commit one table's change, as Iceberg clients send it.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `prefix` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/tables

**Create an Iceberg table from a list of columns. Needs CREATE TABLE.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/tables/{id}/stats

**Per-column counts, nulls and ranges from what the table's files recorded about themselves; no data is read and no compute is used. Needs SELECT.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/tables/{id}

**A table's columns and snapshots. Needs only that the caller can see it.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` |  |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/tables/{id}/maintenance

**Queue table compaction or snapshot expiry. Needs MODIFY and no applicable row or column rules.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Target table ID. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `action` (`string`): Either `compact` or `expire`.
  - `older_than` (`integer`): For `expire`, snapshot retention cutoff in seconds (defaults to 604800, max 31536000).

### Responses

| Status | Description |
|---|---|
| `202` | Accepted (returns queued run) |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/compute/targets

**The compute attached to this workspace, and whether each is answering.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | The targets |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/compute/targets

**Attach compute to this workspace. Workspace admins.**

The answer carries the agent's secret, once. Only a hash of it is kept.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `name` (`string`): 
  - `max_memory_mb` (`integer`): Default 2048.
  - `max_cpu_millis` (`integer`): Default 1000.

### Responses

| Status | Description |
|---|---|
| `201` | Attached, with its secret |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/sql/statements

**Run a SQL statement and wait a while for its rows.**

Needs the `sql-access` entitlement and `SELECT` on every table it reads. The statement is parsed first: only one plain query is accepted, and every table must be named `catalog.schema.table`. It comes back finished (200) or still running (202), with the run's id to ask about. When input access cannot be verified the response has result_access=restricted and omits rows and output.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `sql` (`string`): 
  - `limit` (`integer`): Rows to return, at most 10000 (default 1000).
  - `target` (`string`): 
  - `wait_s` (`integer`): Seconds to wait for the answer, at most 60 (default 30).

### Responses

| Status | Description |
|---|---|
| `200` | The finished run, with its rows |
| `202` | Still running |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |
| `429` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/imports

**Bring a file in as a table. A job the server writes runs on the workspace's compute, reads the file from a volume and writes the table through the catalog. Needs READ on the volume and CREATE TABLE on the schema (MODIFY for an append). Answers 202 with the run.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

### Responses

| Status | Description |
|---|---|
| `202` | The run that does the import |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/runs

**The workspace's runs, newest first: your own, or everyone's for a workspace admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `kind` | `query` | No | `string` |  |
| `by` | `query` | No | `string` | Only what this person asked for (a workspace admin's narrowing; everyone else is shown their own). |
| `target` | `query` | No | `string` | Only runs on this compute. |
| `state` | `query` | No | `string` | queued, running, succeeded, failed, cancelled or timed_out. |
| `since_ms` | `query` | No | `integer` | Only runs asked for at or after this time. |
| `before_ms` | `query` | No | `integer` | The page cursor: the created_ms of the oldest run of the page before. |
| `limit` | `query` | No | `integer` |  |

### Responses

| Status | Description |
|---|---|
| `200` | The runs, without their rows or output |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/runs

**Run a job: a Python script with read-only inputs and one output volume.**

Needs the `compute-create` entitlement, `SELECT` or `READ VOLUME` on each input and `WRITE VOLUME` on the output.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `title` (`string`): 
  - `script` (`string`): 
  - `inputs` (`array`): Table or volume ids, mounted read-only under /inputs/<name>.
  - `output` (`string`): A volume id: what the script leaves in /workspace is saved there under runs/<run id>/. Leave it out when the job only writes tables.
  - `table_writes` (`array`): Tables the script may write, as catalog.schema.table: one that exists (needs MODIFY) or one to create (needs CREATE TABLE on its schema). The sandbox has no network; it reaches the catalog's routes for these tables and storage for their folders through two narrow doors (see proposal 0005). A table with row or column rules cannot be written.
  - `target` (`string`): 
  - `memory_mb` (`integer`): 
  - `cpu_millis` (`integer`): 
  - `timeout_s` (`integer`): 

### Responses

| Status | Description |
|---|---|
| `202` | Queued |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |
| `429` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/queries

**Saved queries, newest change first: your own, or everyone's for a workspace admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | The saved queries |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/queries

**Save a query (a name and the SQL).**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

### Responses

| Status | Description |
|---|---|
| `201` | Saved |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/queries/{id}

**One saved query.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | The saved query |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/queries/{id}

**Change a saved query.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Request Body

- **Content-Type**: `application/json` (`object`)

### Responses

| Status | Description |
|---|---|
| `200` | Saved |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/queries/{id}

**Delete a saved query.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Deleted |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/runs/{id}

**One run, with its history and, when authorized, its rows and output.**

When input access cannot be verified, result_access is restricted and results, logs, errors and input metadata are omitted. A directly submitted SQL statement remains visible to its submitter. Full details use result_access=available.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | The run with result_access indicating whether output is available. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/volumes/{id}/threads/{thread}

**Delete a thread's files in a volume (`threads/<thread>/`).**

Needs WRITE on the volume. In this order: the thread is closed to new saved results and committed outputs, the job that holds it is cancelled, and its files are deleted. It stays closed for ten minutes (409 for new work) and its files are deleted once more then, so nothing a late machine wrote remains.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |
| `thread` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Deleted |
| `400` | An RFC 9457 problem document. |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/runs/{id}/result

**The whole result of a statement that was run with `export`, as a short-lived signed address.**

Needs the same right as the run's detail. The answer is `{url, bytes, format, expires_ms}`: fetch `url` with no credential (the bytes go from the storage to the reader, never through this server). The file is an Arrow IPC stream and is removed 24 hours after the run ends, or at once if the person who ran it is erased.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | The signed address |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/runs/{id}/cancel

**Stop a run: at once if it has not started, otherwise when its compute next checks in.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `202` | Stopping |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |
| `409` | An RFC 9457 problem document. |

---

## `DELETE` /api/v1/workspaces/{ws}/compute/targets/{id}

**Remove compute. What was waiting or running on it fails. Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `204` | Removed |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/compute/targets/{id}/secret

**Give the compute a new secret; the agent using the old one is turned away. Workspace admins.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` |  |

### Responses

| Status | Description |
|---|---|
| `200` | The new secret, once |
| `401` | An RFC 9457 problem document. |
| `403` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/schedules

**List schedules owned by the caller, or all schedules for a workspace admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `POST` /api/v1/workspaces/{ws}/schedules

**Schedule an immutable SQL statement, job, or table maintenance action as the current person.**

Give one UTC cron expression or interval_seconds, and exactly one of sql, job, or maintenance. webhook_ref is an optional vault reference and is never returned.

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `name` (`string`): A display name for the schedule.
  - `cron` (`string`): A 5-part UTC cron expression (e.g. `*/15 * * * *`).
  - `interval_seconds` (`integer`): Repeat interval in seconds (minimum 60).
  - `sql` (`string`): A SQL statement to run on each tick.
  - `job` (`object`): A sandboxed job specification.
  - `maintenance` (`object`): Table maintenance action (`table`, `action: compact | expire`, optional `older_than_seconds`).
  - `webhook_ref` (`string`): Optional secret reference in the vault for failure alert notifications.

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

## `DELETE` /api/v1/workspaces/{ws}/schedules/{id}

**Delete a schedule. Its owner or a workspace admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Schedule ID. |

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/schedules/{id}/pause

**Pause a schedule. Its owner or a workspace admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Schedule ID. |

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `PUT` /api/v1/workspaces/{ws}/schedules/{id}/resume

**Resume a schedule from its next future firing. Its owner or a workspace admin.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Schedule ID. |

### Responses

| Status | Description |
|---|---|
| `204` | Done |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---

## `GET` /api/v1/workspaces/{ws}/schedules/{id}/alerts

**List recorded failure alerts for a schedule.**

### Parameters

| Name | In | Required | Type | Description |
|---|---|---|---|---|
| `ws` | `path` | Yes | `string` | A workspace id. |
| `id` | `path` | Yes | `string` | Schedule ID. |

### Responses

| Status | Description |
|---|---|
| `200` | OK |
| `401` | An RFC 9457 problem document. |
| `404` | An RFC 9457 problem document. |

---
