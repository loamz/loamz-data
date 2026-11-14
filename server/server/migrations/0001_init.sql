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

-- Catalogs, schemas, tables, volumes and views are all rows here, one level pointing at the one above.
CREATE TABLE securables (
    id             TEXT PRIMARY KEY,
    org_id         TEXT NOT NULL,
    -- the workspace its catalog belongs to: a catalog has exactly one, and everything inside it carries the same
    workspace_id   TEXT NOT NULL,
    -- where its bytes are: a table or volume, its own folder in the store; a catalog, the root its objects' folders go under
    -- (`<workspace id>/<catalog id>`); a schema, its catalog's root; a view has none
    folder         TEXT NOT NULL,
    kind           TEXT NOT NULL,
    parent_id      TEXT NOT NULL,
    name           TEXT NOT NULL,
    description    TEXT,
    description_folded TEXT NOT NULL DEFAULT '',
    tags           TEXT NOT NULL DEFAULT '[]',
    owner          TEXT NOT NULL,
    owner_folded   TEXT NOT NULL DEFAULT '',
    classification TEXT NOT NULL,
    placement      TEXT NOT NULL,
    consistency    TEXT NOT NULL,
    state          TEXT NOT NULL,
    created_ms     BIGINT NOT NULL,
    -- a table's current Iceberg metadata file (a path inside the table's own folder); empty for everything else
    metadata       TEXT NOT NULL,
    -- a name is unique among the things beside it in a workspace: two workspaces can each have a catalog called `sales`
    UNIQUE (org_id, workspace_id, parent_id, name)
);
CREATE INDEX securables_parent ON securables (org_id, parent_id);
CREATE INDEX securables_owner ON securables (org_id, owner);
CREATE INDEX securables_workspace ON securables (org_id, workspace_id);
CREATE INDEX securables_search_order ON securables (org_id, workspace_id, name, id);

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

-- A readable, eraseable name for each identity ref used in an audit chain.
CREATE TABLE audit_names (
    org_id TEXT NOT NULL,
    ref    TEXT NOT NULL,
    name   TEXT NOT NULL,
    PRIMARY KEY (org_id, ref),
    UNIQUE (org_id, name)
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
    last_error   TEXT NOT NULL,
    alert_ref    TEXT NOT NULL DEFAULT ''
);
CREATE INDEX tasks_due ON tasks (state, lease_until);

-- A cap on the runs a workspace may have queued or running at once, below its organization's. No row: no cap of its own.
CREATE TABLE workspace_quotas (
    workspace_id    TEXT PRIMARY KEY,
    max_active_runs BIGINT NOT NULL
);

-- Which compute may read a catalog. A catalog with none listed can be read from any compute of its workspace.
CREATE TABLE catalog_compute (
    catalog_id TEXT NOT NULL,
    target_id  TEXT NOT NULL,
    PRIMARY KEY (catalog_id, target_id)
);

-- Compute a workspace has attached. A target is one compute agent (a machine running `loamz-runner agent`) that connects out to
-- this server and asks for work; it never accepts connections. Its secret is shown once and kept only as a hash.
CREATE TABLE compute_targets (
    id               TEXT PRIMARY KEY,
    org_id           TEXT NOT NULL,
    workspace_id     TEXT NOT NULL,
    name             TEXT NOT NULL,
    secret_hash      TEXT NOT NULL,
    -- 'agent' (a machine that connects out) or 'serverless' (a provider endpoint the server calls, started on demand)
    kind             TEXT NOT NULL,
    endpoint         TEXT NOT NULL,
    max_memory_mb    BIGINT NOT NULL,
    max_cpu_millis   BIGINT NOT NULL,
    created_by       TEXT NOT NULL,
    created_ms       BIGINT NOT NULL,
    last_seen_ms     BIGINT NOT NULL,
    UNIQUE (workspace_id, name)
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
    finished_ms   BIGINT NOT NULL,
    schedule_task_id TEXT UNIQUE,
    -- a statement's whole result kept as an Arrow file in the data's own storage: its size and the folder it is in (0 and empty: none)
    result_bytes  BIGINT NOT NULL DEFAULT 0,
    result_folder TEXT NOT NULL DEFAULT '',
    -- a key a person may send with a request so that sending it again returns this run instead of making another
    idempotency_key TEXT NOT NULL DEFAULT '',
    -- a job that commits files to a person's volume holds its thread (`<volume id>:<thread>`) while it is queued or running
    thread_key TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX runs_thread_active ON runs (workspace_id, thread_key) WHERE thread_key <> '' AND state IN ('queued', 'running');
-- A thread whose files are being deleted: no new result or output may be written to it until `until_ms`, when the sweep deletes its files
-- once more (a machine that was running when it was deleted may still hold a credential for a few minutes) and removes this row.
CREATE TABLE thread_deletions (
    workspace_id TEXT NOT NULL,
    org_id       TEXT NOT NULL,
    volume_id    TEXT NOT NULL,
    thread       TEXT NOT NULL,
    until_ms     BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, volume_id, thread)
);
CREATE UNIQUE INDEX runs_idempotency ON runs (workspace_id, requested_by, idempotency_key) WHERE idempotency_key <> '';
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

