CREATE UNIQUE INDEX idx_memory_document_outbox_version
    ON memory_document_outbox(memory_id, version, event_type);
