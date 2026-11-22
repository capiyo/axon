CREATE TABLE IF NOT EXISTS model_providers (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    status TEXT NOT NULL,
    data_terms_json TEXT NOT NULL,
    terms_reviewed_at TEXT,
    reviewed_by TEXT,
    payload_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS models (
    id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL REFERENCES model_providers(id),
    model_name TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('candidate', 'approved', 'disabled')),
    categories_json TEXT NOT NULL,
    abilities_json TEXT NOT NULL,
    context_length INTEGER NOT NULL,
    input_cost_per_million INTEGER NOT NULL,
    output_cost_per_million INTEGER NOT NULL,
    version TEXT,
    flagged_for_review INTEGER NOT NULL DEFAULT 0,
    payload_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_models_provider_status ON models(provider_id, status);
CREATE INDEX IF NOT EXISTS idx_models_category_status ON models(status, model_name);

CREATE TABLE IF NOT EXISTS model_evaluations (
    id TEXT PRIMARY KEY NOT NULL,
    model_id TEXT NOT NULL REFERENCES models(id),
    category TEXT NOT NULL,
    test_set_version TEXT NOT NULL,
    score REAL NOT NULL,
    run_at TEXT NOT NULL,
    cost_micros INTEGER NOT NULL,
    payload_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_model_evaluations_model_category ON model_evaluations(model_id, category, run_at);

CREATE TABLE IF NOT EXISTS model_stats (
    id TEXT PRIMARY KEY NOT NULL,
    model_id TEXT NOT NULL REFERENCES models(id),
    category TEXT NOT NULL,
    role TEXT NOT NULL,
    runs INTEGER NOT NULL,
    completion_rate REAL NOT NULL,
    supervisor_pass_rate REAL NOT NULL,
    user_retry_rate REAL NOT NULL,
    cost_per_run_micros INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    last_updated TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE(model_id, category, role)
);

CREATE TABLE IF NOT EXISTS assignment_decisions (
    id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL,
    role TEXT NOT NULL,
    slot INTEGER NOT NULL,
    candidates_json TEXT NOT NULL,
    filters_json TEXT NOT NULL,
    chosen_model_id TEXT NOT NULL REFERENCES models(id),
    reason TEXT NOT NULL,
    estimated_cost_micros INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE(run_id, role, slot)
);
CREATE INDEX IF NOT EXISTS idx_assignment_decisions_run ON assignment_decisions(run_id, created_at);

CREATE TABLE IF NOT EXISTS platform_admins (
    user_id TEXT PRIMARY KEY NOT NULL,
    firebase_uid TEXT NOT NULL UNIQUE,
    granted_at TEXT NOT NULL,
    granted_by TEXT
);

CREATE TABLE IF NOT EXISTS model_change_flags (
    id TEXT PRIMARY KEY NOT NULL,
    model_id TEXT NOT NULL REFERENCES models(id),
    change_kind TEXT NOT NULL,
    previous_value TEXT,
    observed_value TEXT,
    detected_at TEXT NOT NULL,
    cleared_at TEXT,
    cleared_by TEXT,
    resolution_note TEXT,
    payload_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_model_change_flags_open ON model_change_flags(cleared_at, detected_at);
