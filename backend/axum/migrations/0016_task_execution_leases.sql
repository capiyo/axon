CREATE TABLE task_execution_leases (
    task_id TEXT PRIMARY KEY NOT NULL,
    owner_token TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_task_execution_leases_expiry
    ON task_execution_leases (lease_expires_at);
