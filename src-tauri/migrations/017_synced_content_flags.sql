-- Keep age/subtitle metadata even when a local work has no online source link.
ALTER TABLE works ADD COLUMN nsfw_detected INTEGER NOT NULL DEFAULT 0 CHECK (nsfw_detected IN (0, 1));
ALTER TABLE works ADD COLUMN has_subtitle_metadata INTEGER NOT NULL DEFAULT 0 CHECK (has_subtitle_metadata IN (0, 1));
