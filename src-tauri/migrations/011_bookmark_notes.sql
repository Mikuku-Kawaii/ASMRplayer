ALTER TABLE bookmarks ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';
-- Unresolved backups retain the exact version identity without inventing a media version.
CREATE TABLE IF NOT EXISTS unlinked_bookmarks (
    id TEXT PRIMARY KEY NOT NULL,
    record_json TEXT NOT NULL
);
