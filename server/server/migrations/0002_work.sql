-- What people save in a workspace to come back to. A saved query is private to the person who wrote it (a workspace admin sees all of
-- them); it holds SQL text, never data.
CREATE TABLE saved_queries (
    id            TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL,
    workspace_id  TEXT NOT NULL,
    owner         TEXT NOT NULL,
    name          TEXT NOT NULL,
    sql           TEXT NOT NULL,
    target_id     TEXT NOT NULL,
    created_ms    BIGINT NOT NULL,
    updated_ms    BIGINT NOT NULL
);
CREATE INDEX saved_queries_workspace ON saved_queries (workspace_id, updated_ms);
