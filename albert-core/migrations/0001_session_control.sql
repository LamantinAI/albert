CREATE TABLE albert_users (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) > 0)
);

CREATE TABLE albert_identity_bindings (
    connector_id TEXT NOT NULL CHECK (length(connector_id) > 0),
    external_actor_id TEXT NOT NULL CHECK (length(external_actor_id) > 0),
    user_id TEXT NOT NULL REFERENCES albert_users(id),
    PRIMARY KEY (connector_id, external_actor_id)
);

-- Private channel selectors only. A group's fixed session belongs to the
-- authoritative octo-history catalog and never appears in this table.
CREATE TABLE albert_private_session_routes (
    connector_id TEXT NOT NULL CHECK (length(connector_id) > 0),
    channel_id TEXT NOT NULL CHECK (length(channel_id) > 0),
    user_id TEXT NOT NULL REFERENCES albert_users(id),
    session_id TEXT NOT NULL CHECK (length(session_id) > 0),
    return_session_id TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    PRIMARY KEY (connector_id, channel_id, user_id)
);

-- No transcript, title, attachment or message receipt belongs here. These
-- markers survive loss of a temporary session's RAM journal after a restart.
CREATE TABLE albert_session_markers (
    session_id TEXT PRIMARY KEY NOT NULL CHECK (length(session_id) > 0),
    runtime_epoch TEXT NOT NULL CHECK (length(runtime_epoch) > 0),
    state TEXT NOT NULL CHECK (state IN ('temporary', 'saved', 'closed', 'expired')),
    revision INTEGER NOT NULL CHECK (revision > 0)
);
