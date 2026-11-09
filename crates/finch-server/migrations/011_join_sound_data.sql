-- Keep new WAVs and their versions in the same row. Existing WAVs still live on disk.
ALTER TABLE users ADD COLUMN join_sound_data BLOB;
