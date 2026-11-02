# Loamz Data production roadmap

Status: work in progress. The [readiness baseline](guides/enterprise-readiness.md) describes the current Rust implementation. A checked build or historical design document is not production acceptance.

Current progress: milestone 1 has a reconciled baseline and CI configuration, but a fresh-environment and hosted-CI run remain to be verified. Milestone 2 is in progress: queued-run reauthorization, bounded per-organization admission, selected classification checks, dependency updates, and bounded runner I/O have been added. Milestones 3–7 remain open; existing jobs and the earlier analysis proposal do not satisfy their release gates.

The target is a Loamz-managed, bounded public SaaS for data engineering, SQL and governed Python/AI analytics. Data remains independent of Chat and Agents; external clients use the same authorization boundary. Use Iceberg and proven engines and integrations behind versioned Loamz interfaces.

## Now / Next / Parked across Loamz

This overview coordinates the repositories; it does not claim their work is complete. Detailed Data milestones and gates follow below.

**Now — make the foundation trustworthy**

- **Data:** verify the baseline, close tenant, execution, classification and recovery gaps, and make the current service supportable.
- **Agents:** maintain a separate runtime and establish delegated user identity when accessing Data; do not treat a shared deployment credential as individual authorization.
- **Chat:** keep messaging and user consent in Chat; verify the existing communication and security flows independently of Data.
- **Platform and deployment:** clarify the managed deployment boundary, identity, operations, backups and service health across products.

**Next — prove the first complete product**

- **Data:** deliver a reliable ingestion-to-pipeline-to-table workflow and governed SQL.
- **Agents and Data:** deliver governed upload, join, isolated Python, chart and derived-dataset analysis through the same Data policies for people and agents.
- **Chat and Agents:** connect the governed analysis experience to conversations without putting Data authorization or storage rules in the chat client or model.
- **Platform and deployment:** connect account provisioning, data usage limits, billing, recovery, support and release controls for a bounded public service.

Data engineering and governed AI analytics remain **joint requirements** for the controlled public SaaS release. The release gate also requires the isolation, recovery, billing and operational evidence described below.

Sandbox launch gate: run the actual gVisor sandbox on production-class Linux with cgroup enforcement and demonstrate CPU, memory and process limits, no network by default, termination, and cleanup. Rootless or WSL-only checks do not satisfy this gate.

**Parked until the first product is reliable**

Distributed compute, broad streaming and connector coverage, model serving, advanced graph capabilities, a marketplace, broad multi-cloud parity and customer-hosted deployments. Reconsider each against measured demand and its own acceptance gate.

## 1. Trustworthy baseline — M

Reconcile the implementation, API, deployment and historical designs; keep readiness claims evidence-backed. Add CI for Rust, frontend, PostgreSQL, API contracts, dependency boundaries and security checks. Record supported versions, deployment topology and current workload limits. Gate: reproduce supported workflows from a fresh environment.

## 2. Data and execution foundation — XL

Verify tenant and workspace isolation; enforce session, permission, sandbox and resource limits. Add admission controls and verify Iceberg conflict handling and interrupted commits. Establish safe maintenance, backups, recovery, key rotation, monitoring and rollback. Enforce classification and opaque sealed data. Gate: isolation, restore, failure and exhaustion tests on intended production infrastructure.

## 3. Reliable data engineering — XL

Support CSV/Parquet uploads and scheduled PostgreSQL ingestion, schema handling, checkpoints, deduplication and backfills. Add versioned SQL/Python pipelines with dependencies, schedules, retries, cancellation and service identities. Publish governed atomic table commits; add quality checks, quarantine, lineage and run history. Gate: a pipeline recovers from worker failure and retry without duplicate or partial publication.

## 4. Governed AI and Python analytics — XL

Build a model-independent analysis service with HTTP, Python SDK and MCP adapters; support private uploads, governed joins, isolated Python, charts and derived datasets. Enforce delegated user identity, credential-free code, private outputs, inherited restrictions and reproducibility. Governed views and row/column policies require policy-enforced materialization, not raw table-folder credentials. Gate: a second unauthorized user cannot read any output from a private analysis.

## 5. Analytics workbench — L

Improve discovery, descriptions, quality and lineage, query diagnostics and permission-aware results. Replace full table-folder downloads with snapshot-aware selective reads. Add reproducible analysis documents and charts, a governed SQL connection for Superset, and accessible states. Gate: a user can discover, understand, query and visualize data within their permissions.

## 6. Controlled public SaaS — XL

Connect signup, provisioning, invitations and closure. Add data entitlements, durable metering, reconciliation, spend caps and abuse controls. Validate production recovery, replicas, rate limits, scheduling fairness, identity, TLS and browser security. Publish support and service objectives. Gate: stages 1–5, operational soak, restore drill, billing reconciliation and independent security review pass at documented limits.

## 7. Enterprise and scale — XL, ongoing

In order: enterprise identity and provisioning; record-level privacy, retention and legal holds; CDC and streaming; distributed compute when benchmarks require it; MLflow and model governance; governed retrieval and AI evaluation; private networking, residency, customer-managed keys, customer hosting and external sharing. Each capability has its own acceptance gate.

Every release must test cross-tenant denial, permission changes, retries, worker loss, interrupted commits, cancellation, quota exhaustion and recovery. Pipeline tests validate persisted outputs; AI tests validate output access and inherited restrictions.
