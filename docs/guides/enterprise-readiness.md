# Enterprise readiness baseline

This is the baseline for the Rust Data service. Older guides and compliance documents were written for the former Python service and are historical until individually revalidated. They are not evidence of current implementation, deployment, certification or contractual commitments.

| Area | Current state | Evidence and limits |
| --- | --- | --- |
| Identity | Implemented | OIDC verification and Keycloak integration in `server/server/src/kernel/auth.rs`; production identity configuration is not verified here. |
| Tenants and catalog access | Implemented, local tests | Organization/workspace membership, inherited object grants, account admins and delegated-agent intersection. See `server/server/tests/api.rs`. |
| Fine-grained data access | Planned | Row filters, column masks and executable governed views are unavailable. Raw table credentials cannot enforce row or column policies. |
| Iceberg tables | Implemented, local tests | REST catalog and single-table commits; interoperability and recovery on production object storage require validation. Multi-table atomic commits are unsupported. A disposable SeaweedFS check covers S3 file and multipart operations; it does not certify R2. |
| SQL and jobs | Implemented, local tests | Read-only SQL and Python jobs on attached compute. Queued runs recheck workspace, entitlement and object grants before lease; viewing run details rechecks the viewer's current grants. Identity-provider group membership in queued runs is a snapshot from submission, and already issued storage credentials remain valid until they expire. The runner downloads entire table folders. Jobs lack a production pipeline scheduler. |
| Classification and artifacts | Partial | Compute refuses sealed inputs and outputs, and personal inputs require a personal output volume. Classification alone does not restrict who can read that volume; derived-output access inheritance and private artifacts are not yet implemented. |
| Sandbox | Implemented, environment-dependent tests | gVisor tests require a configured runtime and root filesystem. Rootless operation does not enforce memory/process limits; local tests are not a production isolation assessment. |
| Audit | Implemented, local tests | Per-organization hash chains with transactional metadata changes. File bytes and their audit records are not one distributed transaction. Multi-replica behavior requires validation. |
| Lifecycle | Partial | Drop and owner-based erasure are durable tasks. Subject-level deletion from shared tables, derivatives, old snapshots and backups is not implemented. |
| SCIM, personal access tokens, legal holds | Planned | The old Python-era guide described these as built; the current Rust API does not implement them. |
| Recovery, HA and observability | Not production verified | Database health endpoint exists; coordinated backups and restore drills, multi-instance operation, metrics, alerting and incident response need production evidence. |
| Metering and cost controls | Partial | `LOAMZ_DATA_MAX_ACTIVE_RUNS` limits queued and running work per organization (default 32), with admission serialized in the database. Storage metering, billing and spend caps are not implemented. |
| Compliance | Not certified | Documentation does not establish SOC 2, ISO 27001, penetration-test, residency, or customer-managed-key claims. |

The Rust advisory check has a documented exception for RUSTSEC-2023-0071: `rsa` is used by the local test-key generator and locked for an unused SQLx backend. CI also fails if `rsa` enters a normal or build dependency tree. The advisory has no fixed release; review this exception when dependencies or test-key generation change. `unsafe-libyaml`, a test dependency, is reported as unmaintained. A raw `cargo audit` still reports the RSA advisory.

## Reproducing checks

`make check` runs the default local checks. The PostgreSQL suite creates temporary test databases and requires a **throwaway** PostgreSQL instance whose user may create databases:

```sh
LOAMZ_TEST_POSTGRES_URL=postgres://data:data-dev-only@127.0.0.1:55434/data \
  cargo test --locked -p loamz-data-server -- --include-ignored
```

The test harness does not drop its PostgreSQL databases; use an isolated instance that can be discarded. The default Rust test run reports those tests as ignored. A passing default run does not prove PostgreSQL behavior.

The gVisor integration tests are explicitly ignored in the default suite. To run all of them, set `LOAMZ_RUNSC` and `LOAMZ_ROOTFS`, run with cgroup enforcement (not `LOAMZ_ROOTLESS`), and use `cargo test -p loamz-runner --test gvisor -- --ignored --test-threads=1`. Rootless runs cannot establish memory or process limits. R2 and browser tests require their own isolated test resources and credentials; consult the scripts before running them.

On 2026-11-02, local `scripts/budget.sh` runs reported 966 ms and 1,020 ms to readiness against the script's 200 ms target, with 11 MB idle memory against its 30 MB target. These are local measurements on WSL2. The same measurement on the server binary built before the object-store upgrade (the previous dependency set) gave 1,020–1,043 ms, so the upgrade did not cause it. The local sign-in service answers in about 10 ms and the server's own startup work (migrations, signing keys) is done within about 60 ms, but the port is not open until about 1,030 ms: something in `main` between loading the signing keys and binding the listener takes about a second, and is not yet found. Measure on the intended deployment infrastructure too. ARM binaries cross-compiled for glibc, but were not executed on ARM.

The [production roadmap](../production-roadmap.md) records remaining work and release gates. Update this baseline only with evidence from the current implementation and environment.
