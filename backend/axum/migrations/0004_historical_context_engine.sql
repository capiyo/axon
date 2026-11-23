-- Global memory epochs are immutable publication points. Agent sessions pin an epoch
-- and therefore never observe background consolidation halfway through a task.
CREATE TABLE IF NOT EXISTS memory_epochs (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    epoch_number INTEGER NOT NULL,
    published_at TEXT NOT NULL,
    source_event_sequence INTEGER NOT NULL,
    indexed_memory_count INTEGER NOT NULL,
    snapshot_digest TEXT NOT NULL,
    UNIQUE(workspace_id, epoch_number)
);
CREATE INDEX IF NOT EXISTS idx_memory_epochs_workspace
    ON memory_epochs(workspace_id, epoch_number DESC);

-- Qoin supplies semantic vectors; Axon stores the vector alongside the
-- immutable source pointer and authorization metadata. No generated vectors.
CREATE TABLE IF NOT EXISTS memory_vector_index (
    memory_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    task_id TEXT,
    project_id TEXT,
    owner_agent_id TEXT,
    visibility TEXT NOT NULL CHECK (visibility IN ('agent_private', 'task_shared', 'project_shared', 'workspace_shared')),
    status TEXT NOT NULL CHECK (status IN ('approved', 'superseded', 'deleted')),
    qoin_vector_json TEXT NOT NULL,
    key_metadata_json TEXT NOT NULL,
    value_payload_json TEXT NOT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    dependency_ids_json TEXT NOT NULL DEFAULT '[]',
    outcome_score REAL NOT NULL DEFAULT 0.5 CHECK (outcome_score >= 0 AND outcome_score <= 1),
    evidence_score REAL NOT NULL DEFAULT 0.5 CHECK (evidence_score >= 0 AND evidence_score <= 1),
    source_event_sequence INTEGER NOT NULL,
    model_version TEXT NOT NULL,
    indexed_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memory_vector_workspace_status
    ON memory_vector_index(workspace_id, status, source_event_sequence);
CREATE INDEX IF NOT EXISTS idx_memory_vector_task
    ON memory_vector_index(workspace_id, task_id, status);
CREATE INDEX IF NOT EXISTS idx_memory_vector_project
    ON memory_vector_index(workspace_id, project_id, status);

-- A session is immutable with respect to its memory epoch. Progressive
-- retrieval adds task-local references without rewriting the initial context.
CREATE TABLE IF NOT EXISTS agent_memory_sessions (
    session_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    task_id TEXT,
    memory_epoch_id TEXT NOT NULL REFERENCES memory_epochs(id),
    initial_memory_ids_json TEXT NOT NULL,
    retrieved_memory_ids_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_memory_sessions_agent
    ON agent_memory_sessions(workspace_id, agent_id, created_at DESC);

CREATE TRIGGER IF NOT EXISTS memory_epochs_reject_update
BEFORE UPDATE ON memory_epochs
BEGIN
    SELECT RAISE(ABORT, 'memory_epochs are immutable');
END;
CREATE TRIGGER IF NOT EXISTS memory_epochs_reject_delete
BEFORE DELETE ON memory_epochs
BEGIN
    SELECT RAISE(ABORT, 'memory_epochs are immutable');
END;
