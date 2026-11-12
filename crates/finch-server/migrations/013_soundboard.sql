CREATE TABLE soundboard_sounds (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    emoji TEXT,
    uploader_id TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    data BLOB NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX soundboard_order ON soundboard_sounds(created_at);
