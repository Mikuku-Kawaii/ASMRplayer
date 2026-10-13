-- 005_pro_audio_dsp.sql (M8: T801-T805)
-- Pro Audio DSP/EQ state, AutoEQ / custom presets, and per-work DSP overrides.

CREATE TABLE IF NOT EXISTS dsp_global_state (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    active_preset_id TEXT,
    config_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS dsp_presets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    config_json TEXT NOT NULL,
    is_builtin INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS work_dsp_overrides (
    work_id TEXT PRIMARY KEY NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    preset_id TEXT,
    config_json TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);
