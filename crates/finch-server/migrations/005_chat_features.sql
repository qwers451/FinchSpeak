-- Replies, mentions, server-side read state and image thumbnails (stage 7, M2).

-- The message a reply answers. No foreign key on purpose: when the original is deleted the
-- reply keeps the id, and the client shows "original message deleted".
ALTER TABLE messages ADD COLUMN reply_ref TEXT;

CREATE TABLE IF NOT EXISTS message_mentions (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (message_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_mentions_user ON message_mentions(user_id);

-- How far each user has read each channel. No row = nothing before the account existed.
CREATE TABLE IF NOT EXISTS read_state (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    last_read_id TEXT NOT NULL,
    last_read_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, channel_id)
);

-- Existing members start with everything read, so an upgrade does not light up every channel.
INSERT OR IGNORE INTO read_state (user_id, channel_id, last_read_id, last_read_at)
SELECT u.id, m.channel_id, m.id, m.created_at
FROM users u
JOIN messages m ON m.id = (
    SELECT m2.id FROM messages m2 WHERE m2.channel_id = m.channel_id ORDER BY m2.created_at DESC, m2.id DESC LIMIT 1
)
GROUP BY u.id, m.channel_id;

ALTER TABLE attachments ADD COLUMN width INTEGER;
ALTER TABLE attachments ADD COLUMN height INTEGER;
ALTER TABLE attachments ADD COLUMN thumb_path TEXT;
