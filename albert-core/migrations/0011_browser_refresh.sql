CREATE TABLE albert_browser_refresh (
    credential_id TEXT PRIMARY KEY NOT NULL REFERENCES albert_browser_credentials(id),
    token_digest TEXT NOT NULL UNIQUE,
    csrf TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL CHECK(expires_at > created_at)
);
CREATE INDEX albert_browser_refresh_expiry ON albert_browser_refresh(expires_at);
