-- 002_favorites_and_links.sql (M4: T404-T407)
-- Favorites and undoable work/version association records.

CREATE TABLE IF NOT EXISTS favorites (
    work_id TEXT PRIMARY KEY NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_favorites_created_at ON favorites(created_at DESC);

CREATE TABLE IF NOT EXISTS association_history (
    id TEXT PRIMARY KEY NOT NULL,
    entity_type TEXT NOT NULL CHECK (entity_type IN ('work', 'version')),
    primary_id TEXT NOT NULL,
    secondary_id TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    conflict_note TEXT,
    undone INTEGER NOT NULL DEFAULT 0 CHECK (undone IN (0, 1)),
    created_at TEXT NOT NULL
);
