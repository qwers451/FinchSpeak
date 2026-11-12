CREATE TABLE music_tracks (
    id TEXT PRIMARY KEY,
    uploader_id TEXT NOT NULL,
    title TEXT NOT NULL,
    artist TEXT,
    duration_ms INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    format TEXT NOT NULL,
    path TEXT NOT NULL,
    temporary INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    queued_in INTEGER,
    deleting INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX music_library ON music_tracks(temporary, created_at);
CREATE INDEX music_temporary_owner ON music_tracks(uploader_id, temporary);
