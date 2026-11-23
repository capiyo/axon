-- Progressive retrieval is append-only and idempotent per agent session.
CREATE TABLE IF NOT EXISTS agent_memory_session_retrievals (
    session_id TEXT NOT NULL REFERENCES agent_memory_sessions(session_id),
    memory_id TEXT NOT NULL,
    retrieved_at TEXT NOT NULL,
    PRIMARY KEY(session_id, memory_id)
);
CREATE INDEX IF NOT EXISTS idx_agent_memory_retrieval_session
    ON agent_memory_session_retrievals(session_id, retrieved_at);
