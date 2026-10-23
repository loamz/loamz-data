# Incident response

For a security or availability incident affecting a cell or this project's supply chain. Operators of a cell adopt it as
their runbook and fill in their own contacts.

## Severity
| Level | Meaning | Examples | Start within |
| --- | --- | --- | --- |
| SEV1 | Confirmed breach, data loss or full outage | Credential leak with evidence of use; cross-tenant data exposure; data plane down | 15 minutes, around the clock |
| SEV2 | Probable security issue or major degradation | Unconfirmed suspicious access; a critical vulnerability under active exploitation upstream; a failed restore drill | 1 hour |
| SEV3 | Contained or minor | A single failed deploy; a high vulnerability with no exploit path | Next business day |

## Roles
**Incident lead** decides and records. **Communications** informs customers and the public. **Scribe** keeps a timestamped
log. **Subject experts** investigate. One person can hold several roles in a small team; the lead is never the person
debugging.

## Steps
1. **Detect and declare.** From an alert, a report to the security contact, a failing check, or a customer. Open an
   incident record and name the lead.
2. **Contain.** Stop the harm first: revoke access tokens (`DELETE /api/v1/access-tokens/{id}`; administrators can revoke
   any), rotate secrets and client credentials, disable users through SCIM or the identity provider, isolate a workload
   with a network policy, or pause a compute resource. Prefer reversible actions.
3. **Preserve evidence.** Export the audit log (`audit-events:export`) and verify it (`audit-events:verify`), keep
   container and pod logs, and place a **legal hold** on affected tenants so erasure and retention cannot remove
   evidence.
4. **Assess.** What was accessed, by whom, since when, for which tenants. The audit events and request ids answer most of
   it.
5. **Eradicate and recover.** Fix the cause, redeploy from signed images, restore from backups if data was changed
   (`disaster-recovery.md`), and rotate anything that could have been seen.
6. **Notify.** Affected customers without undue delay once confirmed, and regulators where required (for personal data
   under GDPR, within 72 hours of becoming aware). Security advisories for this project go through GitHub's private
   advisory flow in `SECURITY.md`.
7. **Review.** Within five business days write the post-incident review (below), assign follow-ups with owners and dates,
   and track them to completion.

## Post-incident review template
- **Summary** in two sentences, severity, duration.
- **Impact:** who and what, with numbers.
- **Timeline:** detection, declaration, containment, resolution (UTC).
- **Cause:** what happened and why it was possible. Blameless.
- **What worked, what did not.**
- **Follow-ups:** action, owner, due date.
