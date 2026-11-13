# Auth API

Endpoints under `/api/v1/auth`.

## `POST` /api/v1/auth/token

**The sign-in token exchange, done by the server for the browser.**

Every workspace is its own website, and the identity provider only lets exact addresses call its token endpoint from a page. The page asks its own server, which asks the identity provider directly. Only `authorization_code` (with `code`, `redirect_uri`, `code_verifier`) and `refresh_token` pass, always as the `loamz-data` client. Needs no token: it is how a page gets one.

### Request Body

- **Content-Type**: `application/json` (`object`)

  **Fields:**

  - `grant_type` (`any`): 
  - `code` (`string`): 
  - `redirect_uri` (`string`): 
  - `code_verifier` (`string`): 
  - `refresh_token` (`string`): 

### Responses

| Status | Description |
|---|---|
| `200` | The identity provider's token answer |
| `400` | An RFC 9457 problem document. |
| `502` | An RFC 9457 problem document. |
| `503` | An RFC 9457 problem document. |

---
