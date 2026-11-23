-- NULL identifies existing ordered track playlists, including empty ones.
ALTER TABLE playlists ADD COLUMN work_items TEXT;
