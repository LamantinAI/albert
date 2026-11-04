CREATE TABLE albert_telegram_bots (
    connector_id TEXT PRIMARY KEY NOT NULL,
    bot_id TEXT NOT NULL,
    username TEXT NOT NULL
);

CREATE TABLE albert_telegram_login_tokens (
    id TEXT PRIMARY KEY NOT NULL,
    digest TEXT NOT NULL UNIQUE,
    account_id TEXT NOT NULL REFERENCES albert_login_accounts(id),
    connector_id TEXT NOT NULL,
    bot_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER,
    revoked_at INTEGER
);
CREATE INDEX albert_telegram_pending_tokens ON albert_telegram_login_tokens
    (account_id, connector_id, expires_at);

CREATE TABLE albert_telegram_bindings (
    connector_id TEXT NOT NULL,
    bot_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    account_id TEXT NOT NULL REFERENCES albert_login_accounts(id),
    chat_id TEXT NOT NULL,
    generation TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER,
    PRIMARY KEY (connector_id, bot_id, actor_id)
);
CREATE INDEX albert_telegram_account_bindings ON albert_telegram_bindings(account_id);

-- Historical attribution is not an authentication credential. A claim records
-- the one account that proved control of an otherwise unowned legacy profile.
CREATE TABLE albert_telegram_legacy_claims (
    legacy_user_id TEXT PRIMARY KEY NOT NULL REFERENCES albert_users(id),
    account_id TEXT NOT NULL REFERENCES albert_login_accounts(id),
    canonical_user_id TEXT NOT NULL REFERENCES albert_users(id)
);

-- Proof that a compact was computed from the public projection of this same
-- group journal. Untagged legacy compacts may contain hidden tool results.
CREATE TABLE albert_group_public_compacts (
    session_id TEXT NOT NULL,
    compact_id INTEGER NOT NULL,
    through_id INTEGER NOT NULL,
    content_digest TEXT NOT NULL,
    PRIMARY KEY (session_id, compact_id)
);

-- Native account provenance fences queued work after role/binding changes.
-- Legacy jobs have no account grant and never acquire one by identity lookup.
ALTER TABLE albert_messages ADD COLUMN account_authority TEXT;
