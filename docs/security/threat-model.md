# Loamz Data Threat Model

This document outlines the security architecture and threat model for Loamz Data across all system trust boundaries, detail what stops attacks today, and catalog planned mitigations.

---

## 1. Trust Boundaries

```
                 ┌─────────────────────────────────────────────────────────┐
                 │                       Keycloak                          │
                 └──────┬───────────────────────▲────────────────────▲─────┘
        OIDC Tokens     │                       │                    │ RFC 8693
                        ▼                       │                    │ Token Exchange
┌──────────┐     ┌──────────────┐        ┌──────┴──────┐      ┌──────┴──────┐
│ Browser  ├────►│  loamz-data  │◄───────┤ OpenBao     │      │ Loamz Agents│
│ Console  │     │  (Server)    │        │ (Vault)     │      │ (Tool app)  │
└──────────┘     └───┬───────▲──┘        └─────────────┘      └─────────────┘
                     │       │
       Vended Creds  │       │ Long-poll Claim
                     ▼       │
           ┌───────────┐   ┌─┴────────────┐
           │ Object    │   │ loamz-runner │
           │ Store(S3) │   │ (Host Agent) │
           └───────────┘   └──┬───────────┘
                              │ gVisor runsc
                              ▼
                           ┌──────────────┐
                           │ Sandbox      │
                           │ (Query Code) │
                           └──────────────┘
```

---

## 2. Threat Analysis by Boundary

