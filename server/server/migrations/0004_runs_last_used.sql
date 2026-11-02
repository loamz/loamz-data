-- "When was each compute target last used" reads every finished run of an organization; with this index it reads only the newest per target.
CREATE INDEX runs_org_target_finished ON runs (org_id, target_id, finished_ms);
