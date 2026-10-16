-- Moderators, timed bans, the action log and settings changed at run time (stage 7, M5).

-- A ban with an end; NULL with `banned_at` set means for good.
ALTER TABLE users ADD COLUMN banned_until INTEGER;

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    actor_id TEXT,
    actor_name TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT,
    details TEXT
);
CREATE INDEX IF NOT EXISTS idx_audit_at ON audit_log(at);
