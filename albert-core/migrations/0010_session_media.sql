CREATE TABLE albert_media_assets (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size INTEGER NOT NULL CHECK(size > 0),
    bytes BLOB NOT NULL,
    CHECK(length(bytes) = size)
);
CREATE INDEX albert_media_session ON albert_media_assets(session_id);
