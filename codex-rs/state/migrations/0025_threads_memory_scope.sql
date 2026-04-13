ALTER TABLE threads ADD COLUMN memory_scope_kind TEXT NOT NULL DEFAULT 'global';
ALTER TABLE threads ADD COLUMN memory_scope_key TEXT NOT NULL DEFAULT 'global';

CREATE INDEX idx_threads_memory_scope_updated_at
    ON threads(memory_scope_kind, memory_scope_key, updated_at DESC, id DESC);
