CREATE TABLE agent_credentials (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    secret_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    revoked_at TEXT,
    last_used_at TEXT,
    UNIQUE (workspace_id, agent_id, id)
);

CREATE INDEX idx_agent_credentials_active
    ON agent_credentials(workspace_id, agent_id, expires_at)
    WHERE revoked_at IS NULL;

CREATE INDEX idx_agent_credentials_owner
    ON agent_credentials(owner_user_id, workspace_id, agent_id);
