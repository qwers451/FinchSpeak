-- Sender-chosen message id: lets a client match its pending message with the stored one and
-- retry a send after a reconnect without creating a duplicate.
ALTER TABLE messages ADD COLUMN client_msg_id TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_author_client_msg_id
    ON messages(author_id, client_msg_id) WHERE client_msg_id IS NOT NULL;
