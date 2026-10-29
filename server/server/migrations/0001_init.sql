-- One migration set for SQLite and Postgres: TEXT ids, BIGINT numbers (epoch milliseconds), no database-only features.
-- Every column is NOT NULL; "no parent" is the empty string.

-- Catalogs, schemas, tables, volumes and views are all rows here, one level pointing at the one above.
CREATE TABLE securables (
    id             TEXT PRIMARY KEY,
    tenant         TEXT NOT NULL,
    kind           TEXT NOT NULL,
    parent_id      TEXT NOT NULL,
    name           TEXT NOT NULL,
    owner          TEXT NOT NULL,
    classification TEXT NOT NULL,
    placement      TEXT NOT NULL,
    consistency    TEXT NOT NULL,
    state          TEXT NOT NULL,
    created_ms     BIGINT NOT NULL,
    UNIQUE (tenant, parent_id, name)
);
CREATE INDEX securables_parent ON securables (tenant, parent_id);
CREATE INDEX securables_owner ON securables (tenant, owner);

-- A grant is one privilege on one securable for one principal (a person, an agent or a service).
CREATE TABLE grants (
    tenant     TEXT NOT NULL,
    principal  TEXT NOT NULL,
    securable  TEXT NOT NULL,
    privilege  TEXT NOT NULL,
    granted_by TEXT NOT NULL,
    created_ms BIGINT NOT NULL,
    PRIMARY KEY (tenant, principal, securable, privilege)
);
CREATE INDEX grants_securable ON grants (tenant, securable);

-- Append-only, hash-chained. It holds who did what to which thing, never the content, and it is not touched by an erase.
CREATE TABLE audit (
    seq          BIGINT PRIMARY KEY,
    at_ms        BIGINT NOT NULL,
    tenant       TEXT NOT NULL,
    actor        TEXT NOT NULL,
    on_behalf_of TEXT NOT NULL,
    action       TEXT NOT NULL,
    target       TEXT NOT NULL,
    outcome      TEXT NOT NULL,
    prev_hash    TEXT NOT NULL,
    hash         TEXT NOT NULL
);

-- Work that must finish even if the server stops half way: a worker leases a task, retries it, and marks it done.
CREATE TABLE tasks (
    id          TEXT PRIMARY KEY,
    tenant      TEXT NOT NULL,
    kind        TEXT NOT NULL,
    subject     TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    on_behalf_of TEXT NOT NULL,
    state       TEXT NOT NULL,
    attempts    BIGINT NOT NULL,
    lease_until BIGINT NOT NULL,
    created_ms  BIGINT NOT NULL,
    last_error  TEXT NOT NULL
);
CREATE INDEX tasks_due ON tasks (state, lease_until);
