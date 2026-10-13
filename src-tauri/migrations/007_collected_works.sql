-- Migration 007: Dual-Dimension Collection ("收藏" -> 我的音频库) vs Likes ("喜欢" -> 喜欢列表)

CREATE TABLE IF NOT EXISTS collected_works (
    work_id TEXT PRIMARY KEY NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_collected_works_created_at
    ON collected_works(created_at DESC);

-- Backfill existing locally imported works so they remain in "我的音频库"
INSERT OR IGNORE INTO collected_works (work_id, created_at)
SELECT DISTINCT le.work_id, COALESCE(le.last_seen_at, '2026-01-01T00:00:00Z')
FROM library_entries le;
