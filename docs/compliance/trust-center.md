# Trust center draft

This is a checklist for a future public trust page, not a statement of current capabilities or commitments. Before publishing any claim, verify it against the deployed system, supporting evidence, and the operating organization's policies. The [readiness baseline](../guides/enterprise-readiness.md) records the current Data service's known state.

## Security

Document the actual deployment model, regions, identity options, access controls, agent delegation, sandbox isolation, encryption, key management, and audit access. Do not claim SCIM, row filters, column masks, customer-managed keys, or customer-hosted deployment until the relevant implementation and deployment have been verified.

## Reliability

Describe the measured service objectives, backups, recovery procedures, restore exercises, incident handling, and status page only after they are operating and evidence is available.

## Supply chain

Describe the checks, dependency audits, image signing, and provenance that are actually enabled for the shipped release. A workflow file alone is not evidence that a check ran or passed.

## Privacy and compliance

Describe supported retention, export, deletion, and legal-hold behavior at its verified scope. State certifications and audit reports only when held by the operating organization and within their actual scope and period.

## Reporting and support

Provide monitored vulnerability-reporting and support channels, response commitments, and the status page after the organization has established them.
