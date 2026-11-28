CREATE TABLE IF NOT EXISTS model_daily_spend (
    user_id TEXT NOT NULL,
    usage_date TEXT NOT NULL,
    spent_micros INTEGER NOT NULL DEFAULT 0 CHECK (spent_micros >= 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (user_id, usage_date)
);
