CREATE TABLE albert_control_applications (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES albert_login_accounts(id),
    client_request_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    target TEXT NOT NULL,
    document_id TEXT,
    revision TEXT NOT NULL,
    state TEXT NOT NULL,
    message TEXT NOT NULL,
    runtime_epoch TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(account_id, client_request_id)
);
