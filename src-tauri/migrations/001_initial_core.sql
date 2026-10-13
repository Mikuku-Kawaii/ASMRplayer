-- 001_initial_core.sql (M1: T104, T105, T107)
-- Core schema for works, sources, overrides, tracks, versions, media_sources,
-- library_roots, library_entries, attachments, playback_positions, work_playback_state,
-- queue_items, player_session, and app_settings.

CREATE TABLE IF NOT EXISTS works (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    rj_code TEXT,
    circle_name TEXT NOT NULL DEFAULT '',
    voice_actors_json TEXT NOT NULL DEFAULT '[]',
    tags_json TEXT NOT NULL DEFAULT '[]',
    cover_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_works_rj_code ON works(rj_code);
CREATE INDEX IF NOT EXISTS idx_works_updated_at ON works(updated_at DESC);

CREATE TABLE IF NOT EXISTS work_sources (
    id TEXT PRIMARY KEY NOT NULL,
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    source_work_id TEXT NOT NULL,
    raw_metadata_json TEXT NOT NULL DEFAULT '{}',
    cached_tree_json TEXT,
    sync_status TEXT NOT NULL DEFAULT 'ok' CHECK (sync_status IN ('ok', 'pending_confirm', 'source_changed', 'error')),
    refreshed_at TEXT NOT NULL,
    expires_at TEXT,
    UNIQUE(provider, source_work_id)
);

CREATE INDEX IF NOT EXISTS idx_work_sources_work_id ON work_sources(work_id);

CREATE TABLE IF NOT EXISTS work_overrides (
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    field TEXT NOT NULL,
    override_mode TEXT NOT NULL CHECK (override_mode IN ('set', 'cleared')),
    value_text TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (work_id, field)
);

CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY NOT NULL,
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    logical_dir_path TEXT NOT NULL DEFAULT '',
    group_label TEXT NOT NULL DEFAULT '',
    sort_key TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tracks_work_id_sort ON tracks(work_id, sort_key);

CREATE TABLE IF NOT EXISTS track_versions (
    id TEXT PRIMARY KEY NOT NULL,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    version_label TEXT NOT NULL DEFAULT 'default',
    duration_ms INTEGER,
    content_fingerprint TEXT,
    association_state TEXT NOT NULL DEFAULT 'confirmed' CHECK (association_state IN ('confirmed', 'pending_confirm', 'conflict')),
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_track_versions_track_id ON track_versions(track_id);
CREATE INDEX IF NOT EXISTS idx_track_versions_fingerprint ON track_versions(content_fingerprint);

CREATE TABLE IF NOT EXISTS library_roots (
    id TEXT PRIMARY KEY NOT NULL,
    absolute_path TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    root_mode TEXT NOT NULL DEFAULT 'library_root' CHECK (root_mode IN ('single_work', 'library_root')),
    availability TEXT NOT NULL DEFAULT 'available' CHECK (availability IN ('available', 'offline', 'permission_denied')),
    last_scanned_at TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS library_entries (
    root_id TEXT NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
    relative_work_path TEXT NOT NULL,
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    dir_mtime_ms INTEGER,
    last_seen_at TEXT NOT NULL,
    PRIMARY KEY (root_id, relative_work_path)
);

CREATE INDEX IF NOT EXISTS idx_library_entries_work_id ON library_entries(work_id);

CREATE TABLE IF NOT EXISTS media_sources (
    id TEXT PRIMARY KEY NOT NULL,
    version_id TEXT NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('local_imported', 'local_downloaded', 'online_provider')),
    provider TEXT,
    source_work_id TEXT,
    remote_file_id TEXT,
    remote_relative_path TEXT,
    root_id TEXT REFERENCES library_roots(id) ON DELETE SET NULL,
    relative_path TEXT,
    absolute_path TEXT,
    format TEXT NOT NULL,
    byte_size INTEGER,
    file_mtime_ms INTEGER,
    content_sha256 TEXT,
    availability TEXT NOT NULL DEFAULT 'available' CHECK (availability IN ('available', 'missing', 'offline_disk', 'pending_confirm', 'invalid')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(root_id, relative_path),
    UNIQUE(provider, source_work_id, remote_file_id)
);

CREATE INDEX IF NOT EXISTS idx_media_sources_version_avail ON media_sources(version_id, availability);

CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY NOT NULL,
    work_id TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    version_id TEXT REFERENCES track_versions(id) ON DELETE SET NULL,
    attachment_type TEXT NOT NULL CHECK (attachment_type IN ('cover', 'subtitle', 'text')),
    title TEXT NOT NULL,
    root_id TEXT REFERENCES library_roots(id) ON DELETE SET NULL,
    relative_path TEXT,
    absolute_path TEXT,
    provider TEXT,
    source_work_id TEXT,
    remote_file_id TEXT,
    subtitle_offset_ms INTEGER NOT NULL DEFAULT 0,
    encoding TEXT NOT NULL DEFAULT 'UTF-8',
    availability TEXT NOT NULL DEFAULT 'available' CHECK (availability IN ('available', 'missing', 'offline_disk', 'invalid')),
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_attachments_work_id ON attachments(work_id);
CREATE INDEX IF NOT EXISTS idx_attachments_version_id ON attachments(version_id);

CREATE TABLE IF NOT EXISTS playback_positions (
    version_id TEXT PRIMARY KEY NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    position_ms INTEGER NOT NULL DEFAULT 0 CHECK (position_ms >= 0),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS work_playback_state (
    work_id TEXT PRIMARY KEY NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    last_version_id TEXT NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    last_played_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_work_playback_state_recent ON work_playback_state(last_played_at DESC);

CREATE TABLE IF NOT EXISTS queue_items (
    id TEXT PRIMARY KEY NOT NULL,
    sort_order INTEGER NOT NULL,
    version_id TEXT NOT NULL REFERENCES track_versions(id) ON DELETE CASCADE,
    preferred_source_id TEXT REFERENCES media_sources(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_queue_items_order ON queue_items(sort_order ASC);

CREATE TABLE IF NOT EXISTS player_session (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    current_queue_item_id TEXT REFERENCES queue_items(id) ON DELETE SET NULL,
    volume REAL NOT NULL DEFAULT 100.0 CHECK (volume >= 0.0 AND volume <= 300.0),
    loop_mode TEXT NOT NULL DEFAULT 'none' CHECK (loop_mode IN ('none', 'single', 'queue')),
    session_generation INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO player_session (id, current_queue_item_id, volume, loop_mode, session_generation, updated_at)
VALUES (1, NULL, 100.0, 'none', 1, '2026-01-01T00:00:00Z');

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
