CREATE TABLE IF NOT EXISTS subtitle_documents (
    version_id TEXT NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    source_key TEXT NOT NULL,
    document_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(version_id, source_key)
);
