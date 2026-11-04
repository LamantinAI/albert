-- Accepted first work is journaled before it becomes a published conversation.
CREATE TABLE albert_web_starts (
    owner_user_id TEXT NOT NULL REFERENCES albert_users(id),
    request_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    host_tools INTEGER NOT NULL CHECK(host_tools IN (0,1)),
    session_id TEXT NOT NULL UNIQUE,
    digest TEXT NOT NULL,
    temporary INTEGER NOT NULL CHECK(temporary IN (0,1)),
    phase TEXT NOT NULL CHECK(phase IN ('accepted','claimed','running','complete','failed','expired')),
    runtime_epoch TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT(datetime('now')),
    PRIMARY KEY(owner_user_id,request_id)
);
CREATE INDEX albert_web_starts_phase ON albert_web_starts(owner_user_id,phase);
