-- A short custom sound played to a voice room when the person joins. The WAV lives on disk
-- (like avatars); this version is part of its ETag and changes with every new sound.
ALTER TABLE users ADD COLUMN join_sound_version INTEGER;
