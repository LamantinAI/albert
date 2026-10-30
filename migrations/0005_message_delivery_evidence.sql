ALTER TABLE albert_message_outbox ADD COLUMN transport_kind TEXT;
ALTER TABLE albert_message_outbox ADD COLUMN submitted_at INTEGER;
ALTER TABLE albert_message_outbox ADD COLUMN observation_deadline INTEGER;
ALTER TABLE albert_message_outbox ADD COLUMN evidence_truncated INTEGER NOT NULL DEFAULT 0;
CREATE TABLE albert_message_delivery_evidence (
    message_id TEXT NOT NULL REFERENCES albert_messages(message_id),
    attempt_token TEXT NOT NULL,
    observation_hash TEXT NOT NULL,
    outcome TEXT NOT NULL,
    native_ids TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(message_id,attempt_token,observation_hash)
);
CREATE INDEX albert_delivery_deadlines ON albert_message_outbox(state,observation_deadline);
ALTER TABLE albert_message_inbox ADD COLUMN report_outcomes TEXT;
CREATE TABLE albert_message_report_policies (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES albert_messages(message_id),
    policy TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0)
);
ALTER TABLE albert_message_inbox ADD COLUMN result_fingerprint TEXT;
