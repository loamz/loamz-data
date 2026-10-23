# Trust center (content for a public page)

Everything below is true of the software today. Replace the bracketed parts with your organization's facts before
publishing, and add only claims you can evidence.

## Security
Loamz runs as a **cell** in your own cloud account. Your data stays in the region you choose, in object storage and a
database you own. We (or your operator) do not copy it elsewhere.

- Single sign-on through your identity provider (OIDC; SAML and LDAP through brokering); user and group provisioning with
  SCIM 2.0; short-lived delegated tokens for agents.
- Per-tenant isolation, fine-grained grants, row filters and column masks.
- Encryption in transit everywhere and at rest by default, with optional customer-managed keys on AWS, Google Cloud and
  Azure.
- A tamper-evident, hash-chained audit log you can export to your SIEM and verify at any time.
- Non-root, read-only containers; network policies; sandboxed code execution without network access.

## Reliability
Replicated services, disruption budgets, an elected controller with automatic takeover, managed multi-AZ database
option, versioned object storage, and a restore drill that runs every week and checks that a backup restores into an
intact catalog.

## Supply chain
Signed container images with SBOM and provenance; keyless package publishing; code scanning, dependency review and
vulnerability audits on every change; an OpenSSF Scorecard.

## Privacy
Export and erasure by person or tenant; legal hold for matters that require preserving data; a data-processing
agreement is available on request [link].

## Compliance
[State only what you hold: for example "SOC 2 Type II report available under NDA, period …" or "ISO 27001 certificate
no. …, scope …". If a program is in progress, say so with the expected date. Do not claim a framework you have not been
assessed against.]

## Reporting a vulnerability
See `/.well-known/security.txt` and `SECURITY.md`. We acknowledge within 3 business days and fix critical issues within
7 days (`vulnerability-management.md`).

## Support and status
Severity definitions and response targets are in `SUPPORT.md`. [Link your status page.]
