-- One migration set for SQLite and Postgres: TEXT ids, BIGINT numbers (epoch milliseconds), no database-only features.
-- Every column is NOT NULL; "no parent" is the empty string.
--
-- People, groups and organizations are Keycloak's. A principal is named as `user:<sub>`, `group:<path>` or `sp:<sub>`.

-- An organization's Data account. `id` is Keycloak's organization id, or `personal:<sub>` for a person's own space.
CREATE TABLE orgs (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    alias      TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_ms BIGINT NOT NULL
);

-- Who manages the account: workspaces, who is assigned, catalogs and everything in them (the metastore admin too).
CREATE TABLE account_admins (
    org_id     TEXT NOT NULL,
    principal  TEXT NOT NULL,
    added_by   TEXT NOT NULL,
    created_ms BIGINT NOT NULL,
    PRIMARY KEY (org_id, principal)
);

CREATE TABLE workspaces (
    id         TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_ms BIGINT NOT NULL,
    UNIQUE (org_id, name)
);

-- Who is assigned to a workspace, and as what (`admin` or `user`).
CREATE TABLE workspace_members (
    workspace_id TEXT NOT NULL,
    principal    TEXT NOT NULL,
    role         TEXT NOT NULL,
    created_ms   BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, principal)
);

-- What a member may do in the workspace at all (`sql-access`, `compute-create`, `sandbox-access`).
CREATE TABLE entitlements (
    workspace_id TEXT NOT NULL,
    principal    TEXT NOT NULL,
    entitlement  TEXT NOT NULL,
    PRIMARY KEY (workspace_id, principal, entitlement)
);

-- A catalog with any binding is visible only in the workspaces bound to it; one with none is visible in all.
CREATE TABLE catalog_bindings (
    workspace_id TEXT NOT NULL,
    catalog_id   TEXT NOT NULL,
    PRIMARY KEY (workspace_id, catalog_id)
);
CREATE INDEX catalog_bindings_catalog ON catalog_bindings (catalog_id);

-- Catalogs, schemas, tables, volumes and views are all rows here, one level pointing at the one above.
CREATE TABLE securables (
    id             TEXT PRIMARY KEY,
    org_id         TEXT NOT NULL,
    kind           TEXT NOT NULL,
    parent_id      TEXT NOT NULL,
    name           TEXT NOT NULL,
    owner          TEXT NOT NULL,
    classification TEXT NOT NULL,
    placement      TEXT NOT NULL,
    consistency    TEXT NOT NULL,
    state          TEXT NOT NULL,
    created_ms     BIGINT NOT NULL,
    UNIQUE (org_id, parent_id, name)
);
CREATE INDEX securables_parent ON securables (org_id, parent_id);
CREATE INDEX securables_owner ON securables (org_id, owner);

-- One privilege on one securable (or on the metastore, `metastore:<org>`) for one principal.
CREATE TABLE grants (
    org_id     TEXT NOT NULL,
    principal  TEXT NOT NULL,
    securable  TEXT NOT NULL,
    privilege  TEXT NOT NULL,
    granted_by TEXT NOT NULL,
    created_ms BIGINT NOT NULL,
    PRIMARY KEY (org_id, principal, securable, privilege)
);
CREATE INDEX grants_securable ON grants (org_id, securable);

-- Append-only, hash-chained, one chain per organization. It holds who did what to which thing, never the content, and it
-- is not touched by an erase.
CREATE TABLE audit (
    org_id       TEXT NOT NULL,
    seq          BIGINT NOT NULL,
    at_ms        BIGINT NOT NULL,
    actor        TEXT NOT NULL,
    on_behalf_of TEXT NOT NULL,
    action       TEXT NOT NULL,
    target       TEXT NOT NULL,
    outcome      TEXT NOT NULL,
    prev_hash    TEXT NOT NULL,
    hash         TEXT NOT NULL,
    PRIMARY KEY (org_id, seq)
);

-- Work that must finish even if the server stops half way: a worker leases a task, retries it, and marks it done.
CREATE TABLE tasks (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL,
    kind         TEXT NOT NULL,
    subject      TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    on_behalf_of TEXT NOT NULL,
    state        TEXT NOT NULL,
    attempts     BIGINT NOT NULL,
    lease_until  BIGINT NOT NULL,
    created_ms   BIGINT NOT NULL,
    last_error   TEXT NOT NULL
);
CREATE INDEX tasks_due ON tasks (state, lease_until);

-- Compute an organization has attached. A target is one compute agent (a machine running `loamz-runner agent`) that connects out to
-- this server and asks for work; it never accepts connections. Its secret is shown once and kept only as a hash.
CREATE TABLE compute_targets (
    id               TEXT PRIMARY KEY,
    org_id           TEXT NOT NULL,
    name             TEXT NOT NULL,
    secret_hash      TEXT NOT NULL,
    max_memory_mb    BIGINT NOT NULL,
    max_cpu_millis   BIGINT NOT NULL,
    created_by       TEXT NOT NULL,
    created_ms       BIGINT NOT NULL,
    last_seen_ms     BIGINT NOT NULL,
    UNIQUE (org_id, name)
);
CREATE UNIQUE INDEX compute_targets_secret ON compute_targets (secret_hash);

-- One piece of work: a SQL statement or a job. `spec` says what to run and with what; `result` is a statement's rows (capped).
CREATE TABLE runs (
    id            TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL,
    workspace_id  TEXT NOT NULL,
    kind          TEXT NOT NULL,
    requested_by  TEXT NOT NULL,
    on_behalf_of  TEXT NOT NULL,
    target_id     TEXT NOT NULL,
    state         TEXT NOT NULL,
    title         TEXT NOT NULL,
    spec          TEXT NOT NULL,
    result        TEXT NOT NULL,
    stdout        TEXT NOT NULL,
    stderr        TEXT NOT NULL,
    exit_code     BIGINT NOT NULL,
    error         TEXT NOT NULL,
    cancel        BIGINT NOT NULL,
    lease_until   BIGINT NOT NULL,
    created_ms    BIGINT NOT NULL,
    started_ms    BIGINT NOT NULL,
    finished_ms   BIGINT NOT NULL
);
CREATE INDEX runs_workspace ON runs (workspace_id, created_ms);
CREATE INDEX runs_due ON runs (state, target_id, created_ms);

-- The token a running job carries instead of anyone's own: the person it runs for, narrowed to the securables it declared. It
-- stops working when the run ends or expires, and only a hash of it is kept.
CREATE TABLE run_tokens (
    token_hash    TEXT PRIMARY KEY,
    run_id        TEXT NOT NULL,
    org_id        TEXT NOT NULL,
    workspace_id  TEXT NOT NULL,
    identity      TEXT NOT NULL,
    reads         TEXT NOT NULL,
    writes        TEXT NOT NULL,
    expires_ms    BIGINT NOT NULL
);
CREATE INDEX run_tokens_run ON run_tokens (run_id);
