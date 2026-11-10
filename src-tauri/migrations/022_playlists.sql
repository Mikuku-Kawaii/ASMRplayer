CREATE TABLE playlists (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    version_ids TEXT NOT NULL DEFAULT '[]',
    sort_order INTEGER NOT NULL DEFAULT 0
);