| Boundary | Asset | Who can attack | What stops them (real file & test) | What does not yet (risks) |
|---|---|---|---|---|
| **1. Browser to Server** | Workspace data, catalogs, query history, audit log | Untrusted caller, session hijacker, rogue tenant user | • `server/server/src/auth.rs`: validates JWT signature, issuer, expiry, and organization claims.<br>• `server/server/src/iam/service.rs`: strictly enforces RBAC privileges (`SELECT`, `MODIFY`). A user without access receives HTTP 404 (never 403), preventing securable enumeration.<br>• `tests/groups_hold_privileges.rs`: asserts callers cannot read or mutate securables outside granted roles. | • Per-workspace rate limiting / API throttling is not yet enforced.<br>• Web session CSRF relies entirely on Bearer tokens (safe for API, but requires strict web client token hygiene). |
| **2. Server to Runner** | Compute work queue, job execution leases, runner credentials | Rogue machine, spoofed runner, intercepted work claims | • `server/runner/src/agent.rs`: runner establishes an outbound HTTPS connection only; no inbound ports are opened on compute hosts.<br>• `server/server/src/compute/service.rs`: agent secret (`agt_...`) verified using constant-time hash comparison; work assigned strictly to authorized machine target.<br>• `server/runner/src/work.rs`: `paths_from_the_server_stay_under_the_folder` test verifies that file paths returned from the server cannot escape the runner working root. | • Dynamic runner heartbeat re-authentication before every task execution is not yet continuous.<br>• Mutual TLS between server and runner is not yet configured (uses token secret over TLS). |
| **3. Runner to Sandbox** | Host system files, compute host kernel, process memory, network | Malicious SQL/Python query payload, fork bomb, resource exhaustion attack | • `server/runner/src/exec.rs`: gVisor (`runsc`) virtualized application kernel isolates user processes from host Linux kernel.<br>• `server/runner/src/spec.rs`: network namespace completely isolated (`--network=none`); rootfs mounted read-only; temp workspace isolated on memory-capped tmpfs.<br>• `server/runner/tests/gvisor.rs`: `a_program_that_will_not_stop_is_killed_and_its_container_removed` proves runaway queries are killed on timeout. | • Enforced cgroup memory/process limits require runner to execute as root (warns when running unprivileged).<br>• Outbound query proxy (allowing query engine to reach only vended storage endpoints while blocking other egress) is planned. |
| **4. Server to Store** | Iceberg table Parquet files, metadata snapshots, bucket keys | Compromised storage client, path traversal across workspaces | • `server/lake/src/iceberg/rest.rs`: vended storage credentials scoped strictly to table subfolder prefix using AWS STS / session policies.<br>• `server/server/src/storage/service.rs`: enforces storage hierarchy; `bytes_live_under_the_workspaces_prefix` in `server/server/tests/files.rs` proves workspace isolation.<br>• `server/server/src/secrets/service.rs`: store access keys referenced only by vault URI (`loamz-data/bucket#key_id`), never persisted or logged. | • RustFS STS session policy revocation during active long-running queries is not instantaneous.<br>• S3 bucket versioning and object lock policies must be configured at the cloud provider level. |
| **5. Server to OpenBao** | Tenant database credentials, S3 root access keys, signing keys | Network sniffer, rogue local process, unauthorized service account | • `server/server/src/secrets/openbao.rs`: AppRole authentication with short-lived tokens; credentials cached only in server process memory.<br>• `server/server/tests/secrets.rs`: `a_secret_with_no_vault_is_an_error_that_names_the_reference_only` ensures secrets are never printed or committed.<br>• `server/server/tests/secrets.rs`: `a_short_serverless_token_is_refused_without_printing_it` verifies secret validation without leakage. | • Automated AppRole secret ID rotation is manual today.<br>• Single-node OpenBao instance in development; production requires 3-node Raft cluster with external unseal. |
| **6. Server to Keycloak** | User identity directory, organization roles, platform admin status | Token forgery, replay attack, claim spoofing | • `server/server/src/auth.rs`: verifies RS256 signatures against Keycloak JWKS endpoint; strictly verifies issuer matches configured OIDC issuer.<br>• `server/server/tests/token.rs`: `a_code_is_exchanged_as_our_client_and_nothing_else_is_added` proves auth code exchange cannot inject extra claims.<br>• `server/server/tests/token.rs`: `nothing_else_can_be_asked_for` asserts token scopes are strictly constrained. | • Multi-factor authentication (MFA) and external IdP federation are supported by Keycloak but not yet mandated by server policy.<br>• Keycloak database automated continuous backup is planned. |
| **7. Tool to Tool (Keycloak Exchange)** | Cross-product Data APIs invoked by AI Agents or automated tools | Rogue tool service, privilege escalation between apps | • `server/server/src/auth.rs`: RFC 8693 Token Exchange verifies incoming token audience is explicitly `loamz-data`.<br>• `server/server/src/auth.rs`: preserves original human caller identity (`subject`) when an agent acts on behalf of a person.<br>• `server/server/tests/admins_add_people.rs`: `an_agent_acting_for_a_person` test verifies agent cannot exceed delegated user rights. | • Fine-grained tool permission scoping (e.g. read-only catalog agent vs read-write ETL agent) is governed at workspace level, not per-tool token level.<br>• Automated revocation of active exchanged tokens upon upstream user logout is not yet instantaneous. |

---

## 3. Not Yet Built (Planned Mitigations)

From `docs/architecture/platform.md` (Section 2 & 6) and platform proposals:

1. **Continuous Backup & Recovery**:
   - Automated WAL archiving of PostgreSQL and Keycloak databases to object storage with automated weekly point-in-time restore testing (`loamz-deploy` backup role).
2. **High-Availability Vault Cluster**:
   - OpenBao 3-node Raft deployment with transit seal and separated unseal keys.
3. **Sandbox Egress Filtering**:
   - Outbound network proxy allowing sandboxed engines to contact only the Loamz API and authorized vended storage endpoints, blocking all other outbound egress.
4. **Workspace Quotas & Scheduling**:
   - Workspace-level limits on concurrent execution, memory-hours, and CPU allocations to prevent noisy neighbor denial of service.
5. **Instantaneous Grant Revocation**:
   - Push-based invalidation of active runs and vended STS credentials when a role grant is revoked during execution.
6. **Mutual TLS (mTLS)**:
   - Encrypted, certificate-authenticated communication across all internal platform services (Caddy $\rightarrow$ Loamz Data $\rightarrow$ PostgreSQL / OpenBao).
