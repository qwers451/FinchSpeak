-- Avatars, chosen presence status with text, and when someone was last online (stage 7, M4).

-- Bumped whenever the picture changes: part of its address, so caches never show a stale one.
ALTER TABLE users ADD COLUMN avatar_version INTEGER;

-- What the person chose to show while connected: online, idle or dnd. (`status` above is unused.)
ALTER TABLE users ADD COLUMN status_pref TEXT NOT NULL DEFAULT 'online';
ALTER TABLE users ADD COLUMN status_text TEXT;

ALTER TABLE users ADD COLUMN last_seen INTEGER;
