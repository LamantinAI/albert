CREATE TABLE albert_media_assets_next (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size INTEGER NOT NULL CHECK(size > 0),
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64)
);
INSERT INTO albert_media_assets_next(id, session_id, filename, content_type, size, sha256)
SELECT id, session_id, filename, content_type, size, sha256 FROM albert_media_assets;
DROP TABLE albert_media_assets;
ALTER TABLE albert_media_assets_next RENAME TO albert_media_assets;
CREATE INDEX albert_media_session ON albert_media_assets(session_id);
