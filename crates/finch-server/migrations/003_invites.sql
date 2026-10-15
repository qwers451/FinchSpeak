-- Invitation codes for registration (AUD-027).
CREATE TABLE IF NOT EXISTS invites (
    code TEXT PRIMARY KEY,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    max_uses INTEGER NOT NULL,
    uses INTEGER NOT NULL DEFAULT 0
);
