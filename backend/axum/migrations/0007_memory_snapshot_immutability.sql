-- Published snapshots and pinned agent context are immutable. New progressive
-- retrieval is recorded as separate append-only rows.
CREATE TRIGGER IF NOT EXISTS memory_epoch_items_reject_update
BEFORE UPDATE ON memory_epoch_items
BEGIN
    SELECT RAISE(ABORT, 'memory_epoch_items are immutable');
END;
CREATE TRIGGER IF NOT EXISTS memory_epoch_items_reject_delete
BEFORE DELETE ON memory_epoch_items
BEGIN
    SELECT RAISE(ABORT, 'memory_epoch_items are immutable');
END;
CREATE TRIGGER IF NOT EXISTS agent_memory_sessions_reject_update
BEFORE UPDATE ON agent_memory_sessions
BEGIN
    SELECT RAISE(ABORT, 'agent memory sessions are immutable');
END;
CREATE TRIGGER IF NOT EXISTS agent_memory_sessions_reject_delete
BEFORE DELETE ON agent_memory_sessions
BEGIN
    SELECT RAISE(ABORT, 'agent memory sessions are immutable');
END;
CREATE TRIGGER IF NOT EXISTS agent_memory_session_retrievals_reject_update
BEFORE UPDATE ON agent_memory_session_retrievals
BEGIN
    SELECT RAISE(ABORT, 'agent memory retrievals are append-only');
END;
CREATE TRIGGER IF NOT EXISTS agent_memory_session_retrievals_reject_delete
BEFORE DELETE ON agent_memory_session_retrievals
BEGIN
    SELECT RAISE(ABORT, 'agent memory retrievals are append-only');
END;
