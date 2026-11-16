CREATE TABLE albert_usage_tracking (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO albert_usage_tracking (id) VALUES (1);
CREATE TABLE albert_model_usage (
    invocation_id TEXT PRIMARY KEY,
    recorded_at TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    origin TEXT NOT NULL,
    input_tokens INTEGER,
    output_tokens INTEGER,
    cached_input_tokens INTEGER,
    total_tokens INTEGER,
    CHECK (input_tokens IS NULL OR input_tokens >= 0),
    CHECK (output_tokens IS NULL OR output_tokens >= 0)
);
CREATE INDEX albert_model_usage_period ON albert_model_usage(recorded_at, model_id);
