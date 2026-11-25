ALTER TABLE albert_media_assets ADD COLUMN sha256 TEXT
    CHECK(sha256 IS NULL OR length(sha256) = 64);
