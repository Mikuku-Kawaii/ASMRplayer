ALTER TABLE downloads ADD COLUMN priority INTEGER NOT NULL DEFAULT 0 CHECK(priority BETWEEN -1 AND 1);
ALTER TABLE downloads ADD COLUMN next_retry_at TEXT;
CREATE INDEX IF NOT EXISTS idx_downloads_schedule ON downloads(status, priority DESC, created_at);
