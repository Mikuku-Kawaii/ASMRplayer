-- Download non-audio files without creating fake tracks or playable sources.
ALTER TABLE attachments ADD COLUMN byte_size INTEGER;
CREATE TABLE downloads_new (
    id TEXT PRIMARY KEY NOT NULL,
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    version_id TEXT REFERENCES track_versions(id) ON DELETE CASCADE,
    source_id TEXT REFERENCES media_sources(id) ON DELETE CASCADE,
    attachment_id TEXT REFERENCES attachments(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('queued','resolving','downloading','verifying','completed','paused','failed','canceled')),
    temp_relative_path TEXT NOT NULL,
    target_relative_path TEXT NOT NULL,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0 CHECK(downloaded_bytes >= 0),
    total_bytes INTEGER,
    etag TEXT,
    last_modified TEXT,
    content_sha256 TEXT,
    integrity_mode TEXT NOT NULL DEFAULT 'size_and_header',
    retry_count INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    error_message TEXT,
    final_renamed INTEGER NOT NULL DEFAULT 0 CHECK(final_renamed IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0 CHECK(priority BETWEEN -1 AND 1),
    next_retry_at TEXT,
    CHECK ((attachment_id IS NULL AND source_id IS NOT NULL AND version_id IS NOT NULL)
        OR (attachment_id IS NOT NULL AND source_id IS NULL AND version_id IS NULL))
);
INSERT INTO downloads_new (id,work_id,version_id,source_id,status,temp_relative_path,target_relative_path,
    downloaded_bytes,total_bytes,etag,last_modified,content_sha256,integrity_mode,retry_count,error_code,
    error_message,final_renamed,created_at,updated_at,priority,next_retry_at)
SELECT id,work_id,version_id,source_id,status,temp_relative_path,target_relative_path,
    downloaded_bytes,total_bytes,etag,last_modified,content_sha256,integrity_mode,retry_count,error_code,
    error_message,final_renamed,created_at,updated_at,priority,next_retry_at FROM downloads ORDER BY rowid;
DROP TABLE downloads;
ALTER TABLE downloads_new RENAME TO downloads;
CREATE INDEX idx_downloads_source_id ON downloads(source_id);
CREATE INDEX idx_downloads_attachment_id ON downloads(attachment_id);
CREATE INDEX idx_downloads_status ON downloads(status);
CREATE INDEX idx_downloads_schedule ON downloads(status,priority DESC,created_at);
