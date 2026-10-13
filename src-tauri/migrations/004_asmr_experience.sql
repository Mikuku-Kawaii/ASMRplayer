-- 004_asmr_experience.sql (M6: T601-T604)
-- Bookmarks, A-B loops, and sleep timer persistence.

CREATE TABLE IF NOT EXISTS bookmarks (
    id TEXT PRIMARY KEY NOT NULL,
    version_id TEXT NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    position_ms INTEGER NOT NULL CHECK (position_ms >= 0),
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bookmarks_version_pos ON bookmarks(version_id, position_ms ASC);

CREATE TABLE IF NOT EXISTS ab_loops (
    id TEXT PRIMARY KEY NOT NULL,
    version_id TEXT NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    CHECK (0 <= start_ms AND start_ms < end_ms)
);

CREATE INDEX IF NOT EXISTS idx_ab_loops_version ON ab_loops(version_id);

CREATE TABLE IF NOT EXISTS sleep_timer_state (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    mode TEXT NOT NULL DEFAULT 'off' CHECK (mode IN ('off', 'minutes', 'end_of_track')),
    deadline_utc TEXT,
    target_generation INTEGER,
    fired_at TEXT,
    updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO sleep_timer_state (id, mode, deadline_utc, target_generation, fired_at, updated_at)
VALUES (1, 'off', NULL, NULL, NULL, '2026-01-01T00:00:00Z');
