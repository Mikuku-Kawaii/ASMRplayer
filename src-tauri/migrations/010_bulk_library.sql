CREATE TABLE IF NOT EXISTS work_listening_marks (
    work_id TEXT PRIMARY KEY NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    listened INTEGER NOT NULL CHECK(listened IN (0,1)),
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS library_batches (
    id TEXT PRIMARY KEY NOT NULL,
    execution_seq INTEGER,
    journal_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_library_batches_execution ON library_batches(execution_seq);
