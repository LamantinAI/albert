-- A deletion tombstone outlives content. Filesystem cleanup is retried against
-- the original host workspace, including after a configuration change/restart.
CREATE TABLE albert_session_erasures (
    session_id TEXT PRIMARY KEY REFERENCES albert_session_markers(session_id),
    workspace_root TEXT NOT NULL
);
CREATE TABLE albert_session_erasure_files (
    session_id TEXT NOT NULL REFERENCES albert_session_erasures(session_id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL,
    workspace_root TEXT NOT NULL,
    PRIMARY KEY(session_id,asset_id,workspace_root)
);
-- Existing-session preparation can outlive a canceled HTTP request. Record its
-- file ownership before the blocking writer starts, without retaining a blob.
CREATE TABLE albert_session_workspace_copies (
    session_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    workspace_root TEXT NOT NULL,
    PRIMARY KEY(asset_id,workspace_root)
);
CREATE INDEX albert_workspace_copies_session ON albert_session_workspace_copies(session_id);
