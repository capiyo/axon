CREATE TABLE memory_retrieval_feedback (
    feedback_id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    document_version INTEGER NOT NULL CHECK (document_version > 0),
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    event_type TEXT NOT NULL CHECK (event_type IN (
        'retrieved', 'selected', 'ignored', 'reused',
        'helpful', 'irrelevant', 'contradicted', 'incorrect'
    )),
    created_at TEXT NOT NULL,
    evidence_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_memory_feedback_document
    ON memory_retrieval_feedback(workspace_id, memory_id, document_version, event_type, created_at);

CREATE INDEX idx_memory_feedback_session
    ON memory_retrieval_feedback(session_id, agent_id, created_at);
