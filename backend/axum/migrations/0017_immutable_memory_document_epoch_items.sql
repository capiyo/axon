CREATE TABLE memory_document_epoch_items (
    epoch_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    document_version INTEGER NOT NULL CHECK (document_version > 0),
    content_hash TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN (
        'agent_private', 'task_shared', 'project_shared', 'workspace_shared'
    )),
    project_id TEXT,
    task_id TEXT,
    owner_id TEXT,
    source_event_sequence INTEGER NOT NULL CHECK (source_event_sequence >= 0),
    qkv_version TEXT NOT NULL,
    qoin_vector_version TEXT,
    qoin_scoring_version TEXT NOT NULL,
    snapshot_at TEXT NOT NULL,
    PRIMARY KEY (epoch_id, memory_id, document_version),
    FOREIGN KEY (epoch_id) REFERENCES memory_epochs(id) ON DELETE RESTRICT,
    FOREIGN KEY (memory_id, document_version)
        REFERENCES memory_document_versions(memory_id, version)
        ON DELETE RESTRICT
);

CREATE INDEX idx_memory_document_epoch_items_memory
    ON memory_document_epoch_items(workspace_id, memory_id, document_version);

CREATE TRIGGER memory_document_epoch_items_no_update
BEFORE UPDATE ON memory_document_epoch_items
BEGIN
    SELECT RAISE(ABORT, 'memory document epoch items are immutable');
END;

CREATE TRIGGER memory_document_epoch_items_no_delete
BEFORE DELETE ON memory_document_epoch_items
BEGIN
    SELECT RAISE(ABORT, 'memory document epoch items are immutable');
END;
