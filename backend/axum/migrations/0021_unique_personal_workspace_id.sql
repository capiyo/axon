CREATE UNIQUE INDEX IF NOT EXISTS idx_users_personal_workspace_id_unique
    ON users (personal_workspace_id);
