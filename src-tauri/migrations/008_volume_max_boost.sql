-- 008_volume_max_boost.sql
-- Raise player_session.volume CHECK constraint ceiling from 100.0 to 300.0 (supporting 200% volume-max boost).

CREATE TABLE IF NOT EXISTS player_session_v8 (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    current_queue_item_id TEXT REFERENCES queue_items(id) ON DELETE SET NULL,
    volume REAL NOT NULL DEFAULT 100.0 CHECK (volume >= 0.0 AND volume <= 300.0),
    loop_mode TEXT NOT NULL DEFAULT 'none' CHECK (loop_mode IN ('none', 'single', 'queue')),
    session_generation INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);

INSERT OR REPLACE INTO player_session_v8 (id, current_queue_item_id, volume, loop_mode, session_generation, updated_at)
SELECT id, current_queue_item_id, volume, loop_mode, session_generation, updated_at
FROM player_session;

DROP TABLE player_session;

ALTER TABLE player_session_v8 RENAME TO player_session;

INSERT OR IGNORE INTO player_session (id, current_queue_item_id, volume, loop_mode, session_generation, updated_at)
VALUES (1, NULL, 100.0, 'none', 1, '2026-01-01T00:00:00Z');