-- A service principal is an identity of one workspace that is not a person: a BI tool, an ML app, a pipeline. It signs in with a
-- token we issue (shown once; only its hash is kept), can be given grants and entitlements as `sp:<id>`, and can open only its
-- own workspace.
CREATE TABLE service_principals (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    name         TEXT NOT NULL,
    secret_hash  TEXT NOT NULL,
    created_by   TEXT NOT NULL,
    created_ms   BIGINT NOT NULL,
    UNIQUE (workspace_id, name)
);
CREATE UNIQUE INDEX service_principals_secret ON service_principals (secret_hash);

-- Storage settings of a workspace: how the server reaches a store (a credential) and which part of it a workspace may use (a
-- location). A credential holds references into the vault, never a secret. Locations never overlap: no two share a store and
-- bucket with one prefix inside the other.
CREATE TABLE storage_credentials (
    id               TEXT PRIMARY KEY,
    org_id           TEXT NOT NULL,
    workspace_id     TEXT NOT NULL,
    name             TEXT NOT NULL,
    endpoint         TEXT NOT NULL,
    region           TEXT NOT NULL,
    provider         TEXT NOT NULL,
    secret_path      TEXT NOT NULL,
    secret_version   BIGINT NOT NULL,
    generation       BIGINT NOT NULL,
    created_by       TEXT NOT NULL,
    created_ms       BIGINT NOT NULL,
    UNIQUE (workspace_id, name)
);

CREATE TABLE storage_locations (
    id            TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL,
    workspace_id  TEXT NOT NULL,
    name          TEXT NOT NULL,
    credential_id TEXT NOT NULL,
    bucket        TEXT NOT NULL,
    -- no leading or trailing slash; empty is the whole bucket
    prefix        TEXT NOT NULL,
    created_by    TEXT NOT NULL,
    created_ms    BIGINT NOT NULL,
    UNIQUE (workspace_id, name)
);
CREATE INDEX storage_locations_credential ON storage_locations (credential_id);

-- Rules for a table: the rows a person may see and the columns shown masked. `masks` is a JSON object of column to expression. A
-- person who manages the table sees it whole; everyone else reads it only through SQL on Loamz compute, where the rules are applied.
CREATE TABLE table_policies (
    table_id   TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL,
    row_filter TEXT NOT NULL,
    masks      TEXT NOT NULL,
    updated_by TEXT NOT NULL,
    updated_ms BIGINT NOT NULL
);

-- A saved SQL statement or job, evaluated with the creator's current rights on each firing.
CREATE TABLE schedules (
    id                 TEXT PRIMARY KEY,
    org_id             TEXT NOT NULL,
    workspace_id       TEXT NOT NULL,
    name               TEXT NOT NULL,
    owner              TEXT NOT NULL,
    cadence_kind       TEXT NOT NULL,
    cadence            TEXT NOT NULL,
    definition         TEXT NOT NULL,
    webhook_ref        TEXT NOT NULL,
    state              TEXT NOT NULL,
    next_run_ms        BIGINT NOT NULL,
    next_alert_ms      BIGINT NOT NULL,
    alert_lease_until  BIGINT NOT NULL,
    last_error         TEXT NOT NULL,
    created_ms         BIGINT NOT NULL
);
CREATE UNIQUE INDEX schedules_active_name ON schedules (workspace_id, name) WHERE state <> 'deleted';
CREATE INDEX schedules_due ON schedules (state, next_run_ms);

-- One task fire belongs to one run. The unique link makes retries after a crash idempotent.
CREATE TABLE schedule_alerts (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL,
    workspace_id    TEXT NOT NULL,
    schedule_id     TEXT NOT NULL,
    run_id          TEXT NOT NULL UNIQUE,
    destination_ref TEXT NOT NULL,
    state           TEXT NOT NULL,
    attempts        BIGINT NOT NULL,
    next_try_ms     BIGINT NOT NULL,
    created_ms      BIGINT NOT NULL,
    sent_ms         BIGINT NOT NULL,
    last_error      TEXT NOT NULL
);
CREATE INDEX schedule_alerts_due ON schedule_alerts (state, next_try_ms);
