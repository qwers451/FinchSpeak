-- Banned accounts cannot log in; existing sessions are deleted when the ban is set.
ALTER TABLE users ADD COLUMN banned_at INTEGER;
