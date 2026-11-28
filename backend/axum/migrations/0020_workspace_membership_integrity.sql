CREATE TRIGGER IF NOT EXISTS workspace_memberships_require_workspace
BEFORE INSERT ON workspace_memberships
WHEN NOT EXISTS (
    SELECT 1 FROM entities
    WHERE kind = 'workspace' AND id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'workspace membership requires an existing workspace');
END;
