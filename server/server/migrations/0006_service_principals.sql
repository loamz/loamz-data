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
