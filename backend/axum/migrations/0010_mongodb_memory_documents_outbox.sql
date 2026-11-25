-- SQLite remains authoritative for document identity, version, scope and sync state.
-- The outbox payload is transient and must be cleared after successful MongoDB persistence.
CREATE TABLE memory_document_versions (
    memory_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    workspace_id TEXT NOT NULL,
    project_id TEXT,
    owner_id TEXT,
    task_id TEXT,
    scope TEXT NOT NULL CHECK (scope IN (
        'agent_private', 'task_shared', 'project_shared', 'workspace_shared'
    )),
    document_type TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    source_event_sequence INTEGER NOT NULL CHECK (source_event_sequence >= 0),
    qkv_version TEXT NOT NULL,
    qoin_vector_version TEXT,
    lifecycle_status TEXT NOT NULL CHECK (
        lifecycle_status IN ('proposed', 'approved', 'rejected', 'superseded', 'deleted', 'archived')
    ),
    sync_status TEXT NOT NULL CHECK (
        sync_status IN ('pending', 'stored', 'indexed', 'failed')
    ) DEFAULT 'pending',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (memory_id, version)
);

CREATE INDEX idx_memory_document_versions_workspace_scope
    ON memory_document_versions(workspace_id, scope, lifecycle_status, sync_status);

CREATE INDEX idx_memory_document_versions_project
    ON memory_document_versions(workspace_id, project_id, memory_id, version);

CREATE TABLE memory_document_outbox (
    event_id TEXT PRIMARY KEY,
    memory_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    event_type TEXT NOT NULL CHECK (event_type IN ('memory_document.put')),
    payload_json TEXT,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'processing', 'completed', 'dead_letter')
    ) DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at TEXT NOT NULL,
    lease_owner TEXT,
    lease_until TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (memory_id, version)
        REFERENCES memory_document_versions(memory_id, version)
        ON DELETE RESTRICT
);

CREATE INDEX idx_memory_document_outbox_ready
    ON memory_document_outbox(status, available_at, created_at);

CREATE INDEX idx_memory_document_outbox_lease
    ON memory_document_outbox(status, lease_until);
