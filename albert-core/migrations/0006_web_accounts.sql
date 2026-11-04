-- Browser accounts authenticate people; canonical users own conversation data.
-- A new owner deliberately has no canonical profile until one is chosen/proven.
CREATE TABLE albert_login_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL COLLATE NOCASE UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('owner','user')),
    canonical_user_id TEXT REFERENCES albert_users(id),
    must_change_password INTEGER NOT NULL DEFAULT 1 CHECK(must_change_password IN (0,1)),
    disabled INTEGER NOT NULL DEFAULT 0 CHECK(disabled IN (0,1)),
    token_version INTEGER NOT NULL DEFAULT 1 CHECK(token_version > 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE albert_browser_credentials (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES albert_login_accounts(id),
    token_version INTEGER NOT NULL,
    issued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER
);
CREATE INDEX albert_browser_credentials_account ON albert_browser_credentials(account_id);
CREATE TABLE albert_auth_bootstrap (
    id INTEGER PRIMARY KEY CHECK(id=1),
    seeded INTEGER NOT NULL DEFAULT 0 CHECK(seeded IN (0,1))
);
INSERT INTO albert_auth_bootstrap (id,seeded) VALUES (1,0);
