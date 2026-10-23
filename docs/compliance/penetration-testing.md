# Penetration testing

## What we test ourselves, continuously
Authorization and tenant isolation have dedicated tests: a user or tenant never reads another's resource (404, not
403); delegated tokens are limited to their scopes; access tokens never act as administrators and cannot create tokens;
SCIM cannot reach another tenant's users or groups; legal holds cannot be bypassed by erasure; the audit chain detects
alteration. Static analysis and dependency scanning run on every change (`vulnerability-management.md`).

## Third-party test: scope for an engagement
**In scope**
- The Loamz Data API (`/api/v1`, `/scim/v2`) and the Loamz Agents API (`/api/v1`) of a staging cell with at least two
  tenants and one administrator per tenant.
- Authentication and authorization: token validation, tenant isolation, scope enforcement, access tokens, SCIM.
- File upload and download paths (presigned URLs), document and memory features, the SQL engine and its policies (row
  filters, column masks, view definer rights), compute attachment.
- The two web consoles' sign-in, session and request-forwarding behaviour.
- The code interpreter sandbox (escape, resource abuse, network access).
- The Helm chart's defaults: network policies, pod security, secrets handling.

**Out of scope:** the cloud provider and the identity provider's own infrastructure, social engineering, denial of
service at volumes that affect other tenants of shared infrastructure.

**Rules of engagement:** a dedicated staging cell with synthetic data, written authorization from the operator, named
testers and source IPs, a stated window, an emergency contact, and no testing of production without a separate
agreement. Findings go to the security contact over a private channel; the operator confirms each with a severity under
`vulnerability-management.md`.

## Deliverables to ask for
An executive summary, findings with reproduction steps, severity and remediation advice, a retest of fixed findings, and
a letter of attestation that can be shared with customers. Keep the report under confidentiality; publish the letter.

## Cadence
At least yearly and after a major architectural change (a new identity flow, a new data path). Track every finding to
closure and keep the evidence for the auditor.
