pub mod download;
pub mod library;
pub mod playback;
pub mod engine_maintenance;
mod source_audio_info;
pub mod session;
pub mod diagnostics;
pub mod backup;
pub mod storage;
mod storage_file;
pub mod updater;
pub mod island;
pub mod instance;

#[cfg(windows)]
mod island_glass;
#[cfg(windows)]
mod island_window;

pub mod release_history;

pub mod desktop_lyrics;
pub mod interface_scale;
pub mod theme;
pub mod window_placement;
pub mod recognition;
pub mod ollama_install;
pub mod speech_engine;
pub mod sensevoice_gpu;
pub mod whisper_engine;
pub mod speech_segments;
pub mod asr_quality;
pub mod dialogue_review;
pub mod subtitle_tasks;
pub mod subtitle_documents;
pub mod script_alignment;
pub mod speech_sounds;
pub mod faster_whisper;
pub mod subtitle_sync;
pub mod subtitle_preferences;
pub mod device_sync;

pub mod immersion;
