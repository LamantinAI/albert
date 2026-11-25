-- Retain only the delivery identity and digest. A deleted fork cannot be revived by replay.
CREATE TABLE albert_fork_receipts (
    namespace TEXT NOT NULL,
    request_key TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    PRIMARY KEY(namespace, request_key)
);

CREATE TABLE albert_fork_name_sequences (
    session_id TEXT PRIMARY KEY,
    last_number INTEGER NOT NULL CHECK(last_number > 0)
);
