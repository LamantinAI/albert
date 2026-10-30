-- Albert routing/control metadata only. Payload and provenance live in the
-- canonical octo-history session journal; record IDs below are typed public IDs.
CREATE TABLE albert_session_execution (
    session_id TEXT PRIMARY KEY NOT NULL,
    location TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0)
);
CREATE TABLE albert_work_reports (
    session_id TEXT NOT NULL,
    work_ref TEXT NOT NULL,
    policy TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    PRIMARY KEY (session_id, work_ref)
);
CREATE TABLE albert_messages (
    message_id TEXT PRIMARY KEY NOT NULL,
    actor_id TEXT NOT NULL,
    source_session TEXT NOT NULL,
    source_record INTEGER NOT NULL,
    envelope TEXT NOT NULL,
    digest TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    deadline_unix_ms INTEGER NOT NULL
);
CREATE TABLE albert_message_inbox (
    message_id TEXT NOT NULL REFERENCES albert_messages(message_id),
    recipient_session TEXT NOT NULL,
    recipient_record INTEGER,
    work_state TEXT NOT NULL CHECK (work_state IN ('passive','queued','running','completed','failed','cancelled','unknown')),
    result_record INTEGER,
    failure_code TEXT,
    PRIMARY KEY (message_id, recipient_session)
);
CREATE TABLE albert_message_outbox (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES albert_messages(message_id),
    resolved TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending','started','delivered','retry','unknown','failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    failure_code TEXT,
    attempt_token TEXT
);
CREATE INDEX albert_message_inbox_queue ON albert_message_inbox(recipient_session, work_state);
CREATE INDEX albert_message_outbox_queue ON albert_message_outbox(state);
