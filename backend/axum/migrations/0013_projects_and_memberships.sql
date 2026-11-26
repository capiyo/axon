CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    created_by_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);

CREATE INDEX idx_projects_workspace_created
    ON projects(workspace_id, created_at, id);

CREATE TABLE project_memberships (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    created_at TEXT NOT NULL,
    revoked_at TEXT,
    PRIMARY KEY (project_id, user_id)
);

CREATE INDEX idx_project_memberships_user_active
    ON project_memberships(user_id, project_id, role)
    WHERE revoked_at IS NULL;

CREATE INDEX idx_project_memberships_project_active
    ON project_memberships(project_id, role, user_id)
    WHERE revoked_at IS NULL;

CREATE TRIGGER projects_workspace_membership_guard
BEFORE INSERT ON projects
WHEN NOT EXISTS (
    SELECT 1 FROM workspace_memberships
    WHERE workspace_id = NEW.workspace_id
      AND user_id = NEW.created_by_user_id
      AND role = 'owner'
)
BEGIN
    SELECT RAISE(ABORT, 'project creator must own the workspace');
END;

CREATE TRIGGER project_memberships_workspace_guard
BEFORE INSERT ON project_memberships
WHEN NOT EXISTS (
    SELECT 1
    FROM projects p
    JOIN workspace_memberships w ON w.workspace_id = p.workspace_id
    WHERE p.id = NEW.project_id
      AND w.user_id = NEW.user_id
)
BEGIN
    SELECT RAISE(ABORT, 'project member must belong to its workspace');
END;
