-- A catalog belongs to exactly one workspace and is never shared (it replaces the bindings). Everything inside a catalog carries
-- the same workspace, so a check needs no walk up the tree.
--
-- An existing catalog goes to the first workspace it was bound to (a catalog bound to several, or to none, goes to the first
-- workspace of its organization): the one-workspace rule is applied to what was there, not guessed away.
ALTER TABLE securables ADD COLUMN workspace_id TEXT NOT NULL DEFAULT '';

UPDATE securables SET workspace_id = COALESCE(
    (SELECT MIN(b.workspace_id) FROM catalog_bindings b WHERE b.catalog_id = securables.id),
    (SELECT MIN(w.id) FROM workspaces w WHERE w.org_id = securables.org_id),
    ''
) WHERE kind = 'catalog';

UPDATE securables SET workspace_id = COALESCE(
    (SELECT c.workspace_id FROM securables c WHERE c.org_id = securables.org_id AND c.id = securables.parent_id), ''
) WHERE kind = 'schema';

UPDATE securables SET workspace_id = COALESCE(
    (SELECT s.workspace_id FROM securables s WHERE s.org_id = securables.org_id AND s.id = securables.parent_id), ''
) WHERE kind IN ('table', 'volume', 'view');

CREATE INDEX securables_workspace ON securables (org_id, workspace_id);

DROP TABLE catalog_bindings;
