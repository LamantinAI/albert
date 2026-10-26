ALTER TABLE albert_session_markers
    ADD COLUMN owner_user_id TEXT REFERENCES albert_users(id);

-- Transport navigation metadata, not message acceptance receipts. It contains
-- no transcript, title, payload, content hash or per-message history.
CREATE TABLE albert_temporary_excursions (
    connector_id TEXT NOT NULL,
    bot_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES albert_users(id),
    session_id TEXT NOT NULL,
    return_session_id TEXT NOT NULL,
    scope_epoch TEXT NOT NULL,
    start_date INTEGER NOT NULL CHECK (start_date > 0),
    start_update_id INTEGER NOT NULL CHECK (start_update_id >= 0),
    start_message_id INTEGER NOT NULL CHECK (start_message_id > 0),
    end_date INTEGER,
    end_update_id INTEGER,
    end_message_id INTEGER,
    disposition TEXT NOT NULL CHECK (disposition IN ('active', 'promoted', 'discarded')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    PRIMARY KEY (session_id, connector_id, bot_id, chat_id, actor_id),
    UNIQUE (connector_id, bot_id, chat_id, actor_id, start_date, start_update_id, start_message_id),
    CHECK ((end_date IS NULL AND end_update_id IS NULL AND end_message_id IS NULL)
        OR (end_date IS NOT NULL AND end_update_id IS NOT NULL AND end_message_id IS NOT NULL))
);
CREATE INDEX albert_excursion_actor ON albert_temporary_excursions
    (connector_id, bot_id, chat_id, actor_id, start_date);
