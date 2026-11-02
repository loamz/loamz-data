-- Admission checks active work per organization, independent of target or workspace.
CREATE INDEX runs_org_state ON runs (org_id, state);
