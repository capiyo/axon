ALTER TABLE memory_vector_index ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'memory_entry'
    CHECK (source_kind IN ('memory_entry', 'chat_message'));
ALTER TABLE memory_epoch_items ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'memory_entry'
    CHECK (source_kind IN ('memory_entry', 'chat_message'));
