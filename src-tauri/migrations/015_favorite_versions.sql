CREATE TABLE favorite_versions (
    version_id TEXT PRIMARY KEY NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);
