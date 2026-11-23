-- Each epoch owns a frozen copy of the vector index. Later re-indexing cannot
-- alter what an already-running agent could have seen.
CREATE TABLE IF NOT EXISTS memory_epoch_items (
    epoch_id TEXT NOT NULL REFERENCES memory_epochs(id),
    workspace_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    task_id TEXT,
    project_id TEXT,
    owner_agent_id TEXT,
    visibility TEXT NOT NULL,
    qoin_vector_json TEXT NOT NULL,
    value_payload_json TEXT NOT NULL,
    tags_json TEXT NOT NULL,
    dependency_ids_json TEXT NOT NULL,
    outcome_score REAL NOT NULL,
    evidence_score REAL NOT NULL,
    source_event_sequence INTEGER NOT NULL,
    model_version TEXT NOT NULL,
    PRIMARY KEY(epoch_id, memory_id)
);
CREATE INDEX IF NOT EXISTS idx_memory_epoch_items_scope
    ON memory_epoch_items(epoch_id, workspace_id, visibility, task_id, project_id);
ALTER TABLE agent_memory_sessions ADD COLUMN project_id TEXT;
