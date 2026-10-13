-- 003_downloads.sql (M5: T501-T508)
-- Offline download tasks, Range validators, and crash recovery state.

CREATE TABLE IF NOT EXISTS downloads (
    id TEXT PRIMARY KEY NOT NULL,
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    version_id TEXT NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL REFERENCES media_sources(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('queued', 'resolving', 'downloading', 'verifying', 'completed', 'paused', 'failed', 'canceled')),
    temp_relative_path TEXT NOT NULL,
    target_relative_path TEXT NOT NULL,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0 CHECK (downloaded_bytes >= 0),
    total_bytes INTEGER,
    etag TEXT,
    last_modified TEXT,
    content_sha256 TEXT,
    integrity_mode TEXT NOT NULL DEFAULT 'size_and_header',
    retry_count INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    error_message TEXT,
    final_renamed INTEGER NOT NULL DEFAULT 0 CHECK (final_renamed IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_downloads_source_id ON downloads(source_id);
CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
