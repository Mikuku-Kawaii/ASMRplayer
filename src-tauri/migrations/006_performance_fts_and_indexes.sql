-- Migration 006: M9 Performance Indexes & SQLite FTS5 Full-Text Search (T901, T902)

-- T901: Composite & Covering Indexes to eliminate full-table scans in list_works and joins
CREATE INDEX IF NOT EXISTS idx_works_updated_at
  ON works(updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_work_playback_last_played
  ON work_playback_state(last_played_at DESC);

CREATE INDEX IF NOT EXISTS idx_work_overrides_work
  ON work_overrides(work_id);

CREATE INDEX IF NOT EXISTS idx_work_sources_work_refreshed
  ON work_sources(work_id, refreshed_at DESC);

CREATE INDEX IF NOT EXISTS idx_tracks_work_id
  ON tracks(work_id);

CREATE INDEX IF NOT EXISTS idx_track_versions_track_id
  ON track_versions(track_id);

CREATE INDEX IF NOT EXISTS idx_media_sources_version_kind_avail
  ON media_sources(version_id, source_kind, availability);

-- T902: SQLite FTS5 Virtual Tables for works and tracks/subtitles
CREATE VIRTUAL TABLE IF NOT EXISTS works_fts USING fts5(
    work_id UNINDEXED,
    rj_code,
    title,
    description,
    circle_name,
    voice_actors,
    tags,
    tokenize = 'unicode61'
);

CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
    track_id UNINDEXED,
    work_id UNINDEXED,
    title,
    logical_dir_path,
    subtitle_text,
    tokenize = 'unicode61'
);

-- Initial backfill for existing databases upgrading to v0.5
DELETE FROM works_fts;
INSERT INTO works_fts (work_id, rj_code, title, description, circle_name, voice_actors, tags)
SELECT id, COALESCE(rj_code, ''), title, description, circle_name, voice_actors_json, tags_json
FROM works;

DELETE FROM tracks_fts;
INSERT INTO tracks_fts (track_id, work_id, title, logical_dir_path, subtitle_text)
SELECT id, work_id, title, logical_dir_path, ''
FROM tracks;

-- Automatic synchronization triggers for works -> works_fts
CREATE TRIGGER IF NOT EXISTS trg_works_fts_insert
AFTER INSERT ON works
BEGIN
    INSERT INTO works_fts (work_id, rj_code, title, description, circle_name, voice_actors, tags)
    VALUES (NEW.id, COALESCE(NEW.rj_code, ''), NEW.title, NEW.description, NEW.circle_name, NEW.voice_actors_json, NEW.tags_json);
END;

CREATE TRIGGER IF NOT EXISTS trg_works_fts_update
AFTER UPDATE ON works
BEGIN
    DELETE FROM works_fts WHERE work_id = OLD.id;
    INSERT INTO works_fts (work_id, rj_code, title, description, circle_name, voice_actors, tags)
    VALUES (NEW.id, COALESCE(NEW.rj_code, ''), NEW.title, NEW.description, NEW.circle_name, NEW.voice_actors_json, NEW.tags_json);
END;

CREATE TRIGGER IF NOT EXISTS trg_works_fts_delete
AFTER DELETE ON works
BEGIN
    DELETE FROM works_fts WHERE work_id = OLD.id;
END;

-- Automatic synchronization triggers for tracks -> tracks_fts
CREATE TRIGGER IF NOT EXISTS trg_tracks_fts_insert
AFTER INSERT ON tracks
BEGIN
    INSERT INTO tracks_fts (track_id, work_id, title, logical_dir_path, subtitle_text)
    VALUES (NEW.id, NEW.work_id, NEW.title, NEW.logical_dir_path, '');
END;

CREATE TRIGGER IF NOT EXISTS trg_tracks_fts_update
AFTER UPDATE ON tracks
BEGIN
    DELETE FROM tracks_fts WHERE track_id = OLD.id;
    INSERT INTO tracks_fts (track_id, work_id, title, logical_dir_path, subtitle_text)
    VALUES (NEW.id, NEW.work_id, NEW.title, NEW.logical_dir_path, '');
END;

CREATE TRIGGER IF NOT EXISTS trg_tracks_fts_delete
AFTER DELETE ON tracks
BEGIN
    DELETE FROM tracks_fts WHERE track_id = OLD.id;
END;
