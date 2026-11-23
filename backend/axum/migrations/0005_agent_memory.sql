-- Scoped, provenance-carrying agent memory. Payloads are immutable versions;
-- current rows are updated transactionally while the audit log is append-only.
CREATE TABLE IF NOT EXISTS agent_memories (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('workspace','task','agent','execution')),
    scope_id TEXT,
    owner_agent_id TEXT,
    sensitivity TEXT NOT NULL CHECK (sensitivity IN ('public','internal','confidential','secret')),
    verification TEXT NOT NULL CHECK (verification IN ('unverified','proposed','verified','rejected')),
    content TEXT NOT NULL,
    source TEXT NOT NULL,
    confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    authority REAL NOT NULL CHECK (authority >= 0 AND authority <= 1),
    created_at TEXT NOT NULL,
    expires_at TEXT,
    version INTEGER NOT NULL DEFAULT 1,
    token_cost INTEGER NOT NULL CHECK (token_cost > 0),
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_memories_scope
    ON agent_memories(workspace_id, scope, scope_id, verification, expires_at);
CREATE TABLE IF NOT EXISTS agent_memory_grants (
    memory_id TEXT NOT NULL REFERENCES agent_memories(id),
    agent_id TEXT NOT NULL,
    granted_by TEXT NOT NULL,
    granted_at TEXT NOT NULL,
    revoked_at TEXT,
    PRIMARY KEY(memory_id, agent_id)
);
CREATE INDEX IF NOT EXISTS idx_agent_memory_grants_agent
    ON agent_memory_grants(agent_id, revoked_at);
CREATE TABLE IF NOT EXISTS agent_memory_versions (
    memory_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    actor TEXT NOT NULL,
    event_type TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    PRIMARY KEY(memory_id, version)
);
CREATE TABLE IF NOT EXISTS agent_memory_allocations (
    id TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL,
    task_id TEXT,
    agent_id TEXT NOT NULL,
    token_budget INTEGER NOT NULL,
    tokens_allocated INTEGER NOT NULL,
    selected_json TEXT NOT NULL,
    excluded_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_memory_allocations_workspace
    ON agent_memory_allocations(workspace_id, created_at);
