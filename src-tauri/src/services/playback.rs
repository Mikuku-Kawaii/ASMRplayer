#[path = "playback/immersive.rs"]
mod immersive;
use crate::db::Database;
use crate::domain::*;
use crate::player::{AmllBroadcastHub, MpvController, MpvEvent, NativeSmtcController};
use crate::providers::{AsmrOneProvider, MediaPurpose, ProviderMediaRef};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, watch};
use super::engine_maintenance::{EngineMaintenanceSettings, MaintenanceClock};

#[derive(Debug, Clone, Default)]
pub struct CachedAudioTagMeta {
    pub version_id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub embedded_cover_bytes: Option<Vec<u8>>,
}

// Only the last session's observed stream facts; never load media to restore them.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedAudioInfo {
    version_id: String,
    source_id: String,
    format: String,
    byte_size: Option<i64>,
    telemetry: AudioStreamTelemetryDto,
}

pub struct PlaybackStateInner {
    maintenance: MaintenanceClock,
    pub session_generation: u64,
    pub state: String,
    pub user_paused_intent: bool,
    pub current_queue_item_id: Option<String>,
    pub current_work_id: Option<String>,
    pub current_work_title: Option<String>,
    pub current_track_id: Option<String>,
    pub current_track_title: Option<String>,
    pub current_version_id: Option<String>,
    pub active_source: Option<MediaSourceDto>,
    pub alternative_sources: Vec<MediaSourceDto>,
    pub position_ms: i64,
    pub current_completed: bool,
    pub duration_ms: Option<i64>,
    pub pending_seek_ms: Option<i64>,
    pub file_loaded_for_gen: bool,
    pub retried_online_for_gen: bool,
    pub mpv_buffering: bool,
    pub buffering_since: Option<Instant>,
    pub online_recovery_running: bool,
    pub auto_healed_for_gen: bool,
    pub volume: f64,
    immersion: super::immersion::ImmersionSettings,
    transition_serial: u64,
    fade_in_pending: bool,
    pub loop_mode: String,
    pub active_ab_loop: Option<AbLoopDto>,
    pub sleep_timer: SleepTimerDto,
    pub dsp_config: DspConfigDto,
    pub active_dsp_preset_id: Option<String>,
    pub audio_exclusive: bool,
    pub smtc_mode: String,
    pub audio_telemetry: AudioStreamTelemetryDto,
    pub last_error: Option<AppError>,
    pub last_persisted_at: Instant,
    pub last_smtc_timeline_sync_at: Instant,
    /// T906: Pre-resolved next queue item `(version_id, source_id, target_uri, headers)`
    pub prefetched_next: Option<(String, String, String, Vec<(String, String)>)>,
    pub prefetching_version_id: Option<String>,
    pub prefetched_at: Option<Instant>,
    last_prefetch_check: Option<Instant>,
    pub cached_audio_tags: Option<CachedAudioTagMeta>,
    pub active_subtitle_cues: Vec<SubtitleCueDto>,
}

impl PlaybackStateInner {
    fn schedule_prefetch(&mut self, now: Instant, position_ms: i64) -> bool {
        if self.state != "playing" || self.prefetching_version_id.is_some()
            || !self.duration_ms.is_some_and(|dur| dur > 0 && position_ms > 0 && dur - position_ms <= 15_000)
            || self.last_prefetch_check.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(5)) {
            return false;
        }
        self.last_prefetch_check = Some(now);
        true
    }
}

#[derive(Clone)]
pub struct PlaybackService {
    db: Database,
    provider: AsmrOneProvider,
    mpv: MpvController,
    inner: Arc<Mutex<PlaybackStateInner>>,
    event_seq: Arc<AtomicU64>,
    app_handle: Arc<Mutex<Option<AppHandle>>>,
    native_smtc: Arc<Mutex<Option<NativeSmtcController>>>,
    audio_output_operation: Arc<tokio::sync::Mutex<()>>,
    load_operation: Arc<tokio::sync::Mutex<()>>,
    recovery_operation: Arc<tokio::sync::Mutex<()>>,
    prefetch_operation: Arc<tokio::sync::Mutex<()>>,
    shutdown_signal: watch::Sender<bool>,
}

impl PlaybackService {
    /// Lock order is playback -> download worker registry -> DB. A paused or
    /// resolving session also owns media, not just an audibly playing session.
    pub(crate) fn with_storage_guard<T>(&self, f: impl FnOnce(bool, &std::collections::HashSet<String>) -> T) -> T {
        let state = self.inner.lock();
        let mut versions = std::collections::HashSet::new();
        versions.extend(state.current_version_id.iter().cloned());
        versions.extend(state.prefetching_version_id.iter().cloned());
        if let Some((version, _, _, _)) = &state.prefetched_next { versions.insert(version.clone()); }
        f(!matches!(state.state.as_str(), "idle" | "stopped") || state.active_source.is_some(), &versions)
    }

    pub fn new(db: Database, provider: AsmrOneProvider) -> Self {
        let _ = AmllBroadcastHub::global();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mpv = MpvController::new(event_tx);
        if let Some(proxy) = provider.proxy_url() {
            mpv.set_proxy(Some(proxy));
        }
        let hw_accel = db
            .get_setting("hardware_acceleration")
            .ok()
            .flatten()
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        mpv.set_hardware_acceleration(hw_accel);

        let audio_exclusive = db
            .get_setting("audio_exclusive")
            .ok()
            .flatten()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        mpv.set_audio_exclusive_flag(audio_exclusive);
        let audio_device = db.get_setting("audio_device").ok().flatten()
            .filter(|id| id == "auto" || id.starts_with("wasapi/"))
            .unwrap_or_else(|| "auto".into());
        mpv.set_initial_audio_device(audio_device);

        let smtc_mode = db
            .get_setting("smtc_mode")
            .ok()
            .flatten()
            .filter(|v| matches!(v.as_str(), "full" | "privacy" | "disabled"))
            .unwrap_or_else(|| "full".into());

        let (saved_qid, saved_vol, saved_loop, saved_gen) = db
            .get_player_session_row()
            .unwrap_or((None, 100.0, "none".into(), 1));

        let sleep_timer = Self::load_sleep_timer_from_db(&db);
        let (mut dsp_config, mut active_dsp_preset_id) = db
            .get_dsp_global_state()
            .unwrap_or_else(|_| (DspConfigDto::default(), Some("asmr_pure_guard".into())));

        let immersion = db.get_setting(super::immersion::SETTINGS_KEY).ok().flatten()
            .and_then(|raw|serde_json::from_str::<super::immersion::ImmersionSettings>(&raw).ok())
            .filter(|settings|settings.validate().is_ok()).unwrap_or_default();
        mpv.set_initial_immersion(immersion.reverse_stereo,immersion.gapless_playback);
        let mut initial = PlaybackStateInner {
            immersion, transition_serial:0, fade_in_pending:false,
            maintenance: MaintenanceClock::new(db.get_setting(super::engine_maintenance::SETTINGS_KEY).ok().flatten()
                .and_then(|raw| serde_json::from_str::<EngineMaintenanceSettings>(&raw).ok())
                .filter(|settings| settings.validate().is_ok()).unwrap_or_default()),
            session_generation: saved_gen.max(1),
            state: "idle".into(),
            user_paused_intent: true,
            current_queue_item_id: saved_qid.clone(),
            current_work_id: None,
            current_work_title: None,
            current_track_id: None,
            current_track_title: None,
            current_version_id: None,
            active_source: None,
            alternative_sources: Vec::new(),
            position_ms: 0,
            current_completed: false,
            duration_ms: None,
            pending_seek_ms: None,
            file_loaded_for_gen: false,
            retried_online_for_gen: false,
            mpv_buffering: false,
            buffering_since: None,
            online_recovery_running: false,
            auto_healed_for_gen: false,
            volume: saved_vol,
            loop_mode: saved_loop,
            active_ab_loop: None,
            sleep_timer,
            dsp_config: dsp_config.clone(),
            active_dsp_preset_id: active_dsp_preset_id.clone(),
            audio_exclusive,
            smtc_mode,
            audio_telemetry: AudioStreamTelemetryDto {
                codec: None,
                sample_rate_hz: None,
                sample_format: None,
                source_bit_depth: None,
                source_sample_rate_hz: None,
                source_bitrate_kbps: None,
                channels: None,
                bitrate_kbps: None,
                active_dsp_summary: summarize_dsp_chain(&dsp_config),
                active_filter_graph: build_lavfi_graph(&dsp_config),
            },
            last_error: None,
            last_persisted_at: Instant::now(),
            last_smtc_timeline_sync_at: Instant::now() - Duration::from_secs(2),
            prefetched_next: None,
            prefetching_version_id: None,
            prefetched_at: None,
            last_prefetch_check: None,
            cached_audio_tags: None,
            active_subtitle_cues: Vec::new(),
        };

        // T212: Restore session in paused state on startup
        if let Ok(queue) = db.get_queue_items() {
            let matched_item = saved_qid
                .as_ref()
                .and_then(|qid| queue.iter().find(|i| &i.id == qid))
                .or_else(|| queue.first());
            if let Some(item) = matched_item {
                if let Ok(Some((w_preset, w_cfg, true))) = db.get_work_dsp_override(&item.work_id) {
                    dsp_config = w_cfg;
                    active_dsp_preset_id = w_preset;
                    initial.dsp_config = dsp_config.clone();
                    initial.active_dsp_preset_id = active_dsp_preset_id;
                    initial.audio_telemetry.active_dsp_summary = summarize_dsp_chain(&dsp_config);
                    initial.audio_telemetry.active_filter_graph = build_lavfi_graph(&dsp_config);
                }
                initial.state = "paused".into();
                initial.current_queue_item_id = Some(item.id.clone());
                initial.current_work_id = Some(item.work_id.clone());
                initial.current_work_title = Some(item.work_title.clone());
                initial.current_track_id = Some(item.track_id.clone());
                initial.current_track_title = Some(item.track_title.clone());
                initial.current_version_id = Some(item.version_id.clone());
                initial.position_ms = if item.completed { 0 } else { item.position_ms };
                initial.current_completed = item.completed;
                initial.duration_ms = item.duration_ms;
                initial.pending_seek_ms = if initial.position_ms > 0 {
                    Some(initial.position_ms)
                } else {
                    None
                };
                let lib = crate::services::library::LibraryService::new(db.clone());
                if let Ok(Some(att)) = lib.find_best_subtitle_for_version(&item.version_id) {
                    if let Ok(cues) = lib.load_subtitle_cues(&att.id) {
                        initial.active_subtitle_cues = cues;
                    }
                }
                initial.active_subtitle_cues = db.subtitle_overlay(&item.version_id, initial.active_subtitle_cues);
            }
        }

        mpv.set_initial_af_graph(build_lavfi_graph(&initial.dsp_config));

        let svc = Self {
            db,
            provider,
            mpv,
            inner: Arc::new(Mutex::new(initial)),
            event_seq: Arc::new(AtomicU64::new(1)),
            app_handle: Arc::new(Mutex::new(None)),
            native_smtc: Arc::new(Mutex::new(None)),
            audio_output_operation: Arc::new(tokio::sync::Mutex::new(())),
            load_operation: Arc::new(tokio::sync::Mutex::new(())),
            recovery_operation: Arc::new(tokio::sync::Mutex::new(())),
            shutdown_signal: watch::channel(false).0,
            prefetch_operation: Arc::new(tokio::sync::Mutex::new(())),
        };

        svc.restore_audio_info();
        svc.spawn_event_loop(event_rx);
        svc.spawn_sleep_timer_loop();
        svc.spawn_buffering_monitor();
        svc
    }

    pub fn attach_app_handle(&self, handle: AppHandle) {
        // Rebinding must revoke the previous controller before enabling its replacement.
        let previous = self.native_smtc.lock().take();
        drop(previous);
        #[cfg(target_os = "windows")]
        if let Some(main_win) = handle.get_webview_window("main") {
            if let Ok(hwnd) = main_win.hwnd() {
                let svc_for_cb = self.clone();
                match NativeSmtcController::bind_for_window(
                    hwnd.0 as isize,
                    move |action, seek_ms| {
                        let svc_task = svc_for_cb.clone();
                        spawn_bg_task(async move {
                            let _ = svc_task.handle_smtc_action(&action, seek_ms).await;
                        });
                    },
                ) {
                    Ok(ctrl) => {
                        if let Ok(smtc_state) = self.get_smtc_display_state() {
                            ctrl.sync_state(&smtc_state);
                        }
                        *self.native_smtc.lock() = Some(ctrl);
                    }
                    Err(err) => {
                        self.db
                            .append_log("WARN", &format!("Failed to bind native SMTC: {err}"));
                    }
                }
            }
        }
        *self.app_handle.lock() = Some(handle);
    }

    pub fn set_proxy(&self, proxy: Option<String>) {
        self.mpv.set_proxy(proxy);
    }

    pub fn set_hardware_acceleration(&self, enabled: bool) {
        self.mpv.set_hardware_acceleration(enabled);
    }

    /// T909: Asynchronously pre-warms the `mpv` sidecar process and Named Pipe IPC in the background
    /// after the main window has mounted, eliminating cold-start latency on first click.
    pub async fn prewarm_engine(&self) -> AppResult<()> {
        let vol = self.inner.lock().volume;
        self.mpv.ensure_started().await?;
        let _ = self.mpv.set_volume(vol).await;
        Ok(())
    }

    fn load_sleep_timer_from_db(db: &Database) -> SleepTimerDto {
        let saved_fade = db
            .get_setting("sleep_fade_out_seconds")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|v| v.clamp(0, 120))
            .unwrap_or(30);
        db.with_conn(|conn| {
            let row: Option<(String, Option<String>, Option<i64>)> = conn
                .query_row(
                    "SELECT mode, deadline_utc, target_generation FROM sleep_timer_state WHERE id = 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            if let Some((mode, deadline_utc, target_gen)) = row {
                let remaining = deadline_utc
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| (dt.with_timezone(&Utc) - Utc::now()).num_seconds().max(0));
                Ok(SleepTimerDto {
                    mode,
                    deadline_utc,
                    remaining_seconds: remaining,
                    target_generation: target_gen.map(|g| g as u64),
                    fade_out_seconds: saved_fade,
                })
            } else {
                Ok(SleepTimerDto {
                    mode: "off".into(),
                    deadline_utc: None,
                    remaining_seconds: None,
                    target_generation: None,
                    fade_out_seconds: saved_fade,
                })
            }
        })
        .unwrap_or(SleepTimerDto {
            mode: "off".into(),
            deadline_utc: None,
            remaining_seconds: None,
            target_generation: None,
            fade_out_seconds: saved_fade,
        })
    }

    pub fn get_snapshot(&self) -> AppResult<PlaybackSnapshot> {
        let queue = self.db.get_queue_items()?;
        let guard = self.inner.lock();
        let seq = self.event_seq.fetch_add(1, Ordering::SeqCst);

        let mut st = guard.sleep_timer.clone();
        if st.mode == "minutes" {
            if let Some(ref dl) = st.deadline_utc {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(dl) {
                    st.remaining_seconds =
                        Some((dt.with_timezone(&Utc) - Utc::now()).num_seconds().max(0));
                }
            }
        }

        let mut telemetry = guard.audio_telemetry.clone();
        if telemetry.codec.is_none() {
            if let Some(ref src) = guard.active_source {
                if !src.format.is_empty() {
                    telemetry.codec = Some(src.format.to_uppercase());
                }
            }
        }
        telemetry.active_dsp_summary = summarize_dsp_chain(&guard.dsp_config);
        telemetry.active_filter_graph = super::immersion::output_graph(&build_lavfi_graph(&guard.dsp_config),guard.immersion.reverse_stereo);
        if guard.immersion.reverse_stereo { telemetry.active_dsp_summary.push_str(" · 声道翻转"); }

        Ok(PlaybackSnapshot {
            event_seq: seq,
            session_generation: guard.session_generation,
            state: guard.state.clone(),
            current_queue_item_id: guard.current_queue_item_id.clone(),
            current_work_id: guard.current_work_id.clone(),
            current_work_title: guard.current_work_title.clone(),
            current_track_id: guard.current_track_id.clone(),
            current_track_title: guard.current_track_title.clone(),
            current_version_id: guard.current_version_id.clone(),
            active_source: guard.active_source.clone(),
            alternative_sources: guard.alternative_sources.clone(),
            position_ms: guard.position_ms,
            duration_ms: guard.duration_ms,
            volume: guard.volume,
            loop_mode: guard.loop_mode.clone(),
            active_ab_loop: guard.active_ab_loop.clone(),
            sleep_timer: st,
            dsp_config: guard.dsp_config.clone(),
            active_dsp_preset_id: guard.active_dsp_preset_id.clone(),
            audio_exclusive: guard.audio_exclusive,
            smtc_mode: guard.smtc_mode.clone(),
            audio_telemetry: telemetry,
            last_error: guard.last_error.clone(),
            queue,
        })
    }

    pub fn emit_snapshot(&self) {
        if let Ok(smtc) = self.get_smtc_display_state() {
            self.inner.lock().last_smtc_timeline_sync_at = Instant::now();
            if let Some(ref ctrl) = *self.native_smtc.lock() {
                ctrl.sync_state(&smtc);
            }
            if let Some(ref handle) = *self.app_handle.lock() {
                let _ = handle.emit("smtc:state", &smtc);
            }
        }
        if let Ok(snap) = self.get_snapshot() {
            if let Some(ref handle) = *self.app_handle.lock() {
                let _ = handle.emit("playback:snapshot", &snap);
            }
        }
    }

    /// Pulses SMTC `MusicProperties.Title` (`"..."` -> `clean_title`) when subtitles or `.winisland/config.toml`
    /// change mid-playback so external SMTC lyric consumers (`WinIsland`, etc.) immediately re-fetch local `.lrc` lyrics.
    pub fn retrigger_smtc_for_subtitles(&self) {
        if let Ok(smtc) = self.get_smtc_display_state() {
            if let Some(ref ctrl) = *self.native_smtc.lock() {
                ctrl.pulse_title_for_lyrics_reload(&smtc);
            }
        }
        let svc = self.clone();
        spawn_bg_task(async move {
            tokio::time::sleep(Duration::from_millis(85)).await;
            svc.emit_snapshot();
        });
    }

    pub fn notify_affinity(&self, work_id: &str) {
        if let Some(ref app) = *self.app_handle.lock() { let _ = app.emit("library:affinity", work_id); }
    }

    pub fn active_subtitles(&self) -> serde_json::Value {
        let guard = self.inner.lock();
        serde_json::json!({ "eventSeq": self.event_seq.load(Ordering::SeqCst), "versionId": guard.current_version_id, "cues": guard.active_subtitle_cues })
    }

    pub fn subtitle_context(&self) -> (u64, Option<String>, Vec<SubtitleCueDto>, Option<MediaSourceDto>) {
        let guard = self.inner.lock();
        (guard.session_generation, guard.current_version_id.clone(), guard.active_subtitle_cues.clone(), guard.active_source.clone())
    }

    pub(crate) fn current_version_for_generation(&self, generation: u64) -> Option<String> {
        let guard = self.inner.lock();
        (guard.session_generation == generation).then(|| guard.current_version_id.clone()).flatten()
    }

    /// Generated results may replace only the exact display captured by this task.
    /// A manual subtitle change or a newer playback generation wins.
    pub fn publish_generated_subtitles(&self, version: &str, generation: u64, expected: &[SubtitleCueDto], cues: Vec<SubtitleCueDto>) -> bool {
        let event = {
            let mut guard = self.inner.lock();
            if guard.session_generation != generation || guard.current_version_id.as_deref() != Some(version) || guard.active_subtitle_cues != expected { return false; }
            if guard.active_subtitle_cues == cues { return true; }
            guard.active_subtitle_cues = cues;
            serde_json::json!({"eventSeq":self.event_seq.fetch_add(1,Ordering::SeqCst),"versionId":version,"cues":guard.active_subtitle_cues})
        };
        if let Some(handle) = self.app_handle.lock().as_ref() { let _ = handle.emit("playback:subtitles",event); }
        self.emit_snapshot();
        self.retrigger_smtc_for_subtitles();
        true
    }

    /// Updates the active subtitle cues for the currently loaded version (if matching) and broadcasts
    /// them to the local AMLL server + Windows SMTC / WinIsland LRC bridge.
    pub fn set_active_subtitle_cues_for_version(
        &self,
        version_id: Option<&str>,
        cues: Vec<SubtitleCueDto>,
    ) {
        let cues = version_id.map(|version| self.db.subtitle_overlay(version, cues.clone())).unwrap_or(cues);
        let subtitle_update = {
            let mut guard = self.inner.lock();
            let matches = match (version_id, guard.current_version_id.as_deref()) {
                (Some(target), Some(cur)) => target == cur,
                (None, Some(_)) => true,
                _ => false,
            };
            if matches {
                let changed = guard.active_subtitle_cues != cues;
                guard.active_subtitle_cues = cues;
                changed.then(|| serde_json::json!({
                    "eventSeq": self.event_seq.fetch_add(1, Ordering::SeqCst),
                    "versionId": guard.current_version_id,
                    "cues": guard.active_subtitle_cues,
                }))
            } else {
                None
            }
        };
        if let Some(subtitles) = subtitle_update {
            if let Some(ref handle) = *self.app_handle.lock() {
                let _ = handle.emit("playback:subtitles", subtitles);
            }
            self.emit_snapshot();
            self.retrigger_smtc_for_subtitles();
        }
    }

    /// Synchronizes two-way SMTC `TimelineProperties` (`StartTime = 0`, `EndTime = duration_ms`, `Position = position_ms`)
    /// with the OS media overlay without resetting metadata.
    pub fn sync_native_smtc_timeline(&self) {
        let (enabled, dur_ms, pos_ms) = {
            let mut guard = self.inner.lock();
            guard.last_smtc_timeline_sync_at = Instant::now();
            let en = guard.smtc_mode != "disabled" && guard.current_version_id.is_some();
            (
                en,
                guard.duration_ms.unwrap_or(0).max(0),
                guard.position_ms.max(0),
            )
        };
        if !enabled {
            return;
        }
        if let Some(ref ctrl) = *self.native_smtc.lock() {
            ctrl.sync_timeline(0, dur_ms, pos_ms);
        }
        // Frontends already receive playback:tick; rebuilding full metadata here
        // copied cover bytes and regenerated subtitles every second.
    }

    /// T905: Builds a lightweight `PlaybackTickDto` without hitting SQLite or cloning `queue` / `alternative_sources`.
    pub fn build_tick(&self) -> PlaybackTickDto {
        let guard = self.inner.lock();
        let seq = self.event_seq.fetch_add(1, Ordering::SeqCst);
        let sleep_rem = if guard.sleep_timer.mode == "minutes" {
            guard
                .sleep_timer
                .deadline_utc
                .as_deref()
                .and_then(|dl| chrono::DateTime::parse_from_rfc3339(dl).ok())
                .map(|dt| (dt.with_timezone(&Utc) - Utc::now()).num_seconds().max(0))
        } else {
            None
        };
        PlaybackTickDto {
            event_seq: seq,
            session_generation: guard.session_generation,
            position_ms: guard.position_ms,
            duration_ms: guard.duration_ms,
            state: guard.state.clone(),
            sleep_remaining_seconds: sleep_rem,
        }
    }

    pub fn emit_tick(&self) {
        let tick = self.build_tick();
        if let Some(ref handle) = *self.app_handle.lock() {
            let _ = handle.emit("playback:tick", tick);
        }
    }

    /// T906: Silently pre-resolves the next queue item when `< 15 s` remain in the current track.
    pub async fn maybe_prefetch_next_track(&self) {
        // Only one resolver may run, even when ticks arrive during slow networking.
        let Ok(_operation) = self.prefetch_operation.try_lock() else { return; };
        let generation = self.inner.lock().session_generation;
        {
            let mut guard = self.inner.lock();
            if guard.prefetched_at.is_some_and(|at| at.elapsed() >= Duration::from_secs(60)) {
                guard.prefetched_next = None;
                guard.prefetched_at = None;
            }
        }
        let (cur_qid, cur_vid, loop_mode, already_prefetched, prefetching) = {
            let guard = self.inner.lock();
            (
                guard.current_queue_item_id.clone(),
                guard.current_version_id.clone(),
                guard.loop_mode.clone(),
                guard.prefetched_next.as_ref().map(|t| t.0.clone()),
                guard.prefetching_version_id.clone(),
            )
        };
        if loop_mode == "single" {
            return;
        }
        let queue = match self.db.get_queue_items() {
            Ok(q) if !q.is_empty() => q,
            _ => return,
        };
        let cur_idx = cur_qid
            .as_ref()
            .and_then(|qid| queue.iter().position(|q| &q.id == qid))
            .or_else(|| {
                cur_vid
                    .as_ref()
                    .and_then(|vid| queue.iter().position(|q| &q.version_id == vid))
            });

        let next_idx = if cur_idx.is_none() {
            Some(0)
        } else if cur_idx.unwrap() + 1 < queue.len() {
            Some(cur_idx.unwrap() + 1)
        } else if loop_mode == "queue" {
            Some(0)
        } else {
            None
        };
        let next_item = match next_idx.and_then(|idx| queue.get(idx)) {
            Some(item) => item.clone(),
            None => return,
        };

        if already_prefetched.as_deref() == Some(&next_item.version_id)
            || prefetching.as_deref() == Some(&next_item.version_id)
        {
            return;
        }

        {
            let mut guard = self.inner.lock();
            if guard.session_generation != generation { return; }
            guard.prefetching_version_id = Some(next_item.version_id.clone());
        }

        let sources = match self.list_sources_for_version(&next_item.version_id) {
            Ok(s) => s,
            Err(_) => {
                self.inner.lock().prefetching_version_id = None;
                return;
            }
        };
        let chosen = if let Some(ref pref_id) = next_item.preferred_source_id {
            sources
                .iter()
                .find(|s| &s.id == pref_id && s.availability == "available")
                .cloned()
        } else {
            sources
                .iter()
                .find(|s| s.availability == "available")
                .cloned()
        };

        let chosen_src = match chosen {
            Some(s) => s,
            None => {
                self.inner.lock().prefetching_version_id = None;
                return;
            }
        };

        if chosen_src.source_kind == "local_imported" || chosen_src.source_kind == "local_downloaded"
        {
            if let Some(ref abs) = chosen_src.absolute_path {
                if !abs.ends_with(".part") && Path::new(abs).is_file() {
                    let mut guard = self.inner.lock();
                    if guard.session_generation != generation { guard.prefetching_version_id = None; return; }
                    if guard.prefetching_version_id.as_deref() != Some(&next_item.version_id) { return; }
                    guard.prefetched_next = Some((
                        next_item.version_id,
                        chosen_src.id,
                        abs.clone(),
                        Vec::new(),
                    ));
                    guard.prefetching_version_id = None;
                    guard.prefetched_at = Some(Instant::now());
                    return;
                }
            }
        } else if chosen_src.source_kind == "online_provider" {
            let media_ref = ProviderMediaRef {
                provider: chosen_src
                    .provider
                    .clone()
                    .unwrap_or_else(|| "asmr_one".into()),
                source_work_id: chosen_src.source_work_id.clone().unwrap_or_default(),
                remote_file_id: chosen_src.remote_file_id.clone().unwrap_or_default(),
                remote_relative_path: chosen_src.remote_relative_path.clone().unwrap_or_default(),
            };
            if let Ok(resolved) = self
                .provider
                .resolve_media(&media_ref, MediaPurpose::Play, false)
                .await
            {
                let mut guard = self.inner.lock();
                if guard.session_generation != generation { guard.prefetching_version_id = None; return; }
                if guard.prefetching_version_id.as_deref() != Some(&next_item.version_id) { return; }
                guard.prefetched_next = Some((
                    next_item.version_id,
                    chosen_src.id,
                    resolved.url,
                    resolved.headers,
                ));
                guard.prefetching_version_id = None;
                guard.prefetched_at = Some(Instant::now());
                return;
            }
        }

        self.inner.lock().prefetching_version_id = None;
    }

    pub(crate) fn list_sources_for_version(&self, version_id: &str) -> AppResult<Vec<MediaSourceDto>> {
        self.read_sources_for_version(version_id, true)
    }

    fn read_sources_for_version(&self, version_id: &str, check_files: bool) -> AppResult<Vec<MediaSourceDto>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, version_id, source_kind, provider, source_work_id, remote_file_id,
                        remote_relative_path, root_id, relative_path, absolute_path, format,
                        byte_size, content_sha256, availability
                 FROM media_sources
                 WHERE version_id = ?1
                 ORDER BY CASE source_kind
                   WHEN 'local_imported' THEN 1
                   WHEN 'local_downloaded' THEN 2
                   ELSE 3 END ASC",
            )?;
            let rows = stmt.query_map(params![version_id], |mr| {
                Ok(MediaSourceDto {
                    id: mr.get(0)?,
                    version_id: mr.get(1)?,
                    source_kind: mr.get(2)?,
                    provider: mr.get(3)?,
                    source_work_id: mr.get(4)?,
                    remote_file_id: mr.get(5)?,
                    remote_relative_path: mr.get(6)?,
                    root_id: mr.get(7)?,
                    relative_path: mr.get(8)?,
                    absolute_path: mr.get(9)?,
                    format: mr.get(10)?,
                    byte_size: mr.get(11)?,
                    content_sha256: mr.get(12)?,
                    availability: mr.get(13)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                let mut dto = r?;
                // Verify local file actually exists and is NEVER a .part file
                if check_files && (dto.source_kind == "local_imported" || dto.source_kind == "local_downloaded") {
                    if let Some(ref abs) = dto.absolute_path {
                        if abs.ends_with(".part") || !Path::new(abs).is_file() {
                            dto.availability = "missing".into();
                        }
                    } else {
                        dto.availability = "missing".into();
                    }
                }
                out.push(dto);
            }
            Ok(out)
        })
    }

    fn get_version_metadata(
        &self,
        version_id: &str,
    ) -> AppResult<(String, String, String, String, Option<i64>, i64, bool)> {
        self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT w.id, w.title, t.id, t.title, tv.duration_ms,
                        COALESCE(pp.position_ms, 0), COALESCE(pp.completed, 0)
                 FROM track_versions tv
                 JOIN tracks t ON t.id = tv.track_id
                 JOIN works w ON w.id = t.work_id
                 LEFT JOIN playback_positions pp ON pp.version_id = tv.id
                 WHERE tv.id = ?1",
                params![version_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)? == 1,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到对应音轨版本"))
        })
    }
}

impl PlaybackService {
    fn restore_audio_info(&self) {
        let mut state = self.inner.lock();
        let Some(version_id) = state.current_version_id.as_deref() else { return };
        // Startup must only read SQLite. Probing removable/network drives here
        // could block the UI thread; playback still validates files before use.
        let sources = self.read_sources_for_version(version_id, false).unwrap_or_default();
        if let Some(saved) = self.db.get_setting("last_session_audio_info").ok().flatten()
            .and_then(|json| serde_json::from_str::<SavedAudioInfo>(&json).ok())
            .filter(|saved| saved.version_id == version_id && sources.iter().any(|source|
                source.id == saved.source_id && source.format == saved.format && source.byte_size == saved.byte_size))
        {
            state.audio_telemetry.codec = saved.telemetry.codec;
            state.audio_telemetry.sample_rate_hz = saved.telemetry.sample_rate_hz;
            state.audio_telemetry.sample_format = saved.telemetry.sample_format;
            state.audio_telemetry.source_bit_depth = saved.telemetry.source_bit_depth;
            state.audio_telemetry.source_sample_rate_hz = saved.telemetry.source_sample_rate_hz;
            state.audio_telemetry.source_bitrate_kbps = saved.telemetry.source_bitrate_kbps;
            state.audio_telemetry.channels = saved.telemetry.channels;
            state.audio_telemetry.bitrate_kbps = saved.telemetry.bitrate_kbps;
        } else {
            state.audio_telemetry.codec = sources.first().map(|source| source.format.to_uppercase());
        }
        state.alternative_sources = sources;
    }

    pub fn persist_current_state(&self, mark_recent: bool) {
        if let Err(error) = self.try_persist_current_state(mark_recent) {
            self.db.append_log("ERROR", &format!("保存播放状态失败: {error}"));
        }
    }

    fn try_persist_current_state(&self, mark_recent: bool) -> AppResult<()> {
        // Serialize the session pointer with queue-plan replacement (inner -> database).
        let mut guard = self.inner.lock();
        let (ver_opt, pos, qid_opt, vol, lmode, sgen, completed) = {
            guard.last_persisted_at = Instant::now();
            (
                guard.current_version_id.clone(),
                guard.position_ms,
                guard.current_queue_item_id.clone(),
                guard.volume,
                guard.loop_mode.clone(),
                guard.session_generation,
                guard.current_completed,
            )
        };

        if let Some(vid) = ver_opt {
            self.db.save_playback_position(&vid, pos, completed, mark_recent)?;
            if let Some(source) = guard.active_source.as_ref().filter(|_| guard.audio_telemetry.sample_rate_hz.is_some() || guard.audio_telemetry.codec.is_some()) {
                let saved = SavedAudioInfo {
                    version_id: vid, source_id: source.id.clone(), format: source.format.clone(),
                    byte_size: source.byte_size, telemetry: guard.audio_telemetry.clone(),
                };
                self.db.set_setting("last_session_audio_info", &serde_json::to_string(&saved)
                    .map_err(|e| AppError::new(ErrorCode::InternalError, e.to_string()))?)?;
            }
        }
        self
            .db
            .update_player_session_row(qid_opt.as_deref(), vol, &lmode, sgen)?;
        Ok(())
    }

    /// T208/T305/T213: Unified playback entry `play(versionId, preferredSourceId?)`.
    pub async fn play(
        &self,
        version_id: &str,
        preferred_source_id: Option<String>,
    ) -> AppResult<PlaybackSnapshot> {
        self.play_internal(version_id, preferred_source_id, false, None)
            .await
    }

    pub async fn play_internal(
        &self,
        version_id: &str,
        preferred_source_id: Option<String>,
        force_online_refresh: bool,
        artificial_resolve_delay: Option<Duration>,
    ) -> AppResult<PlaybackSnapshot> {
        self.play_with_position(version_id, preferred_source_id, force_online_refresh, artificial_resolve_delay, None).await
    }

    /// T1503: bind the bookmark time to the same generation as its exact version.
    /// A separate frontend play/seek sequence could seek a newer, unrelated track.
    pub async fn resume_synced(&self, version_id: &str, position_ms: i64) -> AppResult<PlaybackSnapshot> {
        self.play_with_position(version_id, None, false, None, Some(position_ms.max(0))).await
    }

    pub async fn play_bookmark(&self, bookmark_id: &str) -> AppResult<PlaybackSnapshot> {
        let bm = self.db.get_bookmark(bookmark_id)?;
        self.play_with_position(&bm.version_id, None, false, None, Some(bm.position_ms)).await
    }

    async fn play_with_position(
        &self,
        version_id: &str,
        preferred_source_id: Option<String>,
        force_online_refresh: bool,
        artificial_resolve_delay: Option<Duration>,
        start_position_ms: Option<i64>,
    ) -> AppResult<PlaybackSnapshot> {
        self.play_with_transition(version_id,preferred_source_id,force_online_refresh,artificial_resolve_delay,start_position_ms,false).await
    }

    async fn play_with_transition(
        &self,
        version_id: &str,
        preferred_source_id: Option<String>,
        force_online_refresh: bool,
        artificial_resolve_delay: Option<Duration>,
        start_position_ms: Option<i64>,
        natural_continuation: bool,
    ) -> AppResult<PlaybackSnapshot> {
        // Save previous track progress before switching (T211)
        self.persist_current_state(false);

        let (work_id, work_title, track_id, track_title, db_dur_ms, saved_pos_ms, completed) =
            self.get_version_metadata(version_id)?;

        let all_sources = self.list_sources_for_version(version_id)?;
        let available_sources: Vec<MediaSourceDto> = all_sources
            .iter()
            .filter(|s| s.availability == "available")
            .cloned()
            .collect();

        // Resolve effective DSP config for target work (per-work override > global state, T801)
        let (target_dsp_cfg, target_dsp_preset) =
            if let Ok(Some((w_preset, w_cfg, true))) = self.db.get_work_dsp_override(&work_id) {
                (w_cfg, w_preset)
            } else {
                self.db
                    .get_dsp_global_state()
                    .unwrap_or_else(|_| (DspConfigDto::default(), Some("asmr_pure_guard".into())))
            };

        // Increment sessionGeneration and clear active A-B loop on track switch (T213, T603, R05, R15)
        let (my_gen, resume_pos_ms, volume, af_graph, transition, fade_out, fade_in, fade_ms) = {
            let mut guard = self.inner.lock();
            // Bind under the same lock as queue-plan replacement. Preserve the selected
            // occurrence when a version appears more than once in a saved queue.
            let queue = self.db.get_queue_items()?;
            let existing = queue.iter().find(|q| Some(&q.id) == guard.current_queue_item_id.as_ref() && q.version_id == version_id)
                .or_else(|| queue.iter().find(|q| q.version_id == version_id));
            let queue_item_id = if let Some(item) = existing { Some(item.id.clone()) } else {
                self.db.append_queue_items(&[(version_id.to_string(), preferred_source_id.clone())])?.into_iter().next()
            };
            let fade_out=!natural_continuation&&guard.immersion.fade_enabled&&!guard.current_completed&&guard.file_loaded_for_gen&&!guard.user_paused_intent
                &&matches!(guard.state.as_str(),"playing"|"buffering");
            let fade_in=guard.immersion.fade_enabled&&!(natural_continuation&&guard.immersion.gapless_playback);
            guard.transition_serial+=1;
            guard.fade_in_pending=fade_in;
            guard.session_generation += 1;
            guard.last_prefetch_check = None;
            guard.maintenance.active();
            let gen_val = guard.session_generation;
            // T212: Replaying a completed track individually starts from 0
            let start_pos = start_position_ms.unwrap_or(if completed { 0 } else { saved_pos_ms.max(0) }).max(0);

            guard.state = "resolving".into();
            guard.user_paused_intent = false;
            guard.current_queue_item_id = queue_item_id.clone();
            guard.current_work_id = Some(work_id);
            guard.current_work_title = Some(work_title);
            guard.current_track_id = Some(track_id);
            guard.current_track_title = Some(track_title);
            guard.current_version_id = Some(version_id.to_string());
            guard.active_source = None;
            guard.alternative_sources = available_sources.clone();
            guard.position_ms = start_pos;
            guard.current_completed = false;
            guard.duration_ms = db_dur_ms;
            guard.pending_seek_ms = if start_pos > 0 { Some(start_pos) } else { None };
            guard.file_loaded_for_gen = false;
            // A user-initiated retry starts a fresh attempt. Refreshing an expired
            // URL here must not consume this generation's recovery allowance.
            guard.retried_online_for_gen = false;
            guard.mpv_buffering = false;
            guard.buffering_since = None;
            guard.online_recovery_running = false;
            guard.auto_healed_for_gen = false;
            guard.active_ab_loop = None;
            guard.dsp_config = target_dsp_cfg.clone();
            guard.active_dsp_preset_id = target_dsp_preset;
            let graph = build_lavfi_graph(&target_dsp_cfg);
            guard.audio_telemetry = AudioStreamTelemetryDto {
                codec: None,
                sample_rate_hz: None,
                sample_format: None,
                source_bit_depth: None,
                source_sample_rate_hz: None,
                source_bitrate_kbps: None,
                channels: None,
                bitrate_kbps: None,
                active_dsp_summary: summarize_dsp_chain(&target_dsp_cfg),
                active_filter_graph: graph.clone(),
            };
            guard.last_error = None;
            (gen_val, start_pos, guard.volume, graph, guard.transition_serial, fade_out, fade_in, guard.immersion.fade_duration_ms)
        };
        if fade_out && !self.ramp_transport(my_gen,transition,0.0,fade_ms).await? { return self.get_snapshot(); }
        // Automatically match local subtitles on disk before emitting SMTC snapshot so WinIsland & AMLL
        // immediately receive the exported `.lrc` / `.ttml` subtitle stream on track start.
        let lib_svc = crate::services::library::LibraryService::new(self.db.clone());
        let mut initial_cues = Vec::new();
        let mut pending_online_sub_att_id: Option<String> = None;
        let mut need_cloud_sub_fetch = false;
        if let Ok(Some(att)) = lib_svc.find_best_subtitle_for_version(version_id) {
            if let Ok(cues) = lib_svc.load_subtitle_cues(&att.id) {
                initial_cues = cues;
            } else {
                pending_online_sub_att_id = Some(att.id);
            }
        } else {
            need_cloud_sub_fetch = true;
        }
        let initial_cues = self.db.subtitle_overlay(version_id, initial_cues);
        self.publish_generated_subtitles(version_id, my_gen, &self.subtitle_context().2, initial_cues.clone());

        if pending_online_sub_att_id.is_some() || need_cloud_sub_fetch {
            let svc_sub = self.clone();
            let vid_sub = version_id.to_string();
            let wid_sub = self.inner.lock().current_work_id.clone();
            spawn_bg_task(async move {
                let lib = crate::services::library::LibraryService::new(svc_sub.db.clone());
                if let Some(att_id) = pending_online_sub_att_id {
                    let _ = svc_sub.provider.ensure_online_subtitle_cached(&att_id).await;
                    if let Ok(cues) = lib.load_subtitle_cues(&att_id) {
                        if !cues.is_empty() {
                            let cues = svc_sub.db.subtitle_overlay(&vid_sub, cues);
                            svc_sub.publish_generated_subtitles(&vid_sub, my_gen, &initial_cues, cues);
                            return;
                        }
                    }
                }
                if need_cloud_sub_fetch {
                    if let Some(wid) = wid_sub {
                        if svc_sub
                            .provider
                            .fetch_online_subtitles_for_work(&wid, None)
                            .await
                            .is_ok()
                        {
                            if let Ok(Some(att)) = lib.find_best_subtitle_for_version(&vid_sub) {
                                let _ = svc_sub.provider.ensure_online_subtitle_cached(&att.id).await;
                                if let Ok(cues) = lib.load_subtitle_cues(&att.id) {
                                    if !cues.is_empty() {
                                        let cues = svc_sub.db.subtitle_overlay(&vid_sub, cues);
                                        svc_sub.publish_generated_subtitles(&vid_sub, my_gen, &initial_cues, cues);
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }

        // Ensure local cover file is cached on disk for online works so SMTC & WinIsland always receive a local image stream
        if let Some(wid) = self.inner.lock().current_work_id.clone() {
            if let Ok((_, _, _, _, raw_cover, Some(src_wid))) =
                self.db.get_work_smtc_metadata(&wid)
            {
                let has_local_cover = raw_cover
                    .as_deref()
                    .map(|p| !p.starts_with("http") && Path::new(p).is_file())
                    .unwrap_or(false);
                if !has_local_cover {
                    let svc_cover = self.clone();
                    let wid_clone = wid.clone();
                    spawn_bg_task(async move {
                        if let Some(cached_path) = svc_cover
                            .provider
                            .download_and_cache_cover(&src_wid, raw_cover.as_deref())
                            .await
                        {
                            let _ = svc_cover.db.with_conn(|conn| {
                                conn.execute(
                                    "UPDATE works SET cover_ref = ?1 WHERE id = ?2",
                                    params![cached_path, wid_clone],
                                )?;
                                Ok(())
                            });
                            svc_cover.emit_snapshot();
                        }
                    });
                }
            }
        }

        self.mpv.set_initial_af_graph(af_graph.clone());
        self.emit_snapshot();

        // Select source: user preferred -> complete local (imported/downloaded) -> online_provider
        let chosen_source = if let Some(ref pref_id) = preferred_source_id {
            match all_sources.iter().find(|s| &s.id == pref_id) {
                Some(s) if s.availability == "available" => s.clone(),
                _ => {
                    let alts: Vec<MediaSourceDto> = available_sources
                        .into_iter()
                        .filter(|s| Some(&s.id) != preferred_source_id.as_ref())
                        .collect();
                    let err = AppError::new(
                        ErrorCode::SourceNotFound,
                        "指定的播放来源当前不可用，可选择同一版本的其他可用来源",
                    );
                    let mut guard = self.inner.lock();
                    if guard.session_generation == my_gen {
                        guard.state = "error".into();
                        guard.alternative_sources = alts;
                        guard.last_error = Some(err.clone());
                    }
                    drop(guard);
                    self.emit_snapshot();
                    return Err(err);
                }
            }
        } else {
            available_sources
                .first()
                .cloned()
                .ok_or_else(|| {
                    let err = AppError::new(
                        ErrorCode::SourceNotFound,
                        "当前版本没有任何可用的本地或在线来源",
                    );
                    let mut guard = self.inner.lock();
                    if guard.session_generation == my_gen {
                        guard.state = "error".into();
                        guard.last_error = Some(err.clone());
                    }
                    err
                })?
        };

        // Resolve target path or URL (T906: check prefetched_next cache first!)
        let prefetched_hit = if !force_online_refresh && artificial_resolve_delay.is_none() {
            let mut guard = self.inner.lock();
            if !guard.prefetched_at.is_some_and(|at| at.elapsed() < Duration::from_secs(60)) {
                guard.prefetched_next = None;
            }
            if let Some((ref pf_vid, ref pf_sid, _, _)) = guard.prefetched_next {
                if pf_vid == version_id && pf_sid == &chosen_source.id {
                    guard.prefetched_next.take().map(|(_, _, u, h)| (u, h))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let (target_uri, headers) = if let Some((pf_uri, pf_headers)) = prefetched_hit {
            (pf_uri, pf_headers)
        } else if chosen_source.source_kind == "local_imported"
            || chosen_source.source_kind == "local_downloaded"
        {
            let abs = chosen_source.absolute_path.clone().ok_or_else(|| {
                AppError::new(ErrorCode::SourceNotFound, "本地来源缺少文件路径")
            })?;
            if abs.ends_with(".part") || !Path::new(&abs).is_file() {
                return Err(AppError::new(
                    ErrorCode::SourceNotFound,
                    "本地媒体文件不存在或未完成下载",
                ));
            }
            (abs, Vec::new())
        } else {
            if let Some(delay) = artificial_resolve_delay {
                tokio::time::sleep(delay).await;
            }
            // Check generation right after async wait before continuing (R05)
            if self.inner.lock().session_generation != my_gen {
                return self.get_snapshot();
            }

            let media_ref = ProviderMediaRef {
                provider: chosen_source
                    .provider
                    .clone()
                    .unwrap_or_else(|| "asmr_one".into()),
                source_work_id: chosen_source.source_work_id.clone().unwrap_or_default(),
                remote_file_id: chosen_source.remote_file_id.clone().unwrap_or_default(),
                remote_relative_path: chosen_source
                    .remote_relative_path
                    .clone()
                    .unwrap_or_default(),
            };
            match self
                .provider
                .resolve_media(&media_ref, MediaPurpose::Play, force_online_refresh)
                .await
            {
                Ok(resolved) => (resolved.url, resolved.headers),
                Err(err) => {
                    let mut guard = self.inner.lock();
                    if guard.session_generation == my_gen {
                        guard.state = "error".into();
                        guard.last_error = Some(err.clone());
                    }
                    drop(guard);
                    self.emit_snapshot();
                    return Err(err);
                }
            }
        };

        // A delayed recovery must not replace a newer user-selected track.
        let _load = self.load_operation.lock().await;
        // Check generation again before instructing mpv (R05)
        {
            let mut guard = self.inner.lock();
            if guard.session_generation != my_gen {
                return Ok(drop_and_snap(&self, guard)?);
            }
            guard.state = "loading".into();
            guard.buffering_since = Some(Instant::now());
            guard.active_source = Some(chosen_source.clone());
            guard.alternative_sources = all_sources
                .into_iter()
                .filter(|s| s.id != chosen_source.id && s.availability == "available")
                .collect();
        }
        self.emit_snapshot();

        let fade_in=fade_in&&{let state=self.inner.lock();state.fade_in_pending&&state.immersion.fade_enabled};
        self.mpv.set_transition_gain(if fade_in {0.0} else {1.0}).await?;
        // T910: Pipeline load_file + volume + af filter graph in a single IPC batch
        let paused_on_load=self.inner.lock().user_paused_intent;
        if let Err(err) = self
            .mpv
            .load_file_pipelined(&target_uri, &headers, paused_on_load, volume, &af_graph)
            .await
        {
            let mut guard = self.inner.lock();
            if guard.session_generation == my_gen {
                guard.state = "error".into();
                guard.last_error = Some(err.clone());
            }
            drop(guard);
            self.emit_snapshot();
            return Err(err);
        }

        // Probe the actual selected source in the background; metadata cannot delay playback.
        let info_service = self.clone();
        spawn_bg_task(async move {
            let depth = if chosen_source.source_kind == "online_provider" {
                if !matches!(chosen_source.format.to_ascii_lowercase().as_str(), "wav" | "wave" | "rf64" | "flac" | "aif" | "aiff" | "aifc") {
                    return;
                }
                match info_service.provider.build_client() {
                    Ok(client) => super::source_audio_info::online_bit_depth(&client, &target_uri, &headers).await,
                    Err(_) => None,
                }
            } else {
                super::source_audio_info::local_bit_depth(&target_uri).await
            };
            info_service.apply_source_bit_depth(my_gen, &chosen_source.id, depth);
        });

        // Record playback start in work_playback_state (T404)
        let _ = self
            .db
            .save_playback_position(version_id, resume_pos_ms, false, true);
        self.persist_current_state(true);

        self.get_snapshot()
    }

    fn apply_source_bit_depth(&self, generation: u64, source_id: &str, depth: Option<u16>) {
        {
            let mut state = self.inner.lock();
            if state.session_generation != generation || state.active_source.as_ref().map(|s| s.id.as_str()) != Some(source_id) {
                return;
            }
            state.audio_telemetry.source_bit_depth = depth;
        }
        self.persist_current_state(false);
        self.emit_snapshot();
    }

    /// T209/R03: Builds queue scoped to the user-selected directory group (`group_path`) and starts playback.
    /// Never mixes unconfirmed MP3/WAV directories automatically.
    pub async fn play_directory_group(
        &self,
        work_id: &str,
        group_path: &str,
        start_version_id: Option<String>,
    ) -> AppResult<PlaybackSnapshot> {
        self.play_directory_group_at(work_id, group_path, start_version_id, false).await
    }

    pub async fn play_work(&self, work_id: &str, group_path: Option<String>, from_start: bool) -> AppResult<PlaybackSnapshot> {
        let detail = self.db.get_work_detail(work_id)?;
        let last_track = detail.tracks.iter().find(|t| t.versions.iter().any(|v| Some(&v.id) == detail.last_version_id.as_ref()));
        let group = group_path.or_else(|| last_track.map(|t| t.logical_dir_path.clone()))
            .or_else(|| detail.directory_groups.first().map(|g| g.group_path.clone()))
            .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "作品中没有可播放的音轨"))?;
        self.play_directory_group_at(work_id, &group, detail.last_version_id, from_start).await
    }

    async fn play_directory_group_at(
        &self,
        work_id: &str,
        group_path: &str,
        start_version_id: Option<String>,
        from_start: bool,
    ) -> AppResult<PlaybackSnapshot> {
        let detail = self.db.get_work_detail(work_id)?;
        let mut queue_pairs: Vec<(String, Option<String>)> = Vec::new();
        let mut seen_versions = std::collections::HashSet::new();

        for track in &detail.tracks {
            if track.logical_dir_path == group_path {
                if let Some(first_ver) = track.versions.first() {
                    if seen_versions.insert(first_ver.id.clone()) {
                        queue_pairs.push((first_ver.id.clone(), None));
                    }
                }
            }
        }

        if queue_pairs.is_empty() {
            return Err(AppError::new(
                ErrorCode::SourceNotFound,
                "所选目录下没有可播放的音轨",
            ));
        }

        let group_completed = detail.tracks.iter().filter(|t| t.logical_dir_path == group_path)
            .all(|t| t.versions.first().is_some_and(|v| v.completed));
        let restart = from_start || group_completed;
        let active_idx = if restart { 0 } else { start_version_id
            .as_ref()
            .and_then(|sv| queue_pairs.iter().position(|(vid, _)| vid == sv))
            .unwrap_or(0) };

        let target_version = queue_pairs[active_idx].0.clone();
        let active_qid = self.db.replace_queue_items(&queue_pairs, active_idx)?;
        {
            let mut guard = self.inner.lock();
            guard.current_queue_item_id = active_qid;
        }
        self.play_with_position(&target_version, None, false, None, restart.then_some(0)).await
    }

    /// T1117 / R37: Smartly selects the best playable `(version_id, preferred_source_id)` pairs for a work,
    /// preferring "With SE (有音效 / SEあり, excluding SEなし / 无音效 / NoSE)" and "WAV format" (`WAV > FLAC > MP3/M4A/OGG/OPUS`)
    /// without mixing duplicate format/SE directory groups into the queue.
    pub fn select_preferred_tracks_for_work(
        detail: &WorkDetailDto,
    ) -> Vec<(String, Option<String>)> {
        fn is_no_se_marker(text: &str) -> bool {
            let lower = text.to_lowercase();
            [
                "seなし",
                "se無し",
                "seナシ",
                "无音效",
                "無効果音",
                "効果音なし",
                "効果音無し",
                "nose",
                "no_se",
                "no-se",
                "without se",
                "without_se",
                "なし版",
                "無し版",
            ]
            .iter()
            .any(|kw| lower.contains(kw))
        }

        fn has_positive_se_marker(text: &str) -> bool {
            let lower = text.to_lowercase();
            [
                "seあり",
                "se有り",
                "seアリ",
                "有音效",
                "効果音あり",
                "効果音有り",
                "with se",
                "with_se",
            ]
            .iter()
            .any(|kw| lower.contains(kw))
        }

        fn score_audio_format(fmt: &str) -> i32 {
            match fmt.trim().to_lowercase().as_str() {
                "wav" => 300,
                "flac" => 200,
                "mp3" | "m4a" | "ogg" | "opus" | "aac" => 100,
                _ => 50,
            }
        }

        fn score_source_kind(kind: &str) -> i32 {
            match kind {
                "local_imported" => 30,
                "local_downloaded" => 20,
                "online_provider" => 10,
                _ => 0,
            }
        }

        let mut group_order: Vec<String> = Vec::new();
        for track in &detail.tracks {
            let has_avail = track.versions.iter().any(|v| {
                v.sources
                    .iter()
                    .any(|s| s.availability == "available" && !s.format.is_empty())
            });
            if has_avail && !group_order.contains(&track.logical_dir_path) {
                group_order.push(track.logical_dir_path.clone());
            }
        }

        if group_order.is_empty() {
            return Vec::new();
        }

        fn is_bonus_marker(text: &str) -> bool {
            let lower = text.to_lowercase();
            [
                "特典",
                "おまけ",
                "ボーナストラック",
                "bonus",
                "extra",
                "フリートーク",
                "free talk",
                "試聴",
                "体験版",
                "予告",
                "trailer",
            ]
            .iter()
            .any(|kw| lower.contains(kw))
        }

        let best_group = group_order
            .iter()
            .max_by_key(|g_path| {
                let g_tracks: Vec<&TrackItemDto> = detail
                    .tracks
                    .iter()
                    .filter(|t| &t.logical_dir_path == *g_path)
                    .collect();
                let group_no_se = is_no_se_marker(g_path)
                    || (!g_tracks.is_empty()
                        && g_tracks
                            .iter()
                            .all(|t| is_no_se_marker(&t.title) || is_no_se_marker(&t.group_label)));
                let group_pos_se = has_positive_se_marker(g_path)
                    || g_tracks
                        .iter()
                        .any(|t| has_positive_se_marker(&t.title) || has_positive_se_marker(&t.group_label));
                let group_is_bonus = is_bonus_marker(g_path);

                let mut max_fmt = 0;
                let mut max_kind = 0;
                for t in &g_tracks {
                    for v in &t.versions {
                        for s in &v.sources {
                            if s.availability == "available" {
                                max_fmt = max_fmt.max(score_audio_format(&s.format));
                                max_kind = max_kind.max(score_source_kind(&s.source_kind));
                            }
                        }
                    }
                }
                (
                    !group_no_se,
                    max_fmt,
                    !group_is_bonus,
                    g_tracks.len(),
                    group_pos_se,
                    max_kind,
                )
            })
            .cloned()
            .unwrap_or_default();

        let group_tracks: Vec<&TrackItemDto> = detail
            .tracks
            .iter()
            .filter(|t| t.logical_dir_path == best_group)
            .collect();

        let has_non_no_se_tracks = group_tracks.iter().any(|t| !is_no_se_marker(&t.title));

        let mut out: Vec<(String, Option<String>)> = Vec::new();
        let mut seen_versions = std::collections::HashSet::new();

        for track in group_tracks {
            if has_non_no_se_tracks && is_no_se_marker(&track.title) {
                continue;
            }
            let mut best_candidate: Option<(i32, String, String)> = None;
            for ver in &track.versions {
                let ver_no_se = is_no_se_marker(&ver.version_label);
                let ver_pos_se = has_positive_se_marker(&ver.version_label);
                let ver_se_score = if !ver_no_se {
                    1000 + if ver_pos_se { 150 } else { 0 }
                } else {
                    0
                };
                for src in &ver.sources {
                    if src.availability != "available" {
                        continue;
                    }
                    let path_hint = src
                        .relative_path
                        .as_deref()
                        .or(src.remote_relative_path.as_deref())
                        .unwrap_or("");
                    let src_se_penalty = if is_no_se_marker(path_hint) { -500 } else { 0 };
                    let total_score = ver_se_score
                        + src_se_penalty
                        + score_audio_format(&src.format)
                        + score_source_kind(&src.source_kind);
                    if best_candidate
                        .as_ref()
                        .map(|(best_sc, _, _)| total_score > *best_sc)
                        .unwrap_or(true)
                    {
                        best_candidate = Some((total_score, ver.id.clone(), src.id.clone()));
                    }
                }
            }
            if let Some((_, vid, sid)) = best_candidate {
                if seen_versions.insert(vid.clone()) {
                    out.push((vid, Some(sid)));
                }
            }
        }

        out
    }

    /// Append selected works atomically without replacing the queue or starting playback.
    pub async fn append_works_to_queue(&self, work_ids: Vec<String>) -> AppResult<PlaybackSnapshot> {
        if work_ids.is_empty() || work_ids.len() > 500 {
            return Err(AppError::new(ErrorCode::InvalidInput, "请选择 1 至 500 部作品"));
        }
        let mut seen_works = std::collections::HashSet::new();
        let mut seen_versions = std::collections::HashSet::new();
        let mut pairs = Vec::new();
        for id in work_ids {
            if !seen_works.insert(id.clone()) { continue; }
            let mut detail = self.db.get_work_detail(&id)?;
            if detail.tracks.is_empty() && !detail.online_sources.is_empty() {
                self.provider.ensure_work_online_tracks(&id, None).await?;
                detail = self.db.get_work_detail(&id)?;
            }
            let selected = Self::select_preferred_tracks_for_work(&detail);
            if selected.is_empty() {
                return Err(AppError::new(ErrorCode::SourceNotFound,
                    format!("《{}》暂无可播放音轨，本次未加入队列", detail.title)));
            }
            pairs.extend(selected.into_iter().filter(|(id, _)| seen_versions.insert(id.clone())));
        }
        self.db.append_queue_items(&pairs)?;
        self.emit_snapshot();
        self.get_snapshot()
    }

    /// Saved playlists reuse the same queue, playback generation and source selection.
    pub async fn play_playlist(&self, ids: Vec<String>, append: bool, start: Option<String>) -> AppResult<PlaybackSnapshot> {
        if ids.is_empty() || ids.len()>500 {return Err(AppError::new(ErrorCode::InvalidInput,"歌单中没有可播放的音轨"));}
        let pairs=ids.into_iter().map(|id|(id,None)).collect::<Vec<_>>();
        if append {self.db.append_queue_items(&pairs)?;self.emit_snapshot();return self.get_snapshot();}
        let index=start.and_then(|id|pairs.iter().position(|(v,_)|v==&id)).unwrap_or(0);
        let qid=self.db.replace_queue_items(&pairs,index)?;
        {self.inner.lock().current_queue_item_id=qid;}
        self.play_with_position(&pairs[index].0,None,false,None,None).await
    }

    /// T1117 / R37: Enqueues all favorite works in order (preferring With-SE + WAV format) and starts playback immediately.
    pub async fn play_favorites_queue(
        &self,
        work_ids: Option<Vec<String>>,
    ) -> AppResult<PlaybackSnapshot> {
        let target_ids: Vec<String> = match work_ids {
            Some(ids) if !ids.is_empty() => ids,
            _ => self
                .db
                .list_works("favorites", None)?
                .into_iter()
                .map(|w| w.id)
                .collect(),
        };

        let mut queue_pairs: Vec<(String, Option<String>)> = Vec::new();
        let mut seen_versions = std::collections::HashSet::new();

        for wid in &target_ids {
            if let Ok(detail) = self.db.get_work_detail(wid) {
                for (vid, pref_sid) in Self::select_preferred_tracks_for_work(&detail) {
                    if seen_versions.insert(vid.clone()) {
                        queue_pairs.push((vid, pref_sid));
                    }
                }
            }
        }

        if queue_pairs.is_empty() {
            return Err(AppError::new(
                ErrorCode::SourceNotFound,
                "喜欢列表中暂无可播放的音轨（若为尚未解析目录树的在线作品，请先点开详情加载音轨）",
            ));
        }

        let (first_vid, first_sid) = queue_pairs[0].clone();
        let active_qid = self.db.replace_queue_items(&queue_pairs, 0)?;
        {
            let mut guard = self.inner.lock();
            guard.current_queue_item_id = active_qid;
        }
        self.play(&first_vid, first_sid).await
    }

    pub async fn toggle_pause(&self) -> AppResult<PlaybackSnapshot> {
        let (state, cur_ver, cur_src, user_paused, completed, loaded, generation) = {
            let guard = self.inner.lock();
            (
                guard.state.clone(),
                guard.current_version_id.clone(),
                guard.active_source.clone(),
                guard.user_paused_intent,
                guard.current_completed,
                guard.file_loaded_for_gen,
                guard.session_generation,
            )
        };

        if state == "error" {
            if let Some(vid) = cur_ver {
                let source_id = cur_src.as_ref().map(|s| s.id.clone());
                return self.play_internal(&vid, source_id, true, None).await;
            }
        }
        // If restored from startup in paused state and mpv hasn't loaded the file yet, load & play!
        if (state == "paused" || state == "idle" || state == "ended")
            && (!loaded || !self.mpv.is_running().await || cur_src.is_none() || completed || state == "ended") {
            if let Some(vid) = cur_ver {
                if self.inner.lock().session_generation != generation { return self.get_snapshot(); }
                return self.play(&vid, cur_src.map(|source| source.id)).await;
            }
        }

        let target_pause =
            state == "playing" || state == "buffering" || (state == "loading" && !user_paused);
        let (serial,fade,fade_ms) = {
            let mut guard = self.inner.lock();
            if guard.session_generation != generation { return drop_and_snap(self, guard); }
            guard.transition_serial+=1;
            guard.fade_in_pending=false;
            guard.user_paused_intent = target_pause;
            guard.buffering_since = None;
            guard.state = if target_pause {
                "paused".into()
            } else if guard.mpv_buffering {
                "buffering".into()
            } else {
                "playing".into()
            };
            (guard.transition_serial,guard.immersion.fade_enabled&&guard.file_loaded_for_gen,guard.immersion.fade_duration_ms)
        };
        if target_pause && fade && !self.ramp_transport(generation,serial,0.0,fade_ms).await? { return self.get_snapshot(); }
        let _load=self.load_operation.lock().await;
        {let state=self.inner.lock();if state.session_generation!=generation||state.transition_serial!=serial{return drop_and_snap(self,state);}}
        if !fade && !target_pause { self.mpv.set_transition_gain(1.0).await?; }
        if fade && !target_pause && self.mpv.command(serde_json::json!(["get_property","pause"])).await?.as_bool()==Some(true) {
            self.mpv.set_transition_gain(0.0).await?;
        }
        if let Err(error) = self.mpv.set_pause(target_pause).await {
            {
                let mut guard = self.inner.lock();
                if guard.session_generation != generation { return drop_and_snap(self, guard); }
                guard.state = "error".into();
                guard.file_loaded_for_gen = false;
                guard.user_paused_intent = true;
                guard.last_error = Some(error.clone());
            }
            self.db.append_log("WARN", &format!("Playback pause/resume failed: {error}"));
            self.persist_current_state(false);
            self.emit_snapshot();
            return Err(error);
        }
        drop(_load);
        if !target_pause && fade { let _=self.ramp_transport(generation,serial,1.0,fade_ms).await?; }
        self.persist_current_state(false);
        self.emit_snapshot();
        self.get_snapshot()
    }

    pub async fn seek_ms(&self, position_ms: i64) -> AppResult<PlaybackSnapshot> {
        let clamped = position_ms.max(0);
        let (file_loaded, target) = {
            let mut guard = self.inner.lock();
            let max_pos = guard.duration_ms.filter(|&d| d > 0).unwrap_or(i64::MAX);
            let target = clamped.min(max_pos);
            guard.position_ms = target;
            if !guard.file_loaded_for_gen {
                guard.pending_seek_ms = Some(target);
            }
            (guard.file_loaded_for_gen, target)
        };

        if file_loaded {
            let _ = self.mpv.seek_absolute_ms(target).await;
        }
        self.persist_current_state(false);
        self.sync_native_smtc_timeline();
        self.emit_snapshot();
        self.get_snapshot()
    }

    pub async fn set_volume(&self, volume: f64) -> AppResult<PlaybackSnapshot> {
        let clamped = volume.clamp(0.0, crate::player::MAX_MPV_VOLUME);
        {
            let mut guard = self.inner.lock();
            guard.volume = clamped;
        }
        let _ = self.mpv.set_volume(clamped).await;
        self.persist_current_state(false);
        self.emit_snapshot();
        self.get_snapshot()
    }

    pub fn set_loop_mode(&self, loop_mode: &str) -> AppResult<PlaybackSnapshot> {
        if !matches!(loop_mode, "none" | "single" | "queue") {
            return Err(AppError::new(ErrorCode::InvalidInput, "无效的循环模式"));
        }
        {
            let mut guard = self.inner.lock();
            guard.loop_mode = loop_mode.to_string();
        }
        self.persist_current_state(false);
        self.emit_snapshot();
        self.get_snapshot()
    }

    pub async fn step_queue(&self, delta: isize) -> AppResult<PlaybackSnapshot> {
        self.step_queue_transition(delta,false).await
    }
    async fn step_queue_transition(&self,delta:isize,natural:bool)->AppResult<PlaybackSnapshot> {
        let queue = self.db.get_queue_items()?;
        if queue.is_empty() {
            return self.get_snapshot();
        }
        let (cur_qid, cur_vid, loop_mode) = {
            let guard = self.inner.lock();
            (
                guard.current_queue_item_id.clone(),
                guard.current_version_id.clone(),
                guard.loop_mode.clone(),
            )
        };

        let cur_idx = cur_qid
            .as_ref()
            .and_then(|qid| queue.iter().position(|q| &q.id == qid))
            .or_else(|| {
                cur_vid
                    .as_ref()
                    .and_then(|vid| queue.iter().position(|q| &q.version_id == vid))
            });

        let next_idx_opt = if cur_idx.is_none() {
            Some(0)
        } else if delta >= 0 {
            let cur_idx = cur_idx.unwrap();
            let next = cur_idx + (delta as usize);
            if next < queue.len() {
                Some(next)
            } else if loop_mode == "queue" {
                Some(0)
            } else {
                None
            }
        } else {
            let cur_idx = cur_idx.unwrap();
            let back = (-delta) as usize;
            if cur_idx >= back {
                Some(cur_idx - back)
            } else if loop_mode == "queue" {
                Some(queue.len() - 1)
            } else {
                Some(0)
            }
        };

        if let Some(idx) = next_idx_opt {
            let item = &queue[idx];
            {
                let mut guard = self.inner.lock();
                guard.current_queue_item_id = Some(item.id.clone());
            }
            self.play_with_transition(&item.version_id,item.preferred_source_id.clone(),false,None,None,natural).await
        } else {
            let _ = self.mpv.stop().await;
            {
                let mut guard = self.inner.lock();
                guard.state = "ended".into();
                guard.user_paused_intent = true;
            }
            self.emit_snapshot();
            self.get_snapshot()
        }
    }

    pub fn set_active_ab_loop(
        &self,
        start_ms: i64,
        end_ms: i64,
        label: Option<String>,
        save_to_db: bool,
    ) -> AppResult<PlaybackSnapshot> {
        let (vid, dur_opt) = {
            let guard = self.inner.lock();
            (
                guard
                    .current_version_id
                    .clone()
                    .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "当前未加载音轨"))?,
                guard.duration_ms,
            )
        };
        if start_ms < 0 || start_ms >= end_ms {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "A-B 循环必须满足 0 <= A < B",
            ));
        }
        if let Some(dur) = dur_opt {
            if dur > 0 && end_ms > dur {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "B 点不能超过音轨总时长",
                ));
            }
        }

        let dto = if save_to_db {
            self.db
                .save_ab_loop(&vid, start_ms, end_ms, label.as_deref().unwrap_or("A-B 区间"))?
        } else {
            AbLoopDto {
                id: "active-temp".into(),
                version_id: vid,
                start_ms,
                end_ms,
                label: label.unwrap_or_else(|| "当前 A-B 循环".into()),
            }
        };

        {
            let mut guard = self.inner.lock();
            guard.active_ab_loop = Some(dto);
        }
        self.emit_snapshot();
        self.get_snapshot()
    }

    pub fn clear_active_ab_loop(&self) -> AppResult<PlaybackSnapshot> {
        {
            let mut guard = self.inner.lock();
            guard.active_ab_loop = None;
        }
        self.emit_snapshot();
        self.get_snapshot()
    }

    /// T601/T1101/R14: Configures the sleep timer (`off`, `minutes`, `end_of_track`) with optional logarithmic fade-out seconds.
    pub fn set_sleep_timer(&self, mode: &str, minutes: Option<u32>) -> AppResult<PlaybackSnapshot> {
        self.set_sleep_timer_with_fade(mode, minutes, None)
    }

    pub fn set_sleep_timer_with_fade(
        &self,
        mode: &str,
        minutes: Option<u32>,
        fade_out_seconds: Option<i64>,
    ) -> AppResult<PlaybackSnapshot> {
        if !matches!(mode, "off" | "minutes" | "end_of_track") {
            return Err(AppError::new(ErrorCode::InvalidInput, "无效的睡眠定时模式"));
        }
        let now = Utc::now();
        let deadline_utc = if mode == "minutes" {
            let mins = minutes.unwrap_or(30).max(1) as i64;
            Some((now + chrono::Duration::minutes(mins)).to_rfc3339())
        } else {
            None
        };

        let (target_gen, fade_secs, base_vol, is_active) = {
            let mut guard = self.inner.lock();
            let gen_val = guard.session_generation;
            let rem = if mode == "minutes" {
                Some((minutes.unwrap_or(30).max(1) as i64) * 60)
            } else {
                None
            };
            let fade = fade_out_seconds
                .map(|v| v.clamp(0, 120))
                .unwrap_or(guard.sleep_timer.fade_out_seconds);
            guard.sleep_timer = SleepTimerDto {
                mode: mode.to_string(),
                deadline_utc: deadline_utc.clone(),
                remaining_seconds: rem,
                target_generation: Some(gen_val),
                fade_out_seconds: fade,
            };
            (gen_val, fade, guard.volume, guard.file_loaded_for_gen)
        };

        if let Some(_) = fade_out_seconds {
            let _ = self
                .db
                .set_setting("sleep_fade_out_seconds", &fade_secs.to_string());
        }

        // If user turned off or reset sleep timer, restore base volume immediately
        if is_active {
            let mpv_clone = self.mpv.clone();
            spawn_bg_task(async move {
                let _ = mpv_clone.set_volume(base_vol).await;
            });
        }

        let now_str = now.to_rfc3339();
        self.db.with_conn(|conn| {
            conn.execute(
                "UPDATE sleep_timer_state
                 SET mode = ?1, deadline_utc = ?2, target_generation = ?3, fired_at = NULL, updated_at = ?4
                 WHERE id = 1",
                params![mode, deadline_utc, target_gen as i64, now_str],
            )?;
            Ok(())
        })?;

        self.emit_snapshot();
        self.get_snapshot()
    }

    /// T601/T1101/R14/R24: Evaluates if the minute-based sleep timer is in the logarithmic fade-out window
    /// or has expired (including after OS sleep/wake), applying smooth fade-out and restoring user base volume after pausing.
    pub async fn check_and_fire_sleep_timer(&self) -> bool {
        let (should_fire, fade_vol_opt, base_vol) = {
            let mut guard = self.inner.lock();
            let base = guard.volume;
            if guard.sleep_timer.mode == "minutes" {
                if let Some(ref dl) = guard.sleep_timer.deadline_utc {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(dl) {
                        let rem_secs = (dt.with_timezone(&Utc) - Utc::now()).num_seconds();
                        if rem_secs <= 0 {
                            guard.sleep_timer.mode = "off".into();
                            guard.sleep_timer.deadline_utc = None;
                            guard.sleep_timer.remaining_seconds = None;
                            guard.user_paused_intent = true;
                            if guard.state == "playing" || guard.state == "buffering" {
                                guard.state = "paused".into();
                            }
                            (true, None, base)
                        } else {
                            guard.sleep_timer.remaining_seconds = Some(rem_secs);
                            let fade_secs = guard.sleep_timer.fade_out_seconds;
                            let fade_v = if guard.state == "playing"
                                && fade_secs > 0
                                && rem_secs <= fade_secs
                            {
                                let mult = compute_sleep_fade_multiplier(rem_secs, fade_secs);
                                Some(base * mult)
                            } else {
                                None
                            };
                            (false, fade_v, base)
                        }
                    } else {
                        (false, None, base)
                    }
                } else {
                    (false, None, base)
                }
            } else {
                (false, None, base)
            }
        };

        if let Some(attenuated_vol) = fade_vol_opt {
            let _ = self.mpv.set_volume(attenuated_vol).await;
            self.emit_tick();
        }

        if should_fire {
            let now_str = Utc::now().to_rfc3339();
            let _ = self.db.with_conn(|conn| {
                conn.execute(
                    "UPDATE sleep_timer_state SET mode = 'off', deadline_utc = NULL, fired_at = ?1, updated_at = ?1 WHERE id = 1",
                    params![now_str],
                )?;
                Ok(())
            });
            let _ = self.mpv.set_pause(true).await;
            // T1101/R24: Restore user's base volume after pausing so next play starts at normal level
            let _ = self.mpv.set_volume(base_vol).await;
            self.persist_current_state(false);
            self.emit_snapshot();
        }
        should_fire
    }

    pub async fn get_audio_output(&self) -> AppResult<AudioOutputStateDto> {
        let _operation = self.audio_output_operation.lock().await;
        Ok(AudioOutputStateDto {
            devices: self.mpv.audio_devices().await?,
            selected_device: self.mpv.selected_audio_device(),
        })
    }

    pub async fn set_audio_device(&self, device_id: &str) -> AppResult<AudioOutputStateDto> {
        let _operation = self.audio_output_operation.lock().await;
        let devices = self.mpv.audio_devices().await?;
        if !devices.iter().any(|device| device.id == device_id) {
            return Err(AppError::new(ErrorCode::InvalidInput, "输出设备不可用，请刷新设备列表"));
        }
        let previous = self.mpv.selected_audio_device();
        self.mpv.set_audio_device(device_id).await?;
        if let Err(error) = self.db.set_setting("audio_device", device_id) {
            let _ = self.mpv.set_audio_device(&previous).await;
            return Err(error);
        }
        Ok(AudioOutputStateDto { devices, selected_device: device_id.into() })
    }

    /// T1103/R27: Toggles WASAPI Exclusive Mode (`audio_exclusive`) and persists setting.
    pub async fn set_audio_exclusive(&self, exclusive: bool) -> AppResult<PlaybackSnapshot> {
        let _operation = self.audio_output_operation.lock().await;
        let actual = match self.mpv.set_audio_exclusive(exclusive).await {
            Ok(v) => v,
            Err(err) => {
                {
                    let mut guard = self.inner.lock();
                    guard.audio_exclusive = false;
                    guard.last_error = Some(err.clone());
                }
                let _ = self.db.set_setting("audio_exclusive", "false");
                self.emit_snapshot();
                return Err(err);
            }
        };
        {
            let mut guard = self.inner.lock();
            guard.audio_exclusive = actual;
        }
        let _ = self
            .db
            .set_setting("audio_exclusive", if actual { "true" } else { "false" });
        self.emit_snapshot();
        self.get_snapshot()
    }

    /// T1112/R31: Updates SMTC display/privacy mode (`"full"` | `"privacy"` | `"disabled"`).
    pub fn set_smtc_mode(&self, mode: &str) -> AppResult<PlaybackSnapshot> {
        if !matches!(mode, "full" | "privacy" | "disabled") {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "smtc_mode 必须为 full、privacy 或 disabled",
            ));
        }
        {
            let mut guard = self.inner.lock();
            guard.smtc_mode = mode.to_string();
        }
        self.db.set_setting("smtc_mode", mode)?;
        self.emit_snapshot();
        self.get_snapshot()
    }

    /// T1112/R31: Computes the current Windows SMTC display state, applying privacy masking when `smtc_mode == "privacy"`.
    pub fn get_smtc_display_state(&self) -> AppResult<SmtcDisplayStateDto> {
        let (
            mode,
            state,
            version_id,
            work_id,
            work_title,
            track_title,
            pos_ms,
            dur_ms,
            local_audio_path,
            cached_tags,
            active_cues,
        ) = {
            let guard = self.inner.lock();
            let local_path = guard
                .active_source
                .iter()
                .chain(guard.alternative_sources.iter())
                .filter(|s| {
                    s.source_kind == "local_imported" || s.source_kind == "local_downloaded"
                })
                .filter_map(|s| s.absolute_path.as_ref())
                .find(|p| !p.ends_with(".part") && Path::new(p).is_file())
                .cloned();
            (
                guard.smtc_mode.clone(),
                guard.state.clone(),
                guard.current_version_id.clone(),
                guard.current_work_id.clone(),
                guard.current_work_title.clone(),
                guard.current_track_title.clone(),
                guard.position_ms,
                guard.duration_ms.unwrap_or(0),
                local_path,
                guard.cached_audio_tags.clone(),
                guard.active_subtitle_cues.clone(),
            )
        };

        let has_version = version_id.is_some();

        if mode == "disabled" {
            AmllBroadcastHub::global().clear_on_stop(&self.db.cache_dir);
            return Ok(SmtcDisplayStateDto {
                enabled: false,
                mode,
                playback_status: "Closed".into(),
                title: String::new(),
                artist: String::new(),
                album_title: String::new(),
                album_artist: String::new(),
                cover_path: None,
                cover_bytes: None,
                video_subtitle: None,
                position_ms: 0,
                duration_ms: 0,
                start_time_ms: 0,
                min_seek_time_ms: 0,
                end_time_ms: 0,
                max_seek_time_ms: 0,
                is_play_enabled: false,
                is_pause_enabled: false,
                is_stop_enabled: false,
                is_next_enabled: false,
                is_previous_enabled: false,
                is_playback_position_change_enabled: false,
                clear_all_called: true,
            });
        }

        let playback_status = if !has_version {
            "Stopped"
        } else {
            match state.as_str() {
                "playing" | "loading" | "buffering" | "resolving" => "Playing",
                "paused" => "Paused",
                _ => "Stopped",
            }
        }
        .to_string();

        // Task 2 & Task 4: Preserve exact playback timestamp (never reset Position to 0 on Pause),
        // and call ClearAll when stopped or when no track is loaded, but NEVER when paused.
        let safe_dur = dur_ms.max(0);
        let safe_pos = if safe_dur > 0 {
            pos_ms.clamp(0, safe_dur)
        } else {
            pos_ms.max(0)
        };
        let clear_all_called = !has_version || playback_status == "Stopped";

        if mode == "privacy" {
            AmllBroadcastHub::global().clear_on_stop(&self.db.cache_dir);
            // T1112 Privacy Mask Mode: keep transport controls & timeline active, but mask title, artist, RJ code, and cover!
            return Ok(SmtcDisplayStateDto {
                enabled: true,
                mode,
                playback_status,
                title: "ASMR Audio Track".into(),
                artist: "ASMR Player".into(),
                album_title: String::new(),
                album_artist: "ASMR Player".into(),
                cover_path: None,
                cover_bytes: None,
                video_subtitle: None,
                position_ms: safe_pos,
                duration_ms: safe_dur,
                start_time_ms: 0,
                min_seek_time_ms: 0,
                end_time_ms: safe_dur,
                max_seek_time_ms: safe_dur,
                is_play_enabled: true,
                is_pause_enabled: true,
                is_stop_enabled: true,
                is_next_enabled: true,
                is_previous_enabled: true,
                is_playback_position_change_enabled: true,
                clear_all_called,
            });
        }

        // Probe and cache ID3 / FLAC tags & embedded APIC/Picture once per version_id when playing local files
        let tag_meta = match version_id.as_deref() {
            Some(vid) => {
                if cached_tags.as_ref().map(|c| c.version_id.as_str()) == Some(vid) {
                    cached_tags
                } else if let Some(ref audio_path) = local_audio_path {
                    let probed = probe_local_audio_tags(vid, Path::new(audio_path));
                    self.inner.lock().cached_audio_tags = Some(probed.clone());
                    Some(probed)
                } else {
                    None
                }
            }
            None => None,
        };

        // Full display mode: enrich with circle / voice actors / cover from DB if work_id is active,
        // and fall back to audio file ID3 / FLAC tags (artist, album, album_artist, embedded APIC cover).
        let mut artist = work_title.clone().unwrap_or_else(|| "ASMR Player".into());
        let mut album_title = work_title.clone().unwrap_or_default();
        let mut album_artist = String::new();
        let mut cover_path: Option<String> = None;
        let mut has_explicit_artist = false;

        if let Some(ref wid) = work_id {
            if let Ok((w_title, rj_code, circle_name, voice_actors, raw_cover_path, _src_wid)) =
                self.db.get_work_smtc_metadata(wid)
            {
                if !voice_actors.is_empty() {
                    artist = voice_actors.join(" / ");
                    has_explicit_artist = true;
                } else if !circle_name.is_empty() {
                    artist = circle_name.clone();
                    has_explicit_artist = true;
                }
                if !circle_name.is_empty() {
                    album_artist = circle_name;
                }
                album_title = match rj_code {
                    Some(ref rj) if !rj.is_empty() => format!("[{}] {}", rj, w_title),
                    _ => w_title,
                };
                cover_path = raw_cover_path;
            }
        }

        if let Some(ref meta) = tag_meta {
            if !has_explicit_artist {
                if let Some(ref tag_artist) = meta.artist {
                    artist = tag_artist.clone();
                }
            }
            if album_artist.is_empty() {
                if let Some(ref tag_aa) = meta.album_artist {
                    album_artist = tag_aa.clone();
                }
            }
            if album_title.is_empty() {
                if let Some(ref tag_album) = meta.album {
                    album_title = tag_album.clone();
                }
            }
        }
        if album_artist.is_empty() {
            album_artist = artist.clone();
        }

        let cover_bytes = tag_meta
            .as_ref()
            .and_then(|m| m.embedded_cover_bytes.clone());

        // Section I.1 & II.1: Strip redundant local file extensions (.mp3, .flac, .wav, etc.)
        // so external SMTC hosts (such as WinIsland) can accurately match LRC lyrics by song title.
        let clean_title = track_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(sanitize_smtc_track_title)
            .or_else(|| {
                tag_meta
                    .as_ref()
                    .and_then(|m| m.title.as_deref())
                    .map(sanitize_smtc_track_title)
            })
            .unwrap_or_else(|| "未选择音轨".into());

        let video_subtitle = crate::player::amll::find_active_cue_at(&active_cues, safe_pos);
        if clear_all_called {
            AmllBroadcastHub::global().clear_on_stop(&self.db.cache_dir);
        } else {
            let winisland_cfg_updated = AmllBroadcastHub::global().update_track_and_subtitles(
                &self.db.cache_dir,
                version_id,
                &clean_title,
                &artist,
                &album_title,
                cover_path.clone(),
                safe_dur,
                safe_pos,
                playback_status == "Playing",
                active_cues,
            );
            if winisland_cfg_updated {
                let svc = self.clone();
                spawn_bg_task(async move {
                    tokio::time::sleep(Duration::from_millis(75)).await;
                    if let Ok(smtc) = svc.get_smtc_display_state() {
                        if let Some(ref ctrl) = *svc.native_smtc.lock() {
                            ctrl.pulse_title_for_lyrics_reload(&smtc);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(85)).await;
                    svc.emit_snapshot();
                });
            }
        }

        Ok(SmtcDisplayStateDto {
            enabled: true,
            mode,
            playback_status,
            title: clean_title,
            artist,
            album_title,
            album_artist,
            cover_path,
            cover_bytes,
            video_subtitle,
            position_ms: safe_pos,
            duration_ms: safe_dur,
            start_time_ms: 0,
            min_seek_time_ms: 0,
            end_time_ms: safe_dur,
            max_seek_time_ms: safe_dur,
            is_play_enabled: true,
            is_pause_enabled: true,
            is_stop_enabled: true,
            is_next_enabled: true,
            is_previous_enabled: true,
            is_playback_position_change_enabled: true,
            clear_all_called,
        })
    }

    /// T1504: replace queue membership without interrupting the active playback session.
    pub fn apply_queue_plan(&self, request: &ApplyQueuePlanRequest) -> AppResult<PlaybackSnapshot> {
        let prepared = self.db.prepare_queue_plan(&request.plan_id)?;
        {
            let mut guard = self.inner.lock();
            let current_id = self.db.commit_queue_plan(request, prepared, guard.current_version_id.as_deref())?;
            if request.mode == "replace" {
                guard.current_queue_item_id = current_id;
            }
            guard.prefetched_next = None;
            guard.prefetching_version_id = None;
        }
        self.emit_snapshot();
        self.get_snapshot()
    }

    /// Clears queue pointer and triggers `DisplayUpdater.ClearAll()` if no track is actively playing/paused.
    pub fn on_queue_cleared(&self) {
        {
            let mut guard = self.inner.lock();
            guard.current_queue_item_id = None;
            if !matches!(
                guard.state.as_str(),
                "playing" | "paused" | "loading" | "buffering" | "resolving"
            ) {
                guard.current_version_id = None;
                guard.current_track_id = None;
                guard.current_track_title = None;
                guard.current_work_id = None;
                guard.current_work_title = None;
                guard.active_source = None;
                guard.cached_audio_tags = None;
                guard.active_subtitle_cues.clear();
                guard.state = "stopped".into();
            }
        }
        self.emit_snapshot();
    }

    /// T1112/R31: Handles incoming hardware media key / Bluetooth headset / Windows SMTC transport commands.
    pub async fn handle_smtc_action(
        &self,
        action: &str,
        seek_ms: Option<i64>,
    ) -> AppResult<PlaybackSnapshot> {
        let is_disabled = self.inner.lock().smtc_mode == "disabled";
        if is_disabled {
            return self.get_snapshot();
        }
        match action {
            "play" => {
                let state = self.inner.lock().state.clone();
                if state != "playing" {
                    self.toggle_pause().await
                } else {
                    self.get_snapshot()
                }
            }
            "pause" => {
                let state = self.inner.lock().state.clone();
                if state == "playing"
                    || state == "buffering"
                    || state == "loading"
                    || state == "resolving"
                {
                    self.toggle_pause().await
                } else {
                    self.get_snapshot()
                }
            }
            "toggle_pause" => self.toggle_pause().await,
            "next" => self.step_queue(1).await,
            "previous" => self.step_queue(-1).await,
            "fast_forward" => {
                let cur = self.inner.lock().position_ms;
                self.seek_ms(cur.saturating_add(10_000)).await
            }
            "rewind" => {
                let cur = self.inner.lock().position_ms;
                self.seek_ms(cur.saturating_sub(10_000).max(0)).await
            }
            "stop" => {
                let _ = self.mpv.stop().await;
                {
                    let mut guard = self.inner.lock();
                    guard.user_paused_intent = true;
                    guard.state = "stopped".into();
                }
                self.persist_current_state(false);
                self.emit_snapshot();
                self.get_snapshot()
            }
            "seek" => {
                let target = seek_ms.unwrap_or(0).max(0);
                self.seek_ms(target).await
            }
            _ => Err(AppError::new(
                ErrorCode::InvalidInput,
                format!("未知的 SMTC 动作: {}", action),
            )),
        }
    }

    /// T1001: Self-healing engine restart that recycles `mpv`, restores `volume` & `dsp_config` (`af` filter graph),
    /// and preserves `current_version_id` and `position_ms` (`pending_seek_ms`) in paused state.
    pub async fn restart_engine(&self) -> AppResult<PlaybackSnapshot> {
        let _load = self.load_operation.lock().await;
        self.restart_engine_locked(true).await
    }

    pub fn engine_maintenance_settings(&self) -> EngineMaintenanceSettings {
        self.inner.lock().maintenance.settings.clone()
    }

    pub fn set_engine_maintenance_settings(&self, settings: EngineMaintenanceSettings) -> AppResult<EngineMaintenanceSettings> {
        settings.validate()?;
        let mut state = self.inner.lock();
        self.db.set_setting(super::engine_maintenance::SETTINGS_KEY, &serde_json::to_string(&settings)
            .map_err(|e| AppError::new(ErrorCode::InternalError, e.to_string()))?)?;
        state.maintenance = MaintenanceClock::new(settings.clone());
        Ok(settings)
    }

    async fn maybe_maintain_engine(&self, gap_generation: Option<u64>) -> AppResult<Option<u64>> {
        let due = || {
            let mut state = self.inner.lock();
            if gap_generation.is_some_and(|generation| state.session_generation != generation || !state.current_completed) {
                return false;
            }
            let inactive = matches!(state.state.as_str(), "idle" | "paused" | "stopped" | "ended" | "error");
            state.maintenance.due(Instant::now(), inactive, gap_generation.is_some())
        };
        if !due() || *self.shutdown_signal.borrow() { return Ok(None); }
        let _load = self.load_operation.lock().await;
        if !due() || *self.shutdown_signal.borrow() { return Ok(None); }
        self.db.append_log("INFO", if gap_generation.is_some() { "Scheduled engine restart: completed-track interval" } else { "Scheduled engine restart: idle interval" });
        self.restart_engine_locked(gap_generation.is_none()).await.map(|snapshot| Some(snapshot.session_generation))
    }

    // Caller owns load_operation. Scheduled maintenance rechecks its gate under it.
    async fn restart_engine_locked(&self, reload_current: bool) -> AppResult<PlaybackSnapshot> {
        let (generation, source, volume, af_graph) = {
            let mut guard = self.inner.lock();
            // Failed attempts also wait a full interval; never retry each timer tick.
            guard.maintenance.restarted(Instant::now());
            guard.session_generation += 1;
            guard.state = "paused".into();
            guard.user_paused_intent = true;
            guard.file_loaded_for_gen = false;
            guard.pending_seek_ms = (guard.position_ms > 0).then_some(guard.position_ms);
            guard.mpv_buffering = false;
            guard.buffering_since = None;
            guard.online_recovery_running = false;
            guard.prefetched_next = None;
            guard.prefetched_at = None;
            guard.last_prefetch_check = None;
            (guard.session_generation, guard.active_source.clone(), guard.volume,
                build_lavfi_graph(&guard.dsp_config))
        };
        // Preserve progress before tearing down the old native context.
        self.persist_current_state(false);
        self.mpv.shutdown().await;
        if self.inner.lock().session_generation != generation { return self.get_snapshot(); }
        self.mpv.set_initial_af_graph(af_graph.clone());
        let result = async {
            // Online/empty sessions must also create and validate a new engine.
            self.mpv.ensure_started().await?;
            self.mpv.set_audio_filters(&af_graph).await?;
            self.mpv.set_volume(volume).await?;
            if self.inner.lock().session_generation != generation { return Ok(()); }
            if let Some(source) = source.as_ref().filter(|s| reload_current && !self.inner.lock().current_completed && matches!(s.source_kind.as_str(), "local_imported" | "local_downloaded")) {
                let path = source.absolute_path.as_deref().filter(|p| !p.ends_with(".part") && Path::new(p).is_file())
                    .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "重启后无法加载原音源：本地文件不存在或未完成下载"))?;
                self.mpv.load_file_pipelined(path, &[], true, volume, &af_graph).await?;
                // FileLoaded performs the pending seek. Seeking before it can be lost.
            }
            Ok::<_, AppError>(())
        }.await;
        {
            let mut guard = self.inner.lock();
            if guard.session_generation != generation { return drop_and_snap(self, guard); }
            match &result {
                Ok(()) => guard.last_error = None,
                Err(error) => {
                    guard.state = "error".into();
                    guard.last_error = Some(error.clone());
                }
            }
        }
        if let Err(error) = &result {
            self.db.append_log("WARN", &format!("Engine restart failed: generation={generation}, error={error}"));
        }
        self.persist_current_state(false);
        self.emit_snapshot();
        result?;
        self.get_snapshot()
    }

    /// T1002: Immediately pauses playback and records a protective notice when headphones or audio output devices disconnect.
    pub async fn handle_audio_device_disconnected(&self, reason: &str) -> AppResult<PlaybackSnapshot> {
        let should_pause_mpv = {
            let mut guard = self.inner.lock();
            let was_active = guard.state == "playing"
                || guard.state == "loading"
                || guard.state == "buffering";
            guard.user_paused_intent = true;
            if was_active {
                guard.state = "paused".into();
            }
            guard.last_error = Some(
                AppError::new(ErrorCode::MpvIpcError, reason.to_string())
                    .with_hint("已为您自动暂停播放以防外放，重新连接耳机后点击播放即可继续收听"),
            );
            was_active || guard.file_loaded_for_gen
        };
        if should_pause_mpv {
            let _ = self.mpv.set_pause(true).await;
        }
        self.db.append_log("WARN", &format!("Audio output device disconnected: {}", reason));
        self.persist_current_state(false);
        self.emit_snapshot();
        self.get_snapshot()
    }

    pub fn get_dsp_state(&self, work_id: Option<String>) -> AppResult<DspStateDto> {
        let presets = self.db.list_dsp_presets()?;
        // T1004: Extract inner fields and release `self.inner.lock()` BEFORE calling `self.db`
        let (cur_wid, base_cfg, base_preset, mut telemetry) = {
            let guard = self.inner.lock();
            (
                guard.current_work_id.clone(),
                guard.dsp_config.clone(),
                guard.active_dsp_preset_id.clone(),
                guard.audio_telemetry.clone(),
            )
        };
        let target_work_id = work_id.or(cur_wid);
        let (work_override_enabled, effective_cfg, effective_preset) =
            if let Some(ref wid) = target_work_id {
                if let Ok(Some((w_preset, w_cfg, true))) = self.db.get_work_dsp_override(wid) {
                    (true, w_cfg, w_preset)
                } else {
                    (false, base_cfg, base_preset)
                }
            } else {
                (false, base_cfg, base_preset)
            };
        telemetry.active_dsp_summary = summarize_dsp_chain(&effective_cfg);
        telemetry.active_filter_graph = build_lavfi_graph(&effective_cfg);
        Ok(DspStateDto {
            config: effective_cfg,
            active_preset_id: effective_preset,
            work_override_enabled,
            work_override_work_id: target_work_id,
            presets,
            telemetry,
        })
    }

    /// Refresh restored settings without writing them back into a different scope.
    pub async fn reload_restored_dsp(&self) -> AppResult<()> {
        let graph = {
            let mut guard = self.inner.lock();
            let (mut config, preset) = match guard.current_work_id.as_deref()
                .map(|id| self.db.get_work_dsp_override(id)).transpose()?.flatten() {
                Some((preset, config, true)) => (config, preset),
                _ => self.db.get_dsp_global_state()?,
            };
            validate_dsp_config(&mut config)?;
            let graph = build_lavfi_graph(&config);
            guard.audio_telemetry.active_dsp_summary = summarize_dsp_chain(&config);
            guard.audio_telemetry.active_filter_graph = graph.clone();
            guard.dsp_config = config;
            guard.active_dsp_preset_id = preset;
            graph
        };
        self.mpv.set_initial_af_graph(graph.clone());
        self.mpv.set_audio_filters(&graph).await?;
        self.emit_snapshot();
        Ok(())
    }

    pub async fn update_dsp_config(
        &self,
        mut config: DspConfigDto,
        preset_id: Option<String>,
    ) -> AppResult<DspStateDto> {
        validate_dsp_config(&mut config)?;
        let af_graph = build_lavfi_graph(&config);
        let summary = summarize_dsp_chain(&config);
        // Publish/persist the requested state only after the engine accepts it.
        self.mpv.set_audio_filters(&af_graph).await?;

        let current_work_id = {
            let mut guard = self.inner.lock();
            guard.dsp_config = config.clone();
            guard.active_dsp_preset_id = preset_id.clone();
            guard.audio_telemetry.active_dsp_summary = summary;
            guard.audio_telemetry.active_filter_graph = af_graph.clone();
            guard.current_work_id.clone()
        };

        if let Some(ref wid) = current_work_id {
            if let Ok(Some((_, _, true))) = self.db.get_work_dsp_override(wid) {
                let _ = self.db.set_work_dsp_override(
                    wid,
                    preset_id.as_deref(),
                    &config,
                    true,
                );
            } else {
                let _ = self
                    .db
                    .save_dsp_global_state(&config, preset_id.as_deref());
            }
        } else {
            let _ = self
                .db
                .save_dsp_global_state(&config, preset_id.as_deref());
        }

        self.emit_snapshot();
        self.get_dsp_state(current_work_id)
    }

    pub async fn set_loudness_normalization(&self, enabled: bool) -> AppResult<PlaybackSnapshot> {
        let mut config = self.inner.lock().dsp_config.clone();
        config.loudness_normalization = enabled;
        if enabled {
            config.master_bypass = false;
        }
        self.update_dsp_config(config, None).await?;
        self.get_snapshot()
    }

    pub async fn apply_dsp_preset(&self, preset_id: &str) -> AppResult<DspStateDto> {
        let presets = self.db.list_dsp_presets()?;
        let found = presets
            .into_iter()
            .find(|p| p.id == preset_id)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::InvalidInput,
                    format!("未找到指定的音效预设: {}", preset_id),
                )
            })?;
        self.update_dsp_config(found.config, Some(found.id)).await
    }

    pub async fn import_autoeq_text(&self, text: &str) -> AppResult<DspStateDto> {
        let (parsed_preamp, bands) = parse_autoeq_text(text)?;
        let current_cfg = {
            let guard = self.inner.lock();
            let mut cfg = guard.dsp_config.clone();
            cfg.master_bypass = false;
            cfg.autoeq_enabled = true;
            cfg.autoeq_preamp_db = parsed_preamp;
            cfg.autoeq_bands = bands;
            cfg
        };
        self.update_dsp_config(current_cfg, None).await
    }

    pub async fn clear_autoeq_bands(&self) -> AppResult<DspStateDto> {
        let current_cfg = {
            let guard = self.inner.lock();
            let mut cfg = guard.dsp_config.clone();
            cfg.autoeq_enabled = false;
            cfg.autoeq_name = None;
            cfg.autoeq_bands.clear();
            cfg.autoeq_preamp_db = 0.0;
            cfg
        };
        self.update_dsp_config(current_cfg, None).await
    }

    pub async fn save_custom_dsp_preset(
        &self,
        name: &str,
        description: &str,
        config: DspConfigDto,
    ) -> AppResult<DspStateDto> {
        let saved = self
            .db
            .save_custom_dsp_preset(None, name, description, &config)?;
        self.update_dsp_config(saved.config, Some(saved.id)).await
    }

    pub async fn delete_custom_dsp_preset(&self, preset_id: &str) -> AppResult<DspStateDto> {
        self.db.delete_custom_dsp_preset(preset_id)?;
        let current_work_id = {
            let mut guard = self.inner.lock();
            if guard.active_dsp_preset_id.as_deref() == Some(preset_id) {
                guard.active_dsp_preset_id = None;
            }
            guard.current_work_id.clone()
        };
        self.emit_snapshot();
        self.get_dsp_state(current_work_id)
    }

    pub async fn set_work_dsp_override_enabled(
        &self,
        work_id: &str,
        enabled: bool,
    ) -> AppResult<DspStateDto> {
        if enabled {
            let (cfg, preset_id) = {
                let guard = self.inner.lock();
                (guard.dsp_config.clone(), guard.active_dsp_preset_id.clone())
            };
            self.db
                .set_work_dsp_override(work_id, preset_id.as_deref(), &cfg, true)?;
        } else {
            self.db.clear_work_dsp_override(work_id)?;
            let is_current_work = {
                let guard = self.inner.lock();
                guard.current_work_id.as_deref() == Some(work_id)
            };
            if is_current_work {
                let (global_cfg, global_preset) = self
                    .db
                    .get_dsp_global_state()
                    .unwrap_or_else(|_| (DspConfigDto::default(), Some("asmr_pure_guard".into())));
                return self.update_dsp_config(global_cfg, global_preset).await;
            }
        }
        self.emit_snapshot();
        self.get_dsp_state(Some(work_id.to_string()))
    }

    /// T1404: invalidate in-flight loads, pause, and durably save before installing.
    /// Called only after the user explicitly chooses installation of a verified package.
    pub async fn prepare_for_update(&self) -> AppResult<()> {
        {
            let mut guard = self.inner.lock();
            guard.session_generation += 1;
            guard.user_paused_intent = true;
            guard.state = "paused".into();
            guard.file_loaded_for_gen = false;
        }
        let _ = self.mpv.set_pause(true).await;
        self.try_persist_current_state(false)?;
        self.db.wal_checkpoint_truncate()?;
        self.mpv.shutdown().await;
        // If launching the installer fails, Play reloads the saved version and position.
        self.inner.lock().active_source = None;
        self.emit_snapshot();
        Ok(())
    }

    pub async fn shutdown(&self) {
        // Stop self-owning workers and revoke native callbacks before releasing libmpv.
        self.shutdown_signal.send_replace(true);
        let native_smtc = self.native_smtc.lock().take();
        drop(native_smtc);
        self.app_handle.lock().take();
        self.persist_current_state(false);
        self.mpv.shutdown().await;
        // T1003: Checkpoint and truncate WAL file on clean shutdown
        let _ = self.db.wal_checkpoint_truncate();
    }

    pub async fn stop_if_playing_work(&self, work_id: &str) {
        let should_stop = {
            let mut guard = self.inner.lock();
            if guard.current_work_id.as_deref() == Some(work_id) {
                guard.session_generation += 1;
                guard.state = "idle".into();
                guard.user_paused_intent = true;
                guard.current_queue_item_id = None;
                guard.current_work_id = None;
                guard.current_work_title = None;
                guard.current_track_id = None;
                guard.current_track_title = None;
                guard.current_version_id = None;
                guard.active_source = None;
                guard.alternative_sources.clear();
                guard.position_ms = 0;
                guard.duration_ms = None;
                guard.pending_seek_ms = None;
                guard.file_loaded_for_gen = false;
                guard.active_ab_loop = None;
                guard.active_subtitle_cues.clear();
                guard.last_error = None;
                true
            } else {
                false
            }
        };
        if should_stop {
            self.mpv.shutdown().await;
            self.persist_current_state(false);
        }
        self.emit_snapshot();
    }

    /// Observe host buffering state rather than guessing progress from frontend timers.
    async fn check_online_buffering(&self, now: Instant) {
        let generation = {
            let mut state = self.inner.lock();
            let online = state.active_source.as_ref().is_some_and(|s| s.source_kind == "online_provider");
            if !online || state.user_paused_intent
                || !matches!(state.state.as_str(), "loading" | "playing" | "buffering")
                || (!state.mpv_buffering && state.file_loaded_for_gen) {
                state.buffering_since = None;
                return;
            }
            let since = *state.buffering_since.get_or_insert(now);
            if now.saturating_duration_since(since) < Duration::from_secs(30) { return; }
            state.buffering_since = None;
            state.session_generation
        };
        self.recover_online_stream(generation, true).await;
    }

    async fn recover_online_stream(&self, generation: u64, stalled: bool) {
        // A new load can emit end-file before its command future returns. Queue
        // that failure behind the active recovery instead of dropping it while
        // online_recovery_running is true. Recheck the session after waiting.
        let _recovery = self.recovery_operation.lock().await;
        if *self.shutdown_signal.borrow() { return; }
        let (source, exhausted) = {
            let mut state = self.inner.lock();
            if state.session_generation != generation
                || matches!(state.state.as_str(), "error" | "idle" | "stopped" | "ended") { return; }
            let Some(source) = state.active_source.clone().filter(|s| s.source_kind == "online_provider") else { return; };
            let exhausted = state.retried_online_for_gen;
            state.retried_online_for_gen = true;
            state.online_recovery_running = true;
            (source, exhausted)
        };
        let resolved = if exhausted {
            Err(AppError::new(ErrorCode::NetworkUnavailable, if stalled {
                "在线音频持续缓冲，重新连接后仍未恢复。请检查网络或切换线路后点击播放重试。"
            } else {
                "在线音频加载失败，刷新地址后仍无法播放。请检查网络或切换线路后点击播放重试。"
            }))
        } else {
            self.db.append_log("WARN", &format!("Online stream recovery: generation={generation}, stalled={stalled}; refreshing source once"));
            self.provider.resolve_media(&ProviderMediaRef {
                provider: source.provider.clone().unwrap_or_else(|| "asmr_one".into()),
                source_work_id: source.source_work_id.clone().unwrap_or_default(),
                remote_file_id: source.remote_file_id.clone().unwrap_or_default(),
                remote_relative_path: source.remote_relative_path.clone().unwrap_or_default(),
            }, MediaPurpose::Play, true).await
        };
        let _load = self.load_operation.lock().await;
        if *self.shutdown_signal.borrow() { return; }
        let (volume, graph) = {
            let mut state = self.inner.lock();
            if state.session_generation != generation
                || state.active_source.as_ref().map(|s| &s.id) != Some(&source.id)
                || matches!(state.state.as_str(), "stopped" | "idle" | "ended") {
                if state.session_generation == generation { state.online_recovery_running = false; }
                return;
            }
            // A slow refresh must not interrupt a stream that recovered on its own.
            if stalled && !state.mpv_buffering && state.file_loaded_for_gen {
                state.online_recovery_running = false;
                state.retried_online_for_gen = exhausted;
                return;
            }
            state.file_loaded_for_gen = false;
            state.pending_seek_ms = Some(state.position_ms.max(0));
            state.mpv_buffering = false;
            state.buffering_since = Some(Instant::now());
            state.state = "loading".into();
            (state.volume, build_lavfi_graph(&state.dsp_config))
        };
        let result = match resolved {
            // Load paused; FileLoaded seeks first and then applies the latest user intent.
            Ok(media) => self.mpv.load_file_pipelined(&media.url, &media.headers, true, volume, &graph).await,
            Err(error) => Err(error),
        };
        {
            let mut state = self.inner.lock();
            if state.session_generation != generation { return; }
            state.online_recovery_running = false;
        }
        if let Err(error) = result {
            {
                let mut state = self.inner.lock();
                if state.session_generation != generation { return; }
                state.state = "error".into();
                state.user_paused_intent = true;
                state.buffering_since = None;
                state.last_error = Some(error);
            }
            self.db.append_log("WARN", &format!("Online stream recovery failed: generation={generation}; manual retry available"));
            self.persist_current_state(false);
            self.emit_snapshot();
            let _ = self.mpv.stop().await;
        } else {
            self.emit_snapshot();
        }
    }

    fn spawn_buffering_monitor(&self) {
        let this = self.clone();
        let mut shutdown = self.shutdown_signal.subscribe();
        spawn_bg_task(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => break,
                    _ = interval.tick() => {
                        tokio::select! {
                            biased;
                            _ = shutdown.changed() => break,
                            _ = this.check_online_buffering(Instant::now()) => {},
                        }
                    }
                }
            }
        });
    }

    fn spawn_sleep_timer_loop(&self) {
        let this = self.clone();
        let mut shutdown = self.shutdown_signal.subscribe();
        spawn_bg_task(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tokio::select! {
                biased;
                _ = shutdown.changed() => {},
                _ = async {
                    while !*this.shutdown_signal.borrow() {
                        interval.tick().await;
                        let _ = this.check_and_fire_sleep_timer().await;
                        let _ = this.maybe_maintain_engine(None).await;
                    }
                } => {}
            }
        });
    }

    fn spawn_event_loop(&self, mut event_rx: mpsc::UnboundedReceiver<MpvEvent>) {
        let this = self.clone();
        let mut shutdown = self.shutdown_signal.subscribe();
        spawn_bg_task(async move {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {},
                _ = async {
            while let Some(ev) = event_rx.recv().await {
                if *this.shutdown_signal.borrow() { break; }
                match ev {
                    MpvEvent::FileLoaded { playlist_entry_id } => {
                        let _load = this.load_operation.lock().await;
                        let pending_seek = {
                            let mut guard = this.inner.lock();
                            if matches!(guard.state.as_str(), "error" | "stopped" | "idle") { continue; }
                            guard.file_loaded_for_gen = true;
                            guard.state = if guard.user_paused_intent {
                                "paused".into()
                            } else if guard.mpv_buffering {
                                "buffering".into()
                            } else {
                                "playing".into()
                            };
                            guard.pending_seek_ms
                        };
                        if let Some(seek_target) = pending_seek {
                            if seek_target > 0 {
                                let _ = this.mpv.seek_absolute_ms(seek_target).await;
                            }
                            let mut guard = this.inner.lock();
                            if guard.pending_seek_ms == Some(seek_target) {
                                guard.pending_seek_ms = None;
                            }
                        }
                        let paused = this.inner.lock().user_paused_intent;
                        let _ = this.mpv.set_pause(paused).await;
                        let source_context = {
                            let state = this.inner.lock();
                            state.active_source.as_ref().map(|source| (state.session_generation, source.id.clone()))
                        };
                        if let Some((generation, source_id)) = source_context {
                            if let Some((rate, bitrate)) = this.mpv.source_audio_rates(playlist_entry_id).await {
                                let mut state = this.inner.lock();
                                if state.session_generation == generation && state.active_source.as_ref().map(|s| s.id.as_str()) == Some(source_id.as_str()) {
                                    state.audio_telemetry.source_sample_rate_hz = rate;
                                    state.audio_telemetry.source_bitrate_kbps = bitrate;
                                }
                            }
                        }
                        let transition={let mut state=this.inner.lock();
                            if state.fade_in_pending&&!state.user_paused_intent {state.fade_in_pending=false;Some((state.session_generation,state.transition_serial,state.immersion.fade_duration_ms))}else{None}};
                        if let Some((generation,serial,ms))=transition {let player=this.clone();spawn_bg_task(async move {let _=player.ramp_transport(generation,serial,1.0,ms).await;});}
                        this.persist_current_state(false);
                        this.sync_native_smtc_timeline();
                        this.emit_snapshot();
                    }
                    MpvEvent::DurationChanged(dur_secs) => {
                        let dur_ms = (dur_secs * 1000.0).round() as i64;
                        let (vid_opt, pending_seek) = {
                            let mut guard = this.inner.lock();
                            if dur_ms > 0 {
                                guard.duration_ms = Some(dur_ms);
                            }
                            let seek_opt = if guard.file_loaded_for_gen {
                                guard.pending_seek_ms
                            } else {
                                None
                            };
                            (guard.current_version_id.clone(), seek_opt)
                        };
                        if let Some(vid) = vid_opt {
                            if dur_ms > 0 {
                                let _ = this.db.with_conn(|conn| {
                                    conn.execute(
                                        "UPDATE track_versions SET duration_ms = ?1 WHERE id = ?2",
                                        params![dur_ms, vid],
                                    )?;
                                    Ok(())
                                });
                            }
                        }
                        if let Some(seek_target) = pending_seek {
                            if seek_target > 0 {
                                let _ = this.mpv.seek_absolute_ms(seek_target).await;
                            }
                            let mut guard = this.inner.lock();
                            if guard.pending_seek_ms == Some(seek_target) {
                                guard.pending_seek_ms = None;
                            }
                        }
                        this.sync_native_smtc_timeline();
                        this.emit_snapshot();
                    }
                    MpvEvent::TimePosChanged(pos_secs) => {
                        let pos_ms = (pos_secs * 1000.0).round() as i64;
                        let (
                            should_save,
                            should_sync_smtc,
                            ab_rewind_to,
                            should_prefetch,
                            eot_fade_vol,
                            dur_ms,
                            is_playing,
                        ) = {
                            let mut guard = this.inner.lock();
                            if !guard.file_loaded_for_gen {
                                continue;
                            }
                            // Task 4 Guardrail: Never allow a transient 0 position before pending seek finishes
                            // or while paused to reset a non-zero playback position to 0.
                            if pos_ms <= 0
                                && (guard.pending_seek_ms.unwrap_or(0) > 0
                                    || (guard.state == "paused" && guard.position_ms > 0))
                            {
                                continue;
                            }
                            guard.position_ms = pos_ms.max(0);
                            let rewind = guard.active_ab_loop.as_ref().and_then(|ab| {
                                if pos_ms >= ab.end_ms {
                                    Some(ab.start_ms)
                                } else {
                                    None
                                }
                            });
                            let save = guard.last_persisted_at.elapsed() >= Duration::from_secs(5);
                            let sync_smtc =
                                guard.last_smtc_timeline_sync_at.elapsed() >= Duration::from_secs(1);
                            let prefetch = guard.schedule_prefetch(Instant::now(), pos_ms);
                            let eot_fade = if guard.sleep_timer.mode == "end_of_track"
                                && guard.sleep_timer.fade_out_seconds > 0
                            {
                                guard.duration_ms.and_then(|dur| {
                                    let rem_secs = ((dur - pos_ms).max(0)) / 1000;
                                    if dur > 0 && rem_secs <= guard.sleep_timer.fade_out_seconds {
                                        let mult = compute_sleep_fade_multiplier(
                                            rem_secs,
                                            guard.sleep_timer.fade_out_seconds,
                                        );
                                        Some(guard.volume * mult)
                                    } else {
                                        None
                                    }
                                })
                            } else {
                                None
                            };
                            (
                                save,
                                sync_smtc,
                                rewind,
                                prefetch,
                                eot_fade,
                                guard.duration_ms.unwrap_or(0),
                                guard.state == "playing",
                            )
                        };

                        let _ = AmllBroadcastHub::global().update_progress(
                            pos_ms.max(0),
                            dur_ms,
                            is_playing,
                        );

                        if let Some(attenuated_vol) = eot_fade_vol {
                            let _ = this.mpv.set_volume(attenuated_vol).await;
                        }
                        if let Some(start_ms) = ab_rewind_to {
                            let _ = this.mpv.seek_absolute_ms(start_ms).await;
                        }
                        if should_save {
                            this.persist_current_state(false);
                        }
                        if should_sync_smtc { this.sync_native_smtc_timeline(); }
                        // Saving progress does not change metadata or the queue. Avoid
                        // rebuilding cover/subtitle/SMTC payloads every five seconds.
                        this.emit_tick();
                        if should_prefetch {
                            let svc_clone = this.clone();
                            spawn_bg_task(async move {
                                svc_clone.maybe_prefetch_next_track().await;
                            });
                        }
                    }
                    MpvEvent::PauseChanged(paused) => {
                        {
                            let mut guard = this.inner.lock();
                            if guard.file_loaded_for_gen && paused == guard.user_paused_intent {
                                guard.state = if paused {
                                    "paused".into()
                                } else if guard.mpv_buffering {
                                    "buffering".into()
                                } else {
                                    "playing".into()
                                };
                            }
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::BufferingChanged(buffering) => {
                        {
                            let mut guard = this.inner.lock();
                            guard.mpv_buffering = buffering;
                            if !buffering { guard.buffering_since = None; }
                            if guard.file_loaded_for_gen {
                                if buffering && !guard.user_paused_intent {
                                    guard.state = "buffering".into();
                                } else {
                                    // Buffering must never override user's pause intent (README Section 7)
                                    guard.state = if guard.user_paused_intent {
                                        "paused".into()
                                    } else {
                                        "playing".into()
                                    };
                                }
                            }
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioCodecChanged(codec) => {
                        {
                            let mut guard = this.inner.lock();
                            if codec.as_ref().is_some_and(|value| !value.is_empty()) { guard.audio_telemetry.codec = codec; }
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioSampleRateChanged(sr) => {
                        {
                            let mut guard = this.inner.lock();
                            if sr.is_some_and(|value| value > 0) { guard.audio_telemetry.sample_rate_hz = sr; }
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioFormatChanged(fmt) => {
                        {
                            let mut guard = this.inner.lock();
                            if fmt.as_ref().is_some_and(|value| !value.is_empty()) { guard.audio_telemetry.sample_format = fmt; }
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioChannelsChanged(ch) => {
                        {
                            let mut guard = this.inner.lock();
                            if ch.as_ref().is_some_and(|value| !value.is_empty()) { guard.audio_telemetry.channels = ch; }
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioBitrateChanged(bps) => {
                        let changed = {
                            let mut guard = this.inner.lock();
                            let kbps = bps.filter(|v| *v > 0).map(|v| ((v as f64) / 1000.0).round() as i64);
                            let changed = kbps.is_some() && kbps != guard.audio_telemetry.bitrate_kbps;
                            if changed { guard.audio_telemetry.bitrate_kbps = kbps; }
                            changed
                        };
                        if changed { this.emit_snapshot(); }
                    }
                    MpvEvent::AudioDeviceDisconnected(reason) => {
                        let _ = this.handle_audio_device_disconnected(&reason).await;
                    }
                    MpvEvent::EndFile { reason, error, audio_output_error, .. } => {
                        // Ignore 'stop'/'redirect'/'quit' generated during track switches!
                        if reason == "eof" {
                            // A replaced file may reach EOF while the new generation is fading/resolving.
                            if !this.inner.lock().file_loaded_for_gen { continue; }
                            let finished_generation = {
                                let mut state = this.inner.lock();
                                let generation = state.session_generation;
                                if state.file_loaded_for_gen && !state.current_completed { state.maintenance.finished(generation); }
                                generation
                            };
                            let (vid_opt, dur_opt, loop_mode, stop_at_end_of_track, base_vol) = {
                                let mut guard = this.inner.lock();
                                guard.current_completed = true;
                                guard.position_ms = guard.duration_ms.unwrap_or(guard.position_ms);
                                let stop_eot = guard.sleep_timer.mode == "end_of_track";
                                if stop_eot {
                                    guard.sleep_timer.mode = "off".into();
                                    guard.user_paused_intent = true;
                                    guard.state = "paused".into();
                                }
                                (
                                    guard.current_version_id.clone(),
                                    guard.duration_ms,
                                    guard.loop_mode.clone(),
                                    stop_eot,
                                    guard.volume,
                                )
                            };

                            if let Some(ref vid) = vid_opt {
                                let final_pos = dur_opt.unwrap_or(0);
                                let _ = this
                                    .db
                                    .save_playback_position(vid, final_pos, true, true);
                            }

                            let continuation_generation = match this.maybe_maintain_engine(Some(finished_generation)).await {
                                Ok(Some(generation)) => generation,
                                Ok(None) => finished_generation,
                                Err(_) => continue,
                            };
                            if this.inner.lock().session_generation != continuation_generation { continue; }
                            if !stop_at_end_of_track && this.inner.lock().user_paused_intent {
                                this.inner.lock().state="paused".into();
                                let _=this.mpv.set_pause(true).await;
                                this.emit_snapshot();
                                continue;
                            }
                            if stop_at_end_of_track {
                                // R14 & T1101: "本轨结束停止" must NOT advance to the next track, and restores base volume
                                let _ = this.mpv.set_volume(base_vol).await;
                                let now_str = Utc::now().to_rfc3339();
                                let _ = this.db.with_conn(|conn| {
                                    conn.execute(
                                        "UPDATE sleep_timer_state SET mode = 'off', fired_at = ?1, updated_at = ?1 WHERE id = 1",
                                        params![now_str],
                                    )?;
                                    Ok(())
                                });
                                this.emit_snapshot();
                            } else if loop_mode == "single" {
                                if let Some(vid) = vid_opt {
                                    let _ = this.play_with_transition(&vid,None,false,None,None,true).await;
                                }
                            } else {
                                let _ = this.step_queue_transition(1,true).await;
                            }
                        } else if reason == "error" {
                            let online_generation = {
                                let mut guard = this.inner.lock();
                                if let Some(output_error) = audio_output_error {
                                    // Output failures cannot be repaired by refreshing a media URL.
                                    // Leave the current track/position and device selection intact.
                                    guard.state = "error".into();
                                    guard.file_loaded_for_gen = false;
                                    guard.mpv_buffering = false;
                                    guard.buffering_since = None;
                                    this.db.append_log("WARN", &output_error.to_string());
                                    guard.last_error = Some(output_error);
                                    None
                                } else if guard.active_source.as_ref().is_some_and(|s| s.source_kind == "online_provider") {
                                    Some(guard.session_generation)
                                } else {
                                    guard.state = "error".into();
                                    guard.file_loaded_for_gen = false;
                                    guard.mpv_buffering = false;
                                    guard.buffering_since = None;
                                    guard.last_error = Some(AppError::new(ErrorCode::MediaDecodeFailed,
                                        format!("播放失败: {}", error.unwrap_or_else(|| "媒体加载或解码错误".into()))));
                                    this.db.append_log("WARN", &format!("Local playback failed: generation={}, position_ms={}, error={}",
                                        guard.session_generation, guard.position_ms, guard.last_error.as_ref().unwrap()));
                                    None
                                }
                            };
                            if let Some(generation) = online_generation {
                                let recovery = this.clone();
                                spawn_bg_task(async move { recovery.recover_online_stream(generation, false).await; });
                            } else {
                                this.emit_snapshot();
                            }
                        }
                    }
                    MpvEvent::ProcessExited(msg) => {
                        this.persist_current_state(false);
                        let should_auto_heal = {
                            let mut guard = this.inner.lock();
                            // A paused session also loses its file when the engine exits.
                            guard.file_loaded_for_gen = false;
                            guard.mpv_buffering = false;
                            guard.buffering_since = None;
                            if guard.state == "playing"
                                || guard.state == "loading"
                                || guard.state == "buffering"
                            {
                                if !guard.auto_healed_for_gen {
                                    guard.auto_healed_for_gen = true;
                                    true
                                } else {
                                    guard.state = "error".into();
                                    guard.user_paused_intent = true;
                                    guard.file_loaded_for_gen = false;
                                    guard.last_error =
                                        Some(AppError::new(ErrorCode::MpvIpcError, msg));
                                    false
                                }
                            } else {
                                false
                            }
                        };
                        if should_auto_heal {
                            this.db.append_log(
                                "WARN",
                                "mpv exited unexpectedly during active session; running T1001 self-healing recovery",
                            );
                            let _ = this.restart_engine().await;
                        } else {
                            this.emit_snapshot();
                        }
                    }
                }
            }
                } => {}
            }
        });
    }
}

fn probe_local_audio_tags(version_id: &str, path: &Path) -> CachedAudioTagMeta {
    use lofty::file::TaggedFileExt;
    use lofty::probe::Probe;
    use lofty::tag::{Accessor, ItemKey};

    let mut out = CachedAudioTagMeta {
        version_id: version_id.to_string(),
        ..Default::default()
    };

    if let Ok(tagged) = Probe::open(path).and_then(|p| p.read()) {
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            out.title = tag
                .title()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            out.artist = tag
                .artist()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            out.album = tag
                .album()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            out.album_artist = tag
                .get_string(&ItemKey::AlbumArtist)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        for t in tagged.tags() {
            if let Some(pic) = t.pictures().iter().find(|p| !p.data().is_empty()) {
                out.embedded_cover_bytes = Some(pic.data().to_vec());
                break;
            }
        }
    }

    out
}

fn drop_and_snap(
    svc: &PlaybackService,
    guard: parking_lot::MutexGuard<'_, PlaybackStateInner>,
) -> AppResult<PlaybackSnapshot> {
    drop(guard);
    svc.get_snapshot()
}

#[cfg(test)]
mod playback_tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn maintenance_eof_restarts_between_tracks_but_stop_does_not_count() {
        let (_root, pb) = online_stall_fixture();
        pb.set_engine_maintenance_settings(EngineMaintenanceSettings { idle_minutes: None, completed_tracks: Some(1) }).unwrap();
        pb.inner.lock().state = "playing".into();
        let generation = pb.inner.lock().session_generation;
        let (tx, rx) = mpsc::unbounded_channel(); pb.spawn_event_loop(rx);
        tx.send(MpvEvent::EndFile { reason: "stop".into(), error: None, audio_output_error: None, playlist_entry_id: None }).unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(pb.inner.lock().session_generation, generation);
        assert!(!pb.mpv.is_running().await);
        tx.send(MpvEvent::EndFile { reason: "eof".into(), error: None, audio_output_error: None, playlist_entry_id: None }).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let current = pb.inner.lock().session_generation;
                if current != generation && pb.mpv.is_running().await { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        assert_eq!(pb.mpv.command(serde_json::json!(["get_property", "playlist-count"])).await.unwrap(), serde_json::json!(0));
        assert_eq!(pb.inner.lock().state, "paused");
        assert_eq!(pb.inner.lock().current_version_id.as_deref(), Some("v"));
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn maintenance_persists_settings_and_rechecks_active_playback_under_load_lock() {
        let (_root, pb) = online_stall_fixture();
        assert_eq!(pb.engine_maintenance_settings(), EngineMaintenanceSettings::default());
        let settings = EngineMaintenanceSettings { idle_minutes: Some(1), completed_tracks: Some(2) };
        pb.set_engine_maintenance_settings(settings.clone()).unwrap();
        assert_eq!(serde_json::from_str::<EngineMaintenanceSettings>(&pb.db.get_setting(super::super::engine_maintenance::SETTINGS_KEY).unwrap().unwrap()).unwrap(), settings);
        {
            let mut state = pb.inner.lock(); state.state = "paused".into();
            state.maintenance.due(Instant::now() - Duration::from_secs(61), true, false);
        }
        let held = pb.load_operation.lock().await;
        let worker = pb.clone();
        let attempt = tokio::spawn(async move { worker.maybe_maintain_engine(None).await });
        tokio::task::yield_now().await;
        pb.inner.lock().state = "playing".into();
        drop(held);
        assert!(attempt.await.unwrap().unwrap().is_none());
        assert!(!pb.mpv.is_running().await);
        let generation = pb.inner.lock().session_generation;
        {
            let mut state = pb.inner.lock(); state.current_completed = true;
            state.maintenance.finished(generation - 1); state.maintenance.finished(generation);
        }
        assert!(pb.maybe_maintain_engine(Some(generation - 1)).await.unwrap().is_none());
        assert!(pb.maybe_maintain_engine(Some(generation)).await.unwrap().is_some());
        assert!(pb.mpv.is_running().await);
        assert_eq!(pb.inner.lock().position_ms, 1000);
        assert!(!pb.inner.lock().file_loaded_for_gen);
        assert!(pb.maybe_maintain_engine(None).await.unwrap().is_none());
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn maintenance_failed_idle_restart_waits_for_next_interval() {
        let (root, pb) = online_stall_fixture();
        pb.set_engine_maintenance_settings(EngineMaintenanceSettings { idle_minutes: Some(1), completed_tracks: None }).unwrap();
        {
            let mut state = pb.inner.lock(); state.state = "paused".into();
            state.maintenance.due(Instant::now() - Duration::from_secs(61), true, false);
            let source = state.active_source.as_mut().unwrap(); source.source_kind = "local_imported".into();
            source.absolute_path = Some(root.path().join("missing.wav").to_string_lossy().into_owned());
        }
        assert!(pb.maybe_maintain_engine(None).await.is_err());
        let generation = pb.inner.lock().session_generation;
        for _ in 0..100 { assert!(pb.maybe_maintain_engine(None).await.unwrap().is_none()); }
        assert_eq!(pb.inner.lock().session_generation, generation);
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn engine_restart_online_initializes_engine_and_preserves_source_for_resume() {
        let (_root, pb) = online_stall_fixture();
        assert!(!pb.mpv.is_running().await);
        let old_generation = pb.inner.lock().session_generation;
        let snapshot = pb.restart_engine().await.unwrap();
        assert!(pb.mpv.is_running().await);
        assert_eq!(snapshot.state, "paused");
        assert_eq!(snapshot.active_source.unwrap().id, "remote");
        assert_eq!(snapshot.position_ms, 1000);
        assert!(snapshot.session_generation > old_generation);
        assert!(!pb.inner.lock().file_loaded_for_gen);
        assert_eq!(pb.inner.lock().pending_seek_ms, Some(1000));
        assert!(!pb.inner.lock().mpv_buffering);
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn engine_restart_missing_local_source_reports_failure_instead_of_success() {
        let (root, pb) = online_stall_fixture();
        {
            let mut state = pb.inner.lock();
            let source = state.active_source.as_mut().unwrap();
            source.source_kind = "local_imported".into();
            source.absolute_path = Some(root.path().join("missing.wav").to_string_lossy().into_owned());
        }
        let error = pb.restart_engine().await.unwrap_err();
        assert_eq!(error.code, ErrorCode::SourceNotFound);
        let snapshot = pb.get_snapshot().unwrap();
        assert_eq!(snapshot.state, "error");
        assert_eq!(snapshot.last_error.unwrap().code, ErrorCode::SourceNotFound);
        assert_eq!(snapshot.position_ms, 1000);
        assert_eq!(snapshot.current_version_id.as_deref(), Some("v"));
        assert!(pb.db.read_recent_logs(20).iter().any(|line| line.contains("Engine restart failed")));
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn long_runtime_paused_exit_invalidates_loaded_file_and_resume_reloads() {
        let (root, pb) = online_stall_fixture();
        let audio = root.path().join("silence.wav");
        let bytes = 16000u32 * 2 * 4;
        let mut wav = b"RIFF".to_vec();
        wav.extend((36 + bytes).to_le_bytes()); wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes()); wav.extend(1u16.to_le_bytes()); wav.extend(1u16.to_le_bytes());
        wav.extend(16000u32.to_le_bytes()); wav.extend(32000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes()); wav.extend(16u16.to_le_bytes()); wav.extend(b"data");
        wav.extend(bytes.to_le_bytes()); wav.resize(wav.len() + bytes as usize, 0);
        std::fs::write(&audio, wav).unwrap();
        pb.db.with_conn(|conn| {
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES('local','v','local_imported',?1,'wav','available','now','now')", [audio.to_string_lossy()])?;
            Ok(())
        }).unwrap();
        {
            let mut state = pb.inner.lock();
            state.state = "paused".into(); state.user_paused_intent = true;
            state.active_source = pb.list_sources_for_version("v").unwrap().into_iter().next();
        }
        let generation = pb.inner.lock().session_generation;
        let (tx, rx) = mpsc::unbounded_channel(); pb.spawn_event_loop(rx);
        tx.send(MpvEvent::ProcessExited("synthetic idle engine exit".into())).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while pb.inner.lock().file_loaded_for_gen { tokio::task::yield_now().await; }
        }).await.unwrap();
        assert_eq!(pb.inner.lock().state, "paused");
        assert_eq!(pb.inner.lock().position_ms, 1000);
        pb.mpv.command(serde_json::json!(["set_property", "ao", "null"])).await.unwrap();
        pb.toggle_pause().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !pb.inner.lock().file_loaded_for_gen { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.unwrap();
        assert!(pb.inner.lock().session_generation > generation);
        assert_eq!(pb.inner.lock().active_source.as_ref().unwrap().id, "local");
        assert!(pb.inner.lock().position_ms >= 1000);
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn long_runtime_prefetch_is_bounded_and_idle_never_schedules() {
        let (_root, pb) = online_stall_fixture();
        let now = Instant::now();
        {
            let mut state = pb.inner.lock();
            state.state = "playing".into(); state.duration_ms = Some(60_000);
            let count = (0..60_000).filter(|i| state.schedule_prefetch(now + Duration::from_millis(*i), 50_000)).count();
            assert_eq!(count, 12, "60,000 ticks must schedule at most 12 checks/minute");
            state.state = "paused".into();
            assert!(!state.schedule_prefetch(now + Duration::from_secs(90), 50_000));
            state.state = "playing".into(); state.prefetching_version_id = Some("v".into());
            assert!(!state.schedule_prefetch(now + Duration::from_secs(90), 50_000));
        }
        let operation = pb.prefetch_operation.lock().await;
        tokio::time::timeout(Duration::from_millis(100), pb.maybe_prefetch_next_track()).await.unwrap();
        drop(operation);
        {
            let mut state = pb.inner.lock();
            state.prefetched_next = Some(("v".into(), "s".into(), "expired-url".into(), vec![]));
            state.prefetched_at = Some(now - Duration::from_secs(61));
        }
        pb.maybe_prefetch_next_track().await;
        assert!(pb.inner.lock().prefetched_next.is_none());
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn long_runtime_idle_subtitle_poll_does_not_wait_for_database() {
        use crate::services::{recognition::RecognitionService, subtitle_tasks::SubtitleTaskService};
        let (root, pb) = online_stall_fixture();
        pb.inner.lock().state = "paused".into();
        let tasks = SubtitleTaskService::new(pb.db.clone(),
            RecognitionService::new(pb.db.clone(), root.path().join("d"), "idle-poll-test"),
            pb.clone(), pb.provider.clone());
        let db = pb.db.clone();
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || db.with_conn(|_| {
            held_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
            Ok(())
        }).unwrap());
        held_rx.recv().unwrap();
        let start = Instant::now();
        let mut attempted = None;
        for _ in 0..1000 { tasks.automatic_step(&mut attempted).await; }
        let elapsed = start.elapsed();
        let _ = release_tx.send(()); writer.join().unwrap();
        pb.shutdown().await;
        assert!(attempted.is_none());
        assert!(elapsed < Duration::from_millis(200), "idle polling waited on database: {elapsed:?}");
    }

    #[tokio::test]
    async fn audio_output_failure_keeps_track_progress_and_skips_online_recovery() {
        for source_kind in ["online_provider", "local_imported"] {
            let (_root, pb) = online_stall_fixture();
            let queue_id = pb.db.append_queue_items(&[("v".into(), None)]).unwrap().remove(0);
            {
                let mut state = pb.inner.lock();
                state.active_source.as_mut().unwrap().source_kind = source_kind.into();
                state.current_queue_item_id = Some(queue_id.clone());
                state.pending_seek_ms = Some(2300);
            }
            let (tx, rx) = mpsc::unbounded_channel();
            pb.spawn_event_loop(rx);
            tx.send(MpvEvent::EndFile {
                reason: "error".into(),
                error: Some("audio output initialization failed".into()),
                audio_output_error: Some(AppError::new(ErrorCode::AudioDeviceBusy, "音频设备被占用")),
                playlist_entry_id: None,
            }).unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while pb.inner.lock().last_error.is_none() { tokio::task::yield_now().await; }
            }).await.unwrap();
            {
                let state = pb.inner.lock();
                assert_eq!(state.state, "error");
                assert_eq!(state.last_error.as_ref().unwrap().code, ErrorCode::AudioDeviceBusy);
                assert_eq!(state.current_version_id.as_deref(), Some("v"));
                assert_eq!(state.current_queue_item_id.as_deref(), Some(queue_id.as_str()));
                assert_eq!(state.position_ms, 1000);
                assert_eq!(state.pending_seek_ms, Some(2300));
                assert!(!state.current_completed);
                assert!(!state.file_loaded_for_gen);
                assert!(!state.mpv_buffering);
                assert!(!state.online_recovery_running);
                assert!(!state.retried_online_for_gen);
            }
            assert_eq!(pb.db.get_queue_items().unwrap().len(), 1);
            pb.shutdown().await;
        }
    }

    fn online_stall_fixture() -> (tempfile::TempDir, PlaybackService) {
        let root = tempdir().unwrap();
        let db = Database::open_and_migrate(&root.path().join("d"), &root.path().join("c")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','test','now','now');
                INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','test','1','now');
                INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');")?;
            Ok(())
        }).unwrap();
        let pb = PlaybackService::new(db.clone(), AsmrOneProvider::new(db));
        {
            let mut state = pb.inner.lock();
            state.current_version_id = Some("v".into());
            state.active_source = Some(serde_json::from_value(serde_json::json!({
                "id":"remote", "versionId":"v", "sourceKind":"online_provider",
                "provider":"asmr_one", "sourceWorkId":"1", "remoteFileId":"hash",
                "remoteRelativePath":"test.mp3", "format":"mp3", "availability":"available"
            })).unwrap());
            state.state = "buffering".into();
            state.mpv_buffering = true;
            state.file_loaded_for_gen = true;
            state.user_paused_intent = false;
            state.position_ms = 1000;
            state.volume = 0.0;
        }
        (root, pb)
    }

    #[tokio::test]
    async fn online_failure_during_recovery_is_queued_and_stale_sessions_are_ignored() {
        for stale in [false, true] {
            let (_root, pb) = online_stall_fixture();
            let gate = pb.recovery_operation.lock().await;
            let generation = {
                let mut state = pb.inner.lock();
                state.retried_online_for_gen = true;
                state.online_recovery_running = true;
                state.session_generation
            };
            let recovery = pb.clone();
            let (started, ready) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                started.send(()).unwrap();
                recovery.recover_online_stream(generation, false).await;
            });
            ready.await.unwrap(); // the second failure has reached the busy recovery
            assert!(!task.is_finished());
            {
                let mut state = pb.inner.lock();
                state.online_recovery_running = false;
                if stale { state.session_generation += 1; }
            }
            drop(gate);
            tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
            {
                let state = pb.inner.lock();
                if stale {
                    assert_eq!(state.state, "buffering");
                    assert!(state.last_error.is_none());
                } else {
                    assert_eq!(state.state, "error");
                    assert!(state.last_error.as_ref().unwrap().message.contains("加载失败"));
                    assert!(!state.online_recovery_running);
                }
            }
            pb.shutdown().await;
        }
    }

    #[tokio::test]
    async fn online_manual_retry_gets_fresh_recovery_budget_and_reports_load_failure() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        let (_root, pb) = online_stall_fixture();
        pb.db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO media_sources(id,version_id,source_kind,provider,source_work_id,remote_file_id,remote_relative_path,format,created_at,updated_at)
                VALUES('remote','v','online_provider','asmr_one','1','hash','test.mp3','mp3','now','now');")?;
            Ok(())
        }).unwrap();
        let server = MockServer::start().await;
        Mock::given(path("/api/tracks/1")).respond_with(ResponseTemplate::new(200)
            .set_body_json(serde_json::json!([{"type":"audio", "title":"test.mp3", "hash":"hash",
                "mediaStreamUrl":format!("{}/missing.mp3", server.uri())}]))).expect(4).mount(&server).await;
        Mock::given(path("/missing.mp3")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        pb.provider.set_api_base(&server.uri()).unwrap();
        { let mut state = pb.inner.lock(); state.state = "error".into(); state.retried_online_for_gen = true; }
        for _ in 0..2 {
            pb.toggle_pause().await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                while pb.inner.lock().state != "error" { tokio::time::sleep(Duration::from_millis(10)).await; }
            }).await.expect("A failed manual retry must finish after one automatic refresh");
            let state = pb.inner.lock();
            assert!(state.user_paused_intent);
            let message = &state.last_error.as_ref().unwrap().message;
            assert!(message.contains("加载失败"));
            assert!(!message.contains("持续缓冲"));
        }
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn online_stall_deadline_interrupts_real_native_request_and_can_retry() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        let (_root, pb) = online_stall_fixture();
        let server = MockServer::start().await;
        Mock::given(path("/api/tracks/1")).respond_with(ResponseTemplate::new(200)
            .set_body_json(serde_json::json!([{"type":"audio", "title":"test.mp3", "hash":"hash",
                "mediaStreamUrl":format!("{}/stalled.mp3", server.uri())}]))).expect(1).mount(&server).await;
        Mock::given(path("/stalled.mp3")).respond_with(ResponseTemplate::new(200)
            .set_delay(Duration::from_secs(60))).mount(&server).await;
        pb.provider.set_api_base(&server.uri()).unwrap();
        pb.inner.lock().buffering_since = Some(Instant::now() - Duration::from_secs(31));
        pb.check_online_buffering(Instant::now()).await;
        tokio::time::timeout(Duration::from_secs(3), async {
            while server.received_requests().await.unwrap().len() < 2 { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.unwrap();
        pb.inner.lock().buffering_since = Some(Instant::now() - Duration::from_secs(31));
        tokio::time::timeout(Duration::from_secs(3), pb.check_online_buffering(Instant::now()))
            .await.expect("The host deadline must interrupt a native HTTP read");
        assert_eq!(pb.inner.lock().state, "error");
        assert!(pb.inner.lock().last_error.as_ref().unwrap().message.contains("持续缓冲"));
        let audio = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3");
        { let mut state = pb.inner.lock(); state.state = "loading".into(); state.user_paused_intent = true; }
        pb.mpv.load_file_pipelined(&audio.to_string_lossy(), &[], true, 0.0, "").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !pb.inner.lock().file_loaded_for_gen { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.expect("Playback must remain usable after the host stops a stalled request");
        assert_eq!(pb.inner.lock().state, "paused");
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn online_stall_pause_time_does_not_count_and_resume_has_grace() {
        let (_root, pb) = online_stall_fixture();
        let now = Instant::now();
        {
            let mut state = pb.inner.lock();
            state.user_paused_intent = true;
            state.buffering_since = Some(now - Duration::from_secs(5 * 3600));
        }
        pb.check_online_buffering(now).await;
        assert!(pb.inner.lock().buffering_since.is_none());
        pb.inner.lock().user_paused_intent = false;
        pb.check_online_buffering(now).await;
        pb.check_online_buffering(now + Duration::from_secs(29)).await;
        assert!(!pb.inner.lock().retried_online_for_gen);
        assert_eq!(pb.inner.lock().position_ms, 1000);
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn online_stall_failed_refresh_exits_buffering_and_is_bounded() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        let (_root, pb) = online_stall_fixture();
        let server = MockServer::start().await;
        Mock::given(path("/api/tracks/1")).respond_with(ResponseTemplate::new(404)).expect(1).mount(&server).await;
        pb.provider.set_api_base(&server.uri()).unwrap();
        pb.inner.lock().buffering_since = Some(Instant::now() - Duration::from_secs(31));
        pb.check_online_buffering(Instant::now()).await;
        let generation = {
            let state = pb.inner.lock();
            assert_eq!(state.state, "error");
            assert!(state.last_error.is_some());
            assert_eq!(state.position_ms, 1000);
            assert!(state.user_paused_intent);
            state.session_generation
        };
        pb.recover_online_stream(generation, false).await;
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn online_stall_refresh_preserves_position_and_latest_pause_intent() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        let (_root, pb) = online_stall_fixture();
        let server = MockServer::start().await;
        let audio = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3");
        Mock::given(path("/api/tracks/1")).respond_with(ResponseTemplate::new(200)
            .set_body_json(serde_json::json!([{"type":"audio", "title":"test.mp3", "hash":"hash", "mediaStreamUrl":audio}]))).expect(1).mount(&server).await;
        pb.provider.set_api_base(&server.uri()).unwrap();
        // Hold the real load boundary instead of relying on a 200 ms HTTP delay.
        // The new pause intent must arrive before the resolved source can load.
        let load_gate = pb.load_operation.lock().await;
        let generation = pb.inner.lock().session_generation;
        let recovery = pb.clone();
        let task = tokio::spawn(async move { recovery.recover_online_stream(generation, true).await; });
        tokio::time::timeout(Duration::from_secs(3), async {
            while server.received_requests().await.unwrap().is_empty() { tokio::task::yield_now().await; }
        }).await.unwrap();
        pb.inner.lock().user_paused_intent = true;
        drop(load_gate);
        task.await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if { let state = pb.inner.lock(); state.file_loaded_for_gen && state.pending_seek_ms.is_none() } { break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.unwrap();
        assert_eq!(pb.mpv.command(serde_json::json!(["get_property", "pause"])).await.unwrap(), true);
        let position = pb.mpv.command(serde_json::json!(["get_property", "time-pos"])).await.unwrap().as_f64().unwrap();
        assert!((position - 1.0).abs() < 0.15, "position={position}");
        assert_eq!(pb.inner.lock().state, "paused");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn online_stall_recovery_rechecks_buffer_events_and_exhausted_budget_at_load_boundary() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        // No healthy native stream is falsified into a stall. Explicit event order
        // covers both outcomes of the race, before and after the retry is spent.
        for exhausted in [false, true] {
            for recovered in [false, true] {
                let (_root, pb) = online_stall_fixture();
                let server = MockServer::start().await;
                Mock::given(path("/api/tracks/1")).respond_with(ResponseTemplate::new(404))
                    .expect(if exhausted { 0 } else { 1 }).mount(&server).await;
                pb.provider.set_api_base(&server.uri()).unwrap();
                pb.inner.lock().retried_online_for_gen = exhausted;
                let generation = pb.inner.lock().session_generation;
                let (tx, rx) = mpsc::unbounded_channel();
                pb.spawn_event_loop(rx);
                let load_gate = pb.load_operation.lock().await;
                let recovery = pb.clone();
                let task = tokio::spawn(async move { recovery.recover_online_stream(generation, true).await; });
                tokio::time::timeout(Duration::from_secs(3), async {
                    while !pb.inner.lock().online_recovery_running { tokio::task::yield_now().await; }
                }).await.unwrap();
                if recovered {
                    tx.send(MpvEvent::BufferingChanged(false)).unwrap();
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while pb.inner.lock().mpv_buffering { tokio::task::yield_now().await; }
                    }).await.unwrap();
                }
                drop(load_gate);
                task.await.unwrap();
                {
                    let state = pb.inner.lock();
                    assert_eq!(state.state, if recovered { "playing" } else { "error" });
                    assert_eq!(state.retried_online_for_gen, exhausted || !recovered);
                    assert_eq!(state.file_loaded_for_gen, recovered);
                    assert_eq!(state.user_paused_intent, !recovered);
                    assert_eq!(state.last_error.is_some(), !recovered);
                    assert!(!state.online_recovery_running);
                    assert_eq!(state.position_ms, 1000);
                }
                if !recovered {
                    // Late cache/pause notifications cannot clear the terminal error.
                    tx.send(MpvEvent::BufferingChanged(false)).unwrap();
                    tx.send(MpvEvent::PauseChanged(true)).unwrap();
                    tx.send(MpvEvent::AudioBitrateChanged(Some(421000))).unwrap();
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while pb.inner.lock().audio_telemetry.bitrate_kbps != Some(421) { tokio::task::yield_now().await; }
                    }).await.unwrap();
                    assert_eq!(pb.inner.lock().state, "error");
                    pb.recover_online_stream(generation, true).await;
                }
                assert_eq!(server.received_requests().await.unwrap().len(), usize::from(!exhausted));
                pb.shutdown().await;
            }
        }
    }

    #[tokio::test]
    async fn online_stall_late_refresh_cannot_replace_new_selection() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        let (_root, pb) = online_stall_fixture();
        let server = MockServer::start().await;
        Mock::given(path("/api/tracks/1")).respond_with(ResponseTemplate::new(404).set_delay(Duration::from_millis(200))).expect(1).mount(&server).await;
        pb.provider.set_api_base(&server.uri()).unwrap();
        let generation = pb.inner.lock().session_generation;
        let recovery = pb.clone();
        let task = tokio::spawn(async move { recovery.recover_online_stream(generation, true).await; });
        tokio::time::timeout(Duration::from_secs(3), async {
            while server.received_requests().await.unwrap().is_empty() { tokio::task::yield_now().await; }
        }).await.unwrap();
        {
            let mut state = pb.inner.lock();
            state.session_generation += 1;
            state.current_version_id = Some("new-selection".into());
            state.state = "playing".into();
            state.online_recovery_running = false;
        }
        task.await.unwrap();
        assert_eq!(pb.inner.lock().current_version_id.as_deref(), Some("new-selection"));
        assert_eq!(pb.inner.lock().state, "playing");
        assert!(pb.inner.lock().last_error.is_none());
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn t1513_manual_subtitle_edits_publish_preserve_originals_and_reject_stale_or_running_saves() {
        use crate::services::{recognition::RecognitionService,subtitle_tasks::SubtitleTaskService,subtitle_documents::{SubtitleEdit,SubtitleFormat,SubtitleMode}};
        let root=tempdir().unwrap();
        let db=Database::open_and_migrate(&root.path().join("d"),&root.path().join("c")).unwrap();
        db.with_conn(|conn| {conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');")?; Ok(())}).unwrap();
        let pb=PlaybackService::new(db.clone(),AsmrOneProvider::new(db.clone()));
        let original=vec![SubtitleCueDto {index:13,start_ms:100,end_ms:900,text:"おやすみ".into()}];
        {let mut state=pb.inner.lock(); state.current_version_id=Some("v".into()); state.active_subtitle_cues=original.clone();}
        let svc=SubtitleTaskService::new(db.clone(),RecognitionService::new(db.clone(),root.path().join("d"),"editing-test"),pb.clone(),AsmrOneProvider::new(db.clone()));
        let read=serde_json::to_value(svc.editable_document("v").unwrap()).unwrap();
        let edit=|| SubtitleEdit {version_id:"v".into(),source_key:read["sourceKey"].as_str().unwrap().into(),revision:read["revision"].as_str().unwrap().into(),index:13,original:"おやすみなさい".into(),translation:"晚安".into(),reset:false,undo_model:false,exclude_non_dialogue:false};
        let result=serde_json::to_value(svc.edit_document(edit()).unwrap()).unwrap();
        assert_eq!(pb.subtitle_context().2[0].text,"おやすみなさい\n晚安");
        assert!(svc.edit_document(edit()).is_err());
        let saved=db.subtitle_document("v",read["sourceKey"].as_str().unwrap()).unwrap().unwrap();
        assert_eq!(saved.original,original);
        assert_eq!(db.subtitle_overlay("v",original),saved.rendered());
        let path=root.path().join("export.srt");
        assert!(svc.export_document("v",read["revision"].as_str().unwrap(),SubtitleFormat::Srt,SubtitleMode::Bilingual,&path).is_err());
        assert!(!path.exists());
        svc.export_document("v",result["revision"].as_str().unwrap(),SubtitleFormat::Srt,SubtitleMode::Bilingual,&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("晚安"));
        svc.start_batch(vec!["v".into()],true,true,false).unwrap();
        let mut current_edit=edit(); current_edit.revision=result["revision"].as_str().unwrap().into();
        assert!(svc.edit_document(current_edit).unwrap_err().message.contains("正在运行"));
        svc.cancel();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        svc.shutdown(); pb.shutdown().await;
    }

    fn local_translation_response(request: &wiremock::Request, text: &str) -> wiremock::ResponseTemplate {
        let body: serde_json::Value = request.body_json().unwrap();
        let user: serde_json::Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        let translations: Vec<_> = user["sentences"].as_array().unwrap().iter()
            .map(|line| serde_json::json!({"id":line["id"],"parts":[{"source":line["text"],"translation":text}]})).collect();
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":serde_json::json!({"translations":translations}).to_string()}}))
    }

    #[tokio::test]
    async fn t1513_preferences_rules_and_work_glossary_flow_through_saved_subtitles() {
        use crate::{db::subtitles::{SubtitleDocument,external_key},services::{recognition::{RecognitionService,RecognitionSettings},subtitle_preferences::{SubtitlePreferences,Term},subtitle_tasks::SubtitleTaskService}};
        use wiremock::{Mock,MockServer,matchers::{method,path}};
        let root=tempdir().unwrap(); let db=Database::open_and_migrate(&root.path().join("d"),&root.path().join("c")).unwrap();
        db.with_conn(|c| {c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','作品','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');")?;Ok(())}).unwrap();
        let server=MockServer::start().await;
        let recognition=RecognitionService::new(db.clone(),root.path().join("d"),"prefs-test");
        let term=|source:&str,target:&str| Term{source:source.into(),target:target.into(),..Default::default()};
        let prefs=SubtitlePreferences{glossary:vec![term("先輩","前辈")],work_glossaries:std::collections::BTreeMap::from([("w".into(),vec![term("先輩","学姐")]),("other".into(),vec![term("先輩","不应发送")])]),corrections_enabled:true,corrections:vec![term("おやすみ","おやすみなさい")],translation_style:"soft".into(),translation_punctuation:"minimal".into(),..Default::default()};
        recognition.save_settings(RecognitionSettings{ollama_url:server.uri(),preferences:prefs.clone(),..Default::default()}).unwrap();
        let pb=PlaybackService::new(db.clone(),AsmrOneProvider::new(db.clone()));
        let cues=vec![SubtitleCueDto{index:4,start_ms:0,end_ms:900,text:"先輩おやすみ".into()},SubtitleCueDto{index:5,start_ms:1000,end_ms:1900,text:"またね".into()},SubtitleCueDto{index:6,start_ms:2000,end_ms:2900,text:"呼".into()}];
        let key=external_key(&cues); let mut doc=SubtitleDocument::new("v".into(),key.clone(),cues.clone(),true);
        doc.translation_corrections.insert(5,"手工译文".into()); doc.translations.insert(4,"旧译文".into()); db.save_subtitle_document(&doc).unwrap();
        {let mut state=pb.inner.lock();state.current_version_id=Some("v".into());state.active_subtitle_cues=doc.rendered();}
        let tasks=SubtitleTaskService::new(db.clone(),recognition.clone(),pb.clone(),AsmrOneProvider::new(db.clone()));
        assert_eq!(tasks.subtitle_work_context(None).unwrap().work_id,"w");
        assert_eq!(tasks.apply_current_subtitle_rules("v",false).unwrap(),1);
        assert_eq!(pb.subtitle_context().2[0].text,"先輩おやすみなさい");
        assert_eq!(tasks.apply_current_subtitle_rules("v",true).unwrap(),1);
        assert_eq!(pb.subtitle_context().2[0].text,"先輩おやすみ");
        assert!(db.subtitle_document("v",&key).unwrap().unwrap().rules_suppressed);
        tasks.add_subtitle_term("v",term("先生","老师")).unwrap();
        assert!(tasks.add_subtitle_term("v",term("先生","另一个译名")).is_err());
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(|r: &wiremock::Request| local_translation_response(r, "学姐，晚安～")).mount(&server).await;
        async fn finish(tasks:&SubtitleTaskService) {tokio::time::timeout(Duration::from_secs(5),async {while matches!(tasks.snapshot().state.as_str(),"running"|"cancelling") {tokio::time::sleep(Duration::from_millis(10)).await;}}).await.unwrap();assert_eq!(tasks.snapshot().state,"completed","{:?}",tasks.snapshot().error);}
        tasks.start_batch(vec!["v".into()],true,true,false).unwrap();finish(&tasks).await;
        let requests=server.received_requests().await.unwrap(); assert_eq!(requests.len(),1); // breath and manual translation don't call the API
        let body:serde_json::Value=serde_json::from_slice(&requests[0].body).unwrap();
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("柔和"));
        let user:serde_json::Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(user["glossary"],serde_json::json!([{"source":"先輩","target":"学姐","misrecognitions":[],"context":""}]));assert_eq!(user["sentences"],serde_json::json!([{"id":4,"text":"先輩おやすみ","sourceParts":["先輩おやすみ"]}]));
        let saved=db.subtitle_document("v",&key).unwrap().unwrap(); assert_eq!(saved.original,cues); assert_eq!(saved.translation_text(4),Some("学姐 晚安"));assert_eq!(saved.translation_text(5),Some("手工译文"));assert_eq!(saved.rendered()[2].text,"呼");
        let mut changed=recognition.settings().unwrap().preferences;changed.translation_style="natural".into();recognition.save_preferences(changed).unwrap();
        tasks.start_batch(vec!["v".into()],true,true,false).unwrap();finish(&tasks).await;
        assert_eq!(server.received_requests().await.unwrap().len(),2);assert_eq!(db.subtitle_document("v",&key).unwrap().unwrap().translation_text(5),Some("手工译文"));
        assert_eq!(tasks.apply_current_subtitle_rules("v",false).unwrap(),1);
        {pb.inner.lock().current_version_id=Some("other".into());}
        assert!(tasks.apply_current_subtitle_rules("v",true).is_err());
        assert!(!recognition.model_directory().exists()); assert!(!recognition.runtime_directory().exists());
        tasks.shutdown();pb.shutdown().await;
    }

    #[tokio::test]
    async fn t1513_automatic_modes_restore_full_cache_and_translate_partial_without_recognizing() {
        use crate::{db::subtitles::SubtitleDocument, services::{recognition::{AutoRecognitionMode, RecognitionService, RecognitionSettings}, subtitle_tasks::SubtitleTaskService}};
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::{method, path}};
        let root = tempdir().unwrap();
        let db = Database::open_and_migrate(&root.path().join("d"), &root.path().join("c")).unwrap();
        db.with_conn(|c| { c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now'); INSERT INTO media_sources(id,version_id,source_kind,remote_file_id,format,availability,created_at,updated_at) VALUES('v','v','online_provider','v','mp3','available','now','now');")?; Ok(()) }).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let recognition = RecognitionService::new(db.clone(), root.path().join("d"), "auto-modes");
        let pb = PlaybackService::new(db.clone(), provider.clone());
        let tasks = SubtitleTaskService::new(db.clone(), recognition.clone(), pb.clone(), provider);
        let chinese = vec![SubtitleCueDto { index: 0, start_ms: 0, end_ms: 1000, text: "已有中文字幕".into() }];
        let original = vec![SubtitleCueDto { text: "おはよう".into(), ..chinese[0].clone() }];
        let mut doc = SubtitleDocument::new("v".into(), db.audio_subtitle_key("v").unwrap(), original, true);
        db.save_subtitle_document(&doc).unwrap();
        { let mut state = pb.inner.lock(); state.current_version_id = Some("v".into()); state.state = "playing".into(); state.active_subtitle_cues = chinese.clone(); }
        let mut settings = RecognitionSettings { auto_recognition: AutoRecognitionMode::Missing, ..Default::default() };
        recognition.save_settings(settings.clone()).unwrap();
        let mut attempted = None;
        tasks.automatic_step(&mut attempted).await;
        assert_eq!(pb.subtitle_context().2, chinese);
        assert_eq!(tasks.snapshot().state, "");
        settings.auto_recognition = AutoRecognitionMode::All;
        recognition.save_settings(settings.clone()).unwrap();
        tasks.automatic_step(&mut attempted).await;
        assert_eq!(pb.subtitle_context().2, doc.rendered());
        assert_eq!(tasks.snapshot().state, ""); // Reuses completed cache without a model.
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/tags")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[{"name":"qwen3:4b"}]}))).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(|r: &wiremock::Request| local_translation_response(r, "早上好")).expect(1).mount(&server).await;
        doc.recognition_complete = false;
        doc.completed_chunks.insert(0);
        db.save_subtitle_document(&doc).unwrap();
        settings.auto_recognition = AutoRecognitionMode::Off;
        settings.auto_translation = true;
        settings.ollama_url = server.uri();
        recognition.save_settings(settings).unwrap();
        tasks.automatic_step(&mut attempted).await;
        tokio::time::timeout(Duration::from_secs(5), async { while matches!(tasks.snapshot().state.as_str(), "running" | "cancelling") { tokio::time::sleep(Duration::from_millis(10)).await; } }).await.unwrap();
        assert_eq!(tasks.snapshot().state, "completed", "{:?}", tasks.snapshot().error);
        let saved = db.subtitle_document("v", &doc.source_key).unwrap().unwrap();
        assert!(!saved.recognition_complete && saved.completed_chunks == doc.completed_chunks);
        assert_eq!(saved.translations.get(&0).map(String::as_str), Some("早上好"));
        assert!(!root.path().join("d/models").exists());
        tasks.shutdown();
        pb.shutdown().await;
    }

    #[tokio::test]
    async fn t1513_batch_and_automatic_processing_preserve_playback_and_require_opt_in() {
        use crate::{db::subtitles::SubtitleDocument, services::{recognition::{RecognitionService, RecognitionSettings}, subtitle_tasks::SubtitleTaskService}};
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::{method,path}};
        let root=tempdir().unwrap();
        let db=Database::open_and_migrate(&root.path().join("d"),&root.path().join("c")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now');")?;
            for id in ["a","b","c"] {
                conn.execute("INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES(?1,'t',?1,'now')",[id])?;
                conn.execute("INSERT INTO media_sources(id,version_id,source_kind,remote_file_id,format,availability,created_at,updated_at) VALUES(?1,?1,'online_provider',?1,'mp3','available','now','now')",[id])?;
            } Ok(())
        }).unwrap();
        let original=vec![SubtitleCueDto {index:0,start_ms:0,end_ms:1000,text:"おはよう".into()}];
        for id in ["a","b"] { db.save_subtitle_document(&SubtitleDocument::new(id.into(),db.audio_subtitle_key(id).unwrap(),original.clone(),true)).unwrap(); }
        let server=MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/tags")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[{"name":"qwen3:4b"}]}))).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(|r: &wiremock::Request| local_translation_response(r, "早上好")).mount(&server).await;
        let recognition=RecognitionService::new(db.clone(),root.path().join("d"),"automatic-test");
        let mut settings=RecognitionSettings {ollama_url:server.uri(),..Default::default()};
        recognition.save_settings(settings.clone()).unwrap();
        let playback=PlaybackService::new(db.clone(),AsmrOneProvider::new(db.clone()));
        { let mut state=playback.inner.lock(); state.current_version_id=Some("a".into()); state.state="playing".into(); }
        let tasks=SubtitleTaskService::new(db.clone(),recognition.clone(),playback.clone(),AsmrOneProvider::new(db.clone()));
        let mut attempted=None;
        tasks.automatic_step(&mut attempted).await;
        assert_eq!(tasks.snapshot().state,"");
        assert!(server.received_requests().await.unwrap().is_empty());
        let readiness=tasks.readiness(&["b".into(),"c".into()]).await.unwrap();
        assert!(!readiness[0].can_recognize && readiness[0].can_translate);
        assert!(!readiness[1].can_recognize && !readiness[1].can_translate);
        assert!(tasks.start_batch(vec!["c".into()],false,false,false).is_err());
        async fn finished(tasks:&SubtitleTaskService) {
            tokio::time::timeout(std::time::Duration::from_secs(5),async {
                while matches!(tasks.snapshot().state.as_str(),"running"|"cancelling") {tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
            }).await.unwrap();
        }
        tasks.start_batch(vec!["b".into(),"a".into(),"b".into()],true,false,false).unwrap();
        finished(&tasks).await;
        assert_eq!(tasks.snapshot().state,"completed");
        assert_eq!(tasks.snapshot().tracks_total,2); assert_eq!(tasks.snapshot().tracks_completed,2);
        assert_eq!(playback.subtitle_context().1.as_deref(),Some("a"));
        assert_eq!(playback.get_snapshot().unwrap().state,"playing");
        assert_eq!(db.subtitle_overlay("b",vec![])[0].text,"おはよう\n早上好");
        // Generated Chinese is an overlay, never an existing Chinese attachment.
        assert!(tasks.prepare(&["b".into()]).await.unwrap().is_empty());
        tasks.start_batch(vec!["b".into()],true,false,false).unwrap();
        finished(&tasks).await;
        // Chinese originals are skipped without contacting the translation provider.
        playback.inner.lock().active_subtitle_cues=vec![SubtitleCueDto {index:0,start_ms:0,end_ms:1000,text:"已经是中文字幕".into()}];
        settings.auto_translation=true; recognition.save_settings(settings.clone()).unwrap();
        let before_chinese=server.received_requests().await.unwrap().len();
        tasks.automatic_step(&mut attempted).await;
        assert_eq!(server.received_requests().await.unwrap().len(),before_chinese);
        // Explicit opt-in starts a single automatic job; repeated ticks do not resubmit it.
        db.save_subtitle_document(&SubtitleDocument::new("a".into(),db.audio_subtitle_key("a").unwrap(),original.clone(),true)).unwrap();
        playback.inner.lock().active_subtitle_cues=original.clone();
        playback.inner.lock().session_generation+=1;
        settings.auto_translation=true; recognition.save_settings(settings.clone()).unwrap();
        tasks.automatic_step(&mut attempted).await; finished(&tasks).await;
        assert_eq!(tasks.snapshot().state,"completed");
        let count=server.received_requests().await.unwrap().len();
        tasks.automatic_step(&mut attempted).await;
        assert_eq!(server.received_requests().await.unwrap().len(),count);
        // Automatic translation follows the read-ahead window, then a seek, without
        // translating the rest of the track or resubmitting completed sentences.
        let buffered = vec![
            SubtitleCueDto { index: 0, start_ms: 0, end_ms: 1000, text: "おはよう".into() },
            SubtitleCueDto { index: 1, start_ms: 25_000, end_ms: 26_000, text: "こんにちは".into() },
            SubtitleCueDto { index: 2, start_ms: 65_000, end_ms: 66_000, text: "こんばんは".into() },
        ];
        let key = db.audio_subtitle_key("a").unwrap();
        db.save_subtitle_document(&SubtitleDocument::new("a".into(), key.clone(), buffered.clone(), true)).unwrap();
        { let mut state = playback.inner.lock(); state.active_subtitle_cues = buffered;
          state.session_generation += 1; state.position_ms = 0; state.duration_ms = Some(90_000); }
        tasks.automatic_step(&mut attempted).await; finished(&tasks).await;
        assert_eq!(db.subtitle_document("a", &key).unwrap().unwrap().translations.keys().copied().collect::<Vec<_>>(), vec![0]);
        playback.inner.lock().position_ms = 6000;
        tasks.automatic_step(&mut attempted).await; finished(&tasks).await;
        assert_eq!(db.subtitle_document("a", &key).unwrap().unwrap().translations.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
        playback.inner.lock().position_ms = 65_000;
        tasks.automatic_step(&mut attempted).await; finished(&tasks).await;
        assert_eq!(db.subtitle_document("a", &key).unwrap().unwrap().translations.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2]);
        let count=server.received_requests().await.unwrap().len();
        tasks.automatic_step(&mut attempted).await;
        assert_eq!(server.received_requests().await.unwrap().len(), count);
        // Deletion updates the player immediately and cannot be undone by an
        // automatic task prepared before deletion. Explicit translation still works.
        tasks.delete_generated(&["a".into()], true).unwrap();
        assert!(db.subtitle_document("a", &key).unwrap().unwrap().translations.is_empty());
        assert!(playback.subtitle_context().2.iter().all(|cue| !cue.text.contains("早上好")));
        assert_eq!(playback.get_snapshot().unwrap().state, "playing");
        attempted = None;
        tasks.automatic_step(&mut attempted).await;
        assert!(tasks.start_batch(vec!["a".into()], true, true, true).is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), count);
        tasks.start_batch(vec!["a".into()], true, false, false).unwrap();
        finished(&tasks).await;
        assert_eq!(tasks.snapshot().state, "completed");
        let count = server.received_requests().await.unwrap().len();
        // Cancelling suppresses automatic restarts for this playback session.
        tasks.cancel(); attempted=None;
        tasks.automatic_step(&mut attempted).await;
        assert_eq!(server.received_requests().await.unwrap().len(),count);
        assert!(!recognition.model_directory().exists()); assert!(!recognition.runtime_directory().exists());
        tasks.shutdown(); playback.shutdown().await;
    }

    #[tokio::test]
    async fn t1513_translation_resumes_saved_sentences_and_never_overwrites_new_selection() {
        use crate::{db::subtitles::external_key, services::{recognition::{RecognitionService,RecognitionSettings},subtitle_tasks::SubtitleTaskService}};
        use wiremock::{Mock,MockServer,ResponseTemplate,matchers::{method,path}};
        let dir=tempdir().unwrap();
        let db=Database::open_and_migrate(&dir.path().join("d"),&dir.path().join("c")).unwrap();
        db.with_conn(|conn| { conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES('v','t','original','now'),('other','t','other','now');")?; Ok(()) }).unwrap();
        let server=MockServer::start().await;
        let recognition=RecognitionService::new(db.clone(),dir.path().join("d"),"subtitle-test");
        recognition.save_settings(RecognitionSettings { ollama_url:server.uri(),..Default::default() }).unwrap();
        let playback=PlaybackService::new(db.clone(),AsmrOneProvider::new(db.clone()));
        let mut cues: Vec<_> = (0..6).map(|i| SubtitleCueDto {index:10+i,start_ms:i as i64*1000,end_ms:(i as i64+1)*1000,text:"おはよう".into()}).collect();
        cues.push(SubtitleCueDto {index:9,start_ms:6000,end_ms:7000,text:"ありがとう".into()});
        { let mut state=playback.inner.lock(); state.current_version_id=Some("v".into()); state.active_subtitle_cues=cues.clone(); }
        let tasks=SubtitleTaskService::new(db.clone(),recognition.clone(),playback.clone(),AsmrOneProvider::new(db.clone()));
        assert!(server.received_requests().await.unwrap().is_empty());
        let count=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)); let counter=count.clone();
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(move |r: &wiremock::Request| {
            if counter.fetch_add(1,Ordering::SeqCst)==0 { local_translation_response(r, "早上好") }
            else { ResponseTemplate::new(500) }
        }).mount(&server).await;
        tasks.start("v".into(),true).unwrap();
        assert!(tasks.start("v".into(),true).is_err());
        async fn finished(tasks:&SubtitleTaskService) { tokio::time::timeout(std::time::Duration::from_secs(5),async { while matches!(tasks.snapshot().state.as_str(),"running"|"cancelling") { tokio::time::sleep(std::time::Duration::from_millis(10)).await; } }).await.unwrap(); }
        finished(&tasks).await;
        assert_eq!(tasks.snapshot().state,"failed");
        let key=external_key(&cues);
        assert_eq!(db.subtitle_document("v",&key).unwrap().unwrap().translations.len(),6);
        assert!(playback.subtitle_context().2[0].text.contains("早上好"));
        server.reset().await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(|r: &wiremock::Request| local_translation_response(r, "谢谢")).expect(1).mount(&server).await;
        tasks.start_batch(vec!["v".into()],true,true,false).unwrap(); finished(&tasks).await;
        assert_eq!(tasks.snapshot().state,"completed");
        let requests=server.received_requests().await.unwrap();
        assert_eq!(requests.len(),1); assert_eq!(requests[0].url.path(),"/api/chat");
        let body:serde_json::Value=serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["stream"],false); assert_eq!(body["format"]["type"],"object");
        let user:serde_json::Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(user["sentences"],serde_json::json!([{"id":9,"text":"ありがとう","sourceParts":["ありがとう"]}])); assert_eq!(user["context"],"句子 9 之前 [13] おはよう\n句子 9 之前 [14] おはよう\n句子 9 之前 [15] おはよう");
        assert!(!recognition.model_directory().exists()); assert!(!recognition.runtime_directory().exists());
        let (generation,_,rendered,_)=playback.subtitle_context();
        assert_eq!(rendered[6].text,"ありがとう\n谢谢");
        assert!(db.subtitle_overlay("other",cues.clone())==cues);
        // Old results cannot replace a manual selection, a new version, or a new session.
        assert!(!playback.publish_generated_subtitles("v",generation,&cues,Vec::new()));
        assert!(!playback.publish_generated_subtitles("other",generation,&rendered,Vec::new()));
        assert!(!playback.publish_generated_subtitles("v",generation+1,&rendered,Vec::new()));
        server.reset().await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(30))).mount(&server).await;
        { let mut state=playback.inner.lock(); state.active_subtitle_cues=vec![SubtitleCueDto {index:0,start_ms:0,end_ms:1000,text:"別の字幕".into()}]; }
        tasks.start("v".into(),true).unwrap(); tasks.cancel(); finished(&tasks).await;
        assert_eq!(tasks.snapshot().state,"cancelled");
        playback.shutdown().await;
    }

    #[tokio::test]
    async fn bulk_queue_append_keeps_session_order_and_is_atomic_on_failure() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/fixtures/audio/stereo_lr_check.mp3");
        db.with_conn(|conn| {
            for id in ["a", "b", "empty"] {
                conn.execute("INSERT INTO works(id,title,created_at,updated_at) VALUES(?1,?1,'now','now')", [id])?;
            }
            for (id, work, format) in [("a-wav", "a", "wav"), ("a-mp3", "a", "mp3"), ("b-mp3", "b", "mp3")] {
                conn.execute("INSERT INTO tracks(id,work_id,title,logical_dir_path,sort_key,created_at) VALUES(?1,?2,?1,?3,?1,'now')", params![id, work, format])?;
                conn.execute("INSERT INTO track_versions(id,track_id,version_label,duration_ms,created_at) VALUES(?1,?1,?1,24000,'now')", [id])?;
                conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES(?1,?1,'local_imported',?2,?3,'available','now','now')", params![id, fixture.to_string_lossy(), format])?;
            }
            Ok(())
        }).unwrap();
        let original = db.append_queue_items(&[("a-mp3".into(), None)]).unwrap().remove(0);
        let playback = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        {
            let mut state = playback.inner.lock();
            state.current_queue_item_id = Some(original.clone());
            state.current_version_id = Some("a-mp3".into());
            state.state = "paused".into();
            state.position_ms = 2345;
        }
        let snapshot = playback.append_works_to_queue(vec!["b".into(), "a".into(), "b".into()]).await.unwrap();
        assert_eq!(snapshot.state, "paused");
        assert_eq!(snapshot.position_ms, 2345);
        assert_eq!(snapshot.current_version_id.as_deref(), Some("a-mp3"));
        let queue = db.get_queue_items().unwrap();
        assert_eq!(queue[0].id, original);
        assert_eq!(queue.iter().map(|item| item.version_id.as_str()).collect::<Vec<_>>(), vec!["a-mp3", "b-mp3", "a-wav"]);
        assert!(playback.append_works_to_queue(vec!["a".into(), "empty".into()]).await.is_err());
        assert_eq!(db.get_queue_items().unwrap().len(), 3);
        assert!(playback.append_works_to_queue(vec![]).await.is_err());
        assert!(playback.append_works_to_queue(vec!["a".into(); 501]).await.is_err());
        playback.shutdown().await;
    }

    #[tokio::test]
    async fn paused_session_restores_audio_info_without_loading_media_and_rejects_changed_sources() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now');
                INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','Track','1','now');
                INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES('v','t','Original','now');
                INSERT INTO media_sources(id,version_id,source_kind,format,byte_size,created_at,updated_at)
                VALUES('s','v','online_provider','flac',1000,'now','now');")?;
            Ok(())
        }).unwrap();
        db.append_queue_items(&[("v".into(), None)]).unwrap();
        let player = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        {
            let mut state = player.inner.lock();
            state.active_source = state.alternative_sources.first().cloned();
            state.audio_telemetry.codec = Some("flac".into());
            state.audio_telemetry.sample_rate_hz = Some(96000);
            state.audio_telemetry.channels = Some("stereo".into());
            state.audio_telemetry.bitrate_kbps = Some(1200);
            state.audio_telemetry.source_sample_rate_hz = Some(44100);
            state.audio_telemetry.source_bitrate_kbps = Some(900);
        }
        let generation = player.inner.lock().session_generation;
        player.apply_source_bit_depth(generation, "s", Some(24));
        player.apply_source_bit_depth(generation.wrapping_sub(1), "s", Some(16));
        player.apply_source_bit_depth(generation, "different-source", Some(32));
        assert_eq!(player.get_snapshot().unwrap().audio_telemetry.source_bit_depth, Some(24));
        player.try_persist_current_state(false).unwrap();
        player.shutdown().await;
        let restored = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        let snapshot = restored.get_snapshot().unwrap();
        assert_eq!(snapshot.state, "paused");
        assert_eq!(snapshot.audio_telemetry.sample_rate_hz, Some(96000));
        assert_eq!(snapshot.audio_telemetry.channels.as_deref(), Some("stereo"));
        assert_eq!(snapshot.audio_telemetry.bitrate_kbps, Some(1200));
        assert_eq!(snapshot.audio_telemetry.source_bit_depth, Some(24));
        assert_eq!(snapshot.audio_telemetry.source_sample_rate_hz, Some(44100));
        assert_eq!(snapshot.audio_telemetry.source_bitrate_kbps, Some(900));
        assert!(!restored.inner.lock().file_loaded_for_gen);
        assert!(snapshot.active_source.is_none());
        restored.shutdown().await;
        db.with_conn(|conn| { conn.execute("UPDATE media_sources SET byte_size=2000 WHERE id='s'", [])?; Ok(()) }).unwrap();
        let changed = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        assert!(changed.get_snapshot().unwrap().audio_telemetry.sample_rate_hz.is_none());
        assert!(changed.get_snapshot().unwrap().audio_telemetry.source_bit_depth.is_none());
        assert!(changed.get_snapshot().unwrap().audio_telemetry.source_sample_rate_hz.is_none());
        assert!(changed.get_snapshot().unwrap().audio_telemetry.source_bitrate_kbps.is_none());
        assert_eq!(changed.get_snapshot().unwrap().audio_telemetry.codec.as_deref(), Some("FLAC"));
        changed.shutdown().await;
    }

    #[tokio::test]
    async fn source_depth_playback_reports_24_bit_file_even_when_decoder_outputs_s32() {
        let root = tempdir().unwrap();
        let audio = root.path().join("24bit.wav");
        let data_len = 48000u32 * 2 * 3;
        let mut wav = b"RIFF".to_vec();
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(48000u32.to_le_bytes());
        wav.extend(data_len.to_le_bytes());
        wav.extend(6u16.to_le_bytes());
        wav.extend(24u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        wav.resize(wav.len() + data_len as usize, 0); // Synthetic silence only.
        std::fs::write(&audio, wav).unwrap();
        let db = Database::open_and_migrate(&root.path().join("d"), &root.path().join("c")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','test','now','now');
                INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','test','1','now');
                INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');")?;
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES('s','v','local_imported',?1,'wav','now','now')", [audio.to_string_lossy()])?;
            Ok(())
        }).unwrap();
        let player = PlaybackService::new(db.clone(), AsmrOneProvider::new(db));
        player.inner.lock().volume = 0.0;
        player.mpv.command(serde_json::json!(["set_property", "ao", "null"])).await.unwrap();
        player.mpv.command(serde_json::json!(["set_property", "audio-samplerate", "24000"])).await.unwrap();
        player.play("v", None).await.unwrap();
        let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = player.get_snapshot().unwrap();
                let output_rate = player.mpv.command(serde_json::json!(["get_property", "audio-out-params/samplerate"])).await.ok().and_then(|v| v.as_i64());
                if snapshot.audio_telemetry.source_bit_depth.is_some() && snapshot.audio_telemetry.sample_format.is_some() && snapshot.audio_telemetry.source_sample_rate_hz.is_some() && output_rate == Some(24000) { break snapshot; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        let last = player.get_snapshot().unwrap();
        player.shutdown().await;
        let snapshot = snapshot.unwrap_or_else(|_| panic!("state={} depth={:?} format={:?} error={:?}", last.state, last.audio_telemetry.source_bit_depth, last.audio_telemetry.sample_format, last.last_error));
        assert_eq!(snapshot.audio_telemetry.source_bit_depth, Some(24));
        assert_eq!(snapshot.audio_telemetry.sample_format.as_deref(), Some("s32"));
        assert_eq!(snapshot.audio_telemetry.source_sample_rate_hz, Some(48000));
        assert_eq!(snapshot.audio_telemetry.source_bitrate_kbps, Some(2304));
    }

    #[tokio::test]
    async fn repeated_shutdown_releases_worker_owned_playback_state() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        for cycle in 0..30 {
            let player = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
            let state = Arc::downgrade(&player.inner);
            if cycle < 3 { player.prewarm_engine().await.unwrap(); }
            player.shutdown().await;
            drop(player);
            tokio::time::timeout(Duration::from_secs(2), async {
                while state.upgrade().is_some() { tokio::task::yield_now().await; }
            }).await.expect("shutdown must release both event and sleep timer workers");
        }
    }

    #[tokio::test]
    async fn queue_plan_restore_preserves_session_and_steps_through_duplicate_versions() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3");
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now');
                INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','Track','1','now');")?;
            for version in ["old", "original", "edit"] {
                conn.execute("INSERT INTO track_versions(id,track_id,version_label,duration_ms,created_at) VALUES(?1,'t',?1,24000,'now')", [version])?;
                conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES(?1,?1,'local_imported',?2,'mp3','now','now')", params![version,fixture.to_string_lossy()])?;
            }
            Ok(())
        }).unwrap();
        db.append_queue_items(&[("original".into(),None),("original".into(),None),("edit".into(),None)]).unwrap();
        let plan = db.save_queue_plan("重复版本方案").unwrap();
        db.replace_queue_items(&[("old".into(),None)],0).unwrap();
        let player = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        player.play("old",None).await.unwrap();
        let before = player.get_snapshot().unwrap();
        let preview = db.preview_queue_plan(&plan.id).unwrap();
        let request = ApplyQueuePlanRequest { plan_id:plan.id, mode:"replace".into(),skip_unavailable:false,
            preview_token:preview.preview_token,queue_token:preview.queue_token };
        let restored = player.apply_queue_plan(&request).unwrap();
        assert_eq!(restored.current_version_id, before.current_version_id);
        assert_eq!(restored.session_generation, before.session_generation);
        assert_eq!(restored.state, before.state);
        assert!(restored.current_queue_item_id.is_none());
        player.try_persist_current_state(false).unwrap();
        let first = player.step_queue(1).await.unwrap();
        assert_eq!(first.current_queue_item_id.as_deref(),Some(restored.queue[0].id.as_str()));
        let second = player.step_queue(1).await.unwrap();
        assert_eq!(second.current_queue_item_id.as_deref(),Some(restored.queue[1].id.as_str()));
        let third = player.step_queue(1).await.unwrap();
        assert_eq!(third.current_version_id.as_deref(),Some("edit"));
        player.shutdown().await;
    }

    #[tokio::test]
    async fn audio_output_devices_validate_persist_and_restore_without_starting_playback() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let player = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        let output = player.get_audio_output().await.unwrap();
        assert_eq!(output.selected_device, "auto");
        assert!(output.devices.iter().any(|device| device.id == "auto"));
        assert!(output.devices.iter().all(|device| device.id == "auto" || device.id.starts_with("wasapi/")));
        assert!(player.set_audio_device("wasapi/not-a-real-device").await.is_err());
        assert_eq!(player.mpv.selected_audio_device(), "auto");
        assert!(db.get_setting("audio_device").unwrap().is_none());

        // Choose an actual endpoint when present; no media is loaded by this test.
        let selected = output.devices.last().unwrap().id.clone();
        player.set_audio_device(&selected).await.unwrap();
        assert_eq!(db.get_setting("audio_device").unwrap().as_deref(), Some(selected.as_str()));
        assert_eq!(player.mpv.command(serde_json::json!(["get_property", "audio-device"])).await.unwrap().as_str(), Some(selected.as_str()));
        player.shutdown().await;

        let restored = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        assert_eq!(restored.get_audio_output().await.unwrap().selected_device, selected);
        assert_eq!(restored.mpv.command(serde_json::json!(["get_property", "audio-device"])).await.unwrap().as_str(), Some(selected.as_str()));
        assert!(restored.get_snapshot().unwrap().current_version_id.is_none());
        restored.set_audio_device("auto").await.unwrap();
        assert_eq!(db.get_setting("audio_device").unwrap().as_deref(), Some("auto"));
        restored.shutdown().await;
    }

    #[tokio::test]
    async fn bookmark_position_stays_on_its_exact_version_and_generation_t1503() {
        let dir=tempdir().unwrap();
        let db=Database::open_and_migrate(&dir.path().join("d"),&dir.path().join("c")).unwrap();
        let fixture=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/fixtures/audio/stereo_lr_check.mp3");
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','Track','1','now');")?;
            for id in ["original","other-cut"] {
                conn.execute("INSERT INTO track_versions(id,track_id,version_label,duration_ms,created_at) VALUES(?1,'t',?1,24000,'now')",[id])?;
                conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES(?1,?1,'local_imported',?2,'mp3','available','now','now')",params![id,fixture.to_string_lossy()])?;
            } Ok(())
        }).unwrap();
        db.save_playback_position("original",24000,true,true).unwrap();
        let bm=db.add_bookmark("original",6000,"准确定位").unwrap();
        let player=PlaybackService::new(db.clone(),AsmrOneProvider::new(db.clone()));
        let next=player.play_bookmark(&bm.id).await.unwrap();
        assert_eq!(next.current_version_id.as_deref(),Some("original"));
        assert!(next.position_ms>=6000 && next.position_ms<8000,"bookmark overrides completed-track reset");
        player.shutdown().await;
        // Exercise ordering with the load gate, not 50/350 ms sleeps while
        // real audio keeps advancing on a heavily loaded test machine.
        let player=PlaybackService::new(db.clone(),AsmrOneProvider::new(db.clone()));
        let initial_generation=player.inner.lock().session_generation;
        let gate=player.load_operation.lock().await;
        let slow=player.clone();
        let old=tokio::spawn(async move {slow.play_with_position("original",None,false,None,Some(15000)).await});
        tokio::time::timeout(Duration::from_secs(2),async {
            while player.inner.lock().session_generation == initial_generation { tokio::task::yield_now().await; }
        }).await.unwrap();
        let newer=player.clone();
        let new=tokio::spawn(async move {newer.play("other-cut",None).await});
        tokio::time::timeout(Duration::from_secs(2),async {
            while player.inner.lock().session_generation < initial_generation+2 { tokio::task::yield_now().await; }
        }).await.unwrap();
        // Both generations exist before either may load. The old bookmark's
        // explicit seek must already have been superseded by the new version.
        assert_eq!(player.inner.lock().position_ms,0);
        assert_ne!(player.inner.lock().pending_seek_ms,Some(15000));
        drop(gate);
        let old_result=old.await.unwrap().unwrap();
        let new_result=new.await.unwrap().unwrap();
        assert_eq!(old_result.current_version_id.as_deref(),Some("other-cut"));
        assert_eq!(new_result.current_version_id.as_deref(),Some("other-cut"));
        assert_eq!(old_result.session_generation,new_result.session_generation);
        assert_ne!(player.inner.lock().pending_seek_ms,Some(15000));
        player.shutdown().await;
    }

    #[tokio::test]
    async fn completed_position_survives_persistence_and_replay_starts_at_zero_r05() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3");
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now');
                INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','Track','1','now');
                INSERT INTO track_versions(id,track_id,version_label,duration_ms,created_at) VALUES('v','t','Original',24000,'now');")?;
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at)
                VALUES('s','v','local_imported',?1,'mp3','available','now','now')", params![fixture.to_string_lossy()])?;
            Ok(())
        }).unwrap();
        let playback = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        {
            let mut state = playback.inner.lock();
            state.current_version_id = Some("v".into());
            state.position_ms = 24000;
            state.current_completed = true;
        }
        playback.persist_current_state(false);
        assert!(db.get_work_detail("w").unwrap().tracks[0].versions[0].completed);
        let replay = playback.play("v", None).await.unwrap();
        assert!(replay.position_ms < 2000, "completed replay must start at zero");
        assert!(!playback.inner.lock().current_completed);
        {
            let mut state = playback.inner.lock();
            state.state = "ended".into();
            state.current_completed = true;
            state.position_ms = 24000;
        }
        let replay = playback.toggle_pause().await.unwrap();
        assert!(replay.position_ms < 2000, "play after EOF must restart even with an active source");
        playback.prepare_for_update().await.unwrap();
        assert_eq!(playback.get_snapshot().unwrap().state, "paused");
        assert!(playback.inner.lock().active_source.is_none());
    }

    #[tokio::test]
    async fn work_resume_and_explicit_restart_keep_queue_scope_and_completion_history() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/fixtures/audio/stereo_lr_check.mp3");
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now');
                INSERT INTO collected_works(work_id,created_at) VALUES('w','now');")?;
            for (id, group) in [("1", "WAV"), ("2", "WAV"), ("3", "MP3")] {
                conn.execute("INSERT INTO tracks(id,work_id,title,logical_dir_path,sort_key,created_at) VALUES(?1,'w',?1,?2,?1,'now')", params![id,group])?;
                conn.execute("INSERT INTO track_versions(id,track_id,version_label,duration_ms,created_at) VALUES(?1,?1,'Original',24000,'now')", [id])?;
                conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES(?1,?1,'local_imported',?2,'mp3','available','now','now')", params![id,fixture.to_string_lossy()])?;
            }
            Ok(())
        }).unwrap();
        db.save_playback_position("1", 24000, true, true).unwrap();
        db.save_playback_position("2", 24000, true, true).unwrap();
        let playback = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        let replay = playback.play_work("w", None, false).await.unwrap();
        assert_eq!(replay.current_version_id.as_deref(), Some("1"));
        assert!(replay.position_ms < 2000);
        assert_eq!(db.get_queue_items().unwrap().iter().map(|q| q.version_id.as_str()).collect::<Vec<_>>(), vec!["1", "2"]);
        assert_eq!(db.list_works("all", None).unwrap()[0].listening_status, ListeningStatus::Played);
        playback.seek_ms(5000).await.unwrap();
        let resumed = playback.play_work("w", None, false).await.unwrap();
        assert_eq!(resumed.current_version_id.as_deref(), Some("1"));
        assert!(resumed.position_ms >= 4000 && resumed.position_ms < 7000);
        let restarted = playback.play_work("w", Some("WAV".into()), true).await.unwrap();
        assert_eq!(restarted.current_version_id.as_deref(), Some("1"));
        assert!(restarted.position_ms < 2000);
        assert_eq!(db.list_works("all", None).unwrap()[0].listening_status, ListeningStatus::Played);
        playback.prepare_for_update().await.unwrap();
    }

    #[tokio::test]
    async fn test_generation_race_slow_a_does_not_override_fast_b_r05() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture_mp3 = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w1', 'Race Test Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('tA', 'w1', 'Track A (Slow Online)', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('tB', 'w1', 'Track B (Fast Local)', '002', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('vA', 'tA', 'MP3', 12000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('vB', 'tB', 'MP3', 12000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, provider, source_work_id, remote_file_id, remote_relative_path, format, availability, created_at, updated_at)
                 VALUES ('msA', 'vA', 'online_provider', 'asmr_one', '1705068', '1705068/1', 'mp3/a.mp3', 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('msB', 'vB', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture_mp3],
            )?;
            Ok(())
        })
        .unwrap();

        let pb_a = playback.clone();
        let handle_a = tokio::spawn(async move {
            pb_a.play_internal("vA", None, false, Some(Duration::from_millis(350)))
                .await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        let snap_b = playback.play("vB", None).await.unwrap();
        assert_eq!(snap_b.current_version_id.as_deref(), Some("vB"));

        let _ = handle_a.await;
        let final_snap = playback.get_snapshot().unwrap();
        assert_eq!(
            final_snap.current_version_id.as_deref(),
            Some("vB"),
            "Late resolution of Track A must NEVER hijack playback from Track B (R05)"
        );

        playback.shutdown().await;
    }

    #[tokio::test]
    async fn test_sleep_timer_wake_expiration_and_ab_loop_clear_r14_r15() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture_mp3 = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w1', 'Timer Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t1', 'w1', 'Track 1', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t2', 'w1', 'Track 2', '002', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v1', 't1', 'MP3', 12000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v2', 't2', 'MP3', 12000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms1', 'v1', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture_mp3],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms2', 'v2', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture_mp3],
            )?;
            Ok(())
        })
        .unwrap();

        playback.play("v1", None).await.unwrap();
        playback
            .set_active_ab_loop(1000, 5000, Some("Loop 1".into()), true)
            .unwrap();
        assert!(playback.get_snapshot().unwrap().active_ab_loop.is_some());

        // Switching to v2 must clear active A-B loop (R15)
        playback.play("v2", None).await.unwrap();
        assert!(
            playback.get_snapshot().unwrap().active_ab_loop.is_none(),
            "Switching tracks must clear active A-B loop (R15)"
        );

        // Simulate sleep timer deadline in the past (system woke up after deadline, R14)
        {
            let mut guard = playback.inner.lock();
            guard.state = "playing".into();
            guard.sleep_timer.mode = "minutes".into();
            guard.sleep_timer.deadline_utc =
                Some((Utc::now() - chrono::Duration::seconds(10)).to_rfc3339());
        }
        let fired1 = playback.check_and_fire_sleep_timer().await;
        let fired2 = playback.check_and_fire_sleep_timer().await;
        assert!(fired1, "Expired sleep timer must fire immediately upon wake check");
        assert!(!fired2, "Sleep timer must only fire once (R14)");
        assert_eq!(playback.get_snapshot().unwrap().state, "paused");

        playback.shutdown().await;
    }

    #[tokio::test]
    async fn test_session_persistence_and_crash_recovery_r06() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider.clone());
        assert_eq!(playback.get_snapshot().unwrap().volume, 100.0);

        let fixture_mp3 = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/中文 日本語 スペース テスト_01.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w_r06', 'R06 恢复测试作品', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t_r06', 'w_r06', '01 テスト音轨', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v_r06', 't_r06', 'MP3', 24000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms_r06', 'v_r06', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture_mp3],
            )?;
            Ok(())
        })
        .unwrap();

        playback.play("v_r06", None).await.unwrap();
        playback.set_volume(160.0).await.unwrap();
        playback.seek_ms(6_500).await.unwrap();
        playback.shutdown().await;

        // Reopen PlaybackService on the same DB -> must restore queue, version_id, volume=160.0, position_ms=6500 in paused state
        let playback2 = PlaybackService::new(db.clone(), provider.clone());
        let restored = playback2.get_snapshot().unwrap();
        assert_eq!(restored.current_version_id.as_deref(), Some("v_r06"));
        assert_eq!(restored.volume, 160.0);
        assert_eq!(restored.state, "paused");
        assert!(
            (restored.position_ms - 6_500).abs() <= 2_000,
            "Restored position ({}) must be within 2s of saved 6500ms (R06)",
            restored.position_ms
        );

        // Verify engine restart recovers cleanly
        let restarted = playback2.restart_engine().await.unwrap();
        assert_eq!(restarted.current_version_id.as_deref(), Some("v_r06"));
        playback2.shutdown().await;
    }

    #[tokio::test]
    async fn test_playback_multi_cycle_endurance_and_progress_stability_t701() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture_a = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/中文 日本語 スペース テスト_01.mp3")
            .to_string_lossy()
            .to_string();
        let fixture_b = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w_soak', 'T701 连续播放稳定性作品', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at) VALUES ('t_s1', 'w_soak', '01 音轨 A', 'MP3', 'MP3', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at) VALUES ('t_s2', 'w_soak', '02 音轨 B', 'MP3', 'MP3', '002', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v_s1', 't_s1', 'MP3', 24000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v_s2', 't_s2', 'MP3', 12000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms_s1', 'v_s1', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture_a],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms_s2', 'v_s2', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture_b],
            )?;
            Ok(())
        })
        .unwrap();

        let snap0 = playback
            .play_directory_group("w_soak", "MP3", None)
            .await
            .unwrap();
        assert_eq!(snap0.queue.len(), 2);
        assert_eq!(snap0.current_version_id.as_deref(), Some("v_s1"));

        // Run 6 cycles of seek -> pause -> resume -> step next -> step prev -> verify monotonic eventSeq & DB persistence
        let mut last_seq = snap0.event_seq;
        for i in 1..=6 {
            // loadfile acceptance is not FileLoaded. Exercise pause/resume of an
            // actually loaded track; the unloaded-resume path has its own test.
            tokio::time::timeout(Duration::from_secs(3), async {
                while !playback.inner.lock().file_loaded_for_gen {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.unwrap();
            let target_ms = (i * 1_500) as i64;
            let s1 = playback.seek_ms(target_ms).await.unwrap();
            assert!(s1.event_seq > last_seq);

            let s2 = playback.toggle_pause().await.unwrap();
            assert_eq!(s2.state, "paused");
            let s3 = playback.toggle_pause().await.unwrap();
            assert_eq!(s3.state, "playing");

            let s4 = playback.step_queue(1).await.unwrap();
            assert_eq!(s4.current_version_id.as_deref(), Some("v_s2"));

            let s5 = playback.step_queue(-1).await.unwrap();
            assert_eq!(s5.current_version_id.as_deref(), Some("v_s1"));
            assert!(s5.event_seq > s1.event_seq);
            last_seq = s5.event_seq;
        }

        playback.seek_ms(8_800).await.unwrap();
        playback.shutdown().await;

        let detail = db.get_work_detail("w_soak").unwrap();
        let v1 = detail
            .tracks
            .iter()
            .find(|t| t.id == "t_s1")
            .unwrap()
            .versions
            .first()
            .unwrap();
        assert!(
            (v1.position_ms - 8_800).abs() <= 2_000,
            "Expected persisted position near 8800ms after multi-cycle soak, got {}",
            v1.position_ms
        );

        // Simulate 4-hour (14,400,000 ms = 2,880 x 5s checkpoints) continuous listening soak
        let mut simulated_pos_ms: i64 = 0;
        for step in 1..=2_880 {
            simulated_pos_ms += 5_000;
            // Every 60 checkpoints (5 minutes simulated), persist to SQLite just like periodic 5s flush
            if step % 60 == 0 || step == 2_880 {
                db.save_playback_position("v_s1", simulated_pos_ms, step == 2_880, false)
                    .unwrap();
            }
        }
        assert_eq!(simulated_pos_ms, 14_400_000);
        let detail_4h = db.get_work_detail("w_soak").unwrap();
        let v1_4h = detail_4h
            .tracks
            .iter()
            .find(|t| t.id == "t_s1")
            .unwrap()
            .versions
            .first()
            .unwrap();
        assert_eq!(v1_4h.position_ms, 14_400_000);
        assert!(v1_4h.completed);
    }

    #[test]
    fn test_playback_service_new_on_non_tokio_gui_thread() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db, provider);
        let snap = playback.get_snapshot().unwrap();
        assert_eq!(snap.state, "idle");
    }

    #[tokio::test]
    async fn restored_dsp_refresh_preserves_global_and_work_scopes_and_playback_position() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        db.with_conn(|c| { c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('restore-dsp','音效','now','now');")?; Ok(()) }).unwrap();
        let playback = PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone()));
        let global = DspConfigDto::default();
        let work = DspConfigDto { master_bypass: true, ..global.clone() };
        db.save_dsp_global_state(&global, Some("asmr_pure_guard")).unwrap();
        db.set_work_dsp_override("restore-dsp", None, &work, true).unwrap();
        {
            let mut state = playback.inner.lock();
            state.current_work_id = Some("restore-dsp".into());
            state.position_ms = 42000;
        }
        playback.reload_restored_dsp().await.unwrap();
        assert!(playback.inner.lock().dsp_config.master_bypass);
        assert_eq!(playback.inner.lock().position_ms, 42000);
        assert!(!db.get_dsp_global_state().unwrap().0.master_bypass);
        assert!(db.get_work_dsp_override("restore-dsp").unwrap().unwrap().1.master_bypass);
        db.clear_work_dsp_override("restore-dsp").unwrap();
        playback.reload_restored_dsp().await.unwrap();
        assert!(!playback.inner.lock().dsp_config.master_bypass);
        assert!(db.get_work_dsp_override("restore-dsp").unwrap().is_none());
        playback.shutdown().await;
    }

    #[tokio::test]
    async fn test_dsp_live_hot_reload_and_bypass_without_dropping_progress_r20() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w_dsp', 'R20 DSP 实时热更新作品', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at) VALUES ('t_dsp', 'w_dsp', '01 耳语测试', 'WAV', 'WAV', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v_dsp', 't_dsp', 'WAV', 15000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms_dsp', 'v_dsp', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture],
            )?;
            Ok(())
        })
        .unwrap();

        playback.play("v_dsp", None).await.unwrap();
        let s_seek = playback.seek_ms(6_400).await.unwrap();
        assert_eq!(s_seek.position_ms, 6_400);

        let normalized = playback.set_loudness_normalization(true).await.unwrap();
        assert!(normalized.dsp_config.loudness_normalization);
        assert!(normalized.audio_telemetry.active_filter_graph.contains("loudnorm"));
        assert_eq!(normalized.position_ms, 6_400);
        assert_eq!(normalized.current_version_id.as_deref(), Some("v_dsp"));
        assert!(db.get_dsp_global_state().unwrap().0.loudness_normalization);
        let restored = playback.set_loudness_normalization(false).await.unwrap();
        assert!(!restored.audio_telemetry.active_filter_graph.contains("loudnorm"));
        assert_eq!(restored.position_ms, 6_400);
        assert!(!db.get_dsp_global_state().unwrap().0.loudness_normalization);

        // Apply ASMR De-Esser preset in real time
        let dsp_state = playback.apply_dsp_preset("asmr_de_esser").await.unwrap();
        assert_eq!(dsp_state.active_preset_id.as_deref(), Some("asmr_de_esser"));
        assert!(dsp_state
            .telemetry
            .active_filter_graph
            .contains("equalizer=f=6500"));

        // Import AutoEQ ParametricEQ.txt in real time
        let autoeq_txt = "Preamp: -4.2 dB\nFilter 1: ON PK Fc 3200 Hz Gain -3.0 dB Q 2.2";
        let dsp_autoeq = playback.import_autoeq_text(autoeq_txt).await.unwrap();
        assert_eq!(dsp_autoeq.config.autoeq_bands.len(), 1);
        assert!(dsp_autoeq
            .telemetry
            .active_filter_graph
            .contains("equalizer=f=3200"));

        // Toggle Master Bypass -> bit-transparent empty filter graph without dropping playback progress
        let mut bypass_cfg = dsp_autoeq.config.clone();
        bypass_cfg.master_bypass = true;
        let dsp_bypass = playback
            .update_dsp_config(bypass_cfg, Some("bypass".into()))
            .await
            .unwrap();
        assert_eq!(dsp_bypass.telemetry.active_filter_graph, "");

        let snap_after = playback.get_snapshot().unwrap();
        assert!(
            (snap_after.position_ms - 6_400).abs() <= 1_500,
            "Playback position must be preserved across live DSP filter updates, got {}",
            snap_after.position_ms
        );
        assert_eq!(snap_after.audio_telemetry.active_filter_graph, "");

        playback.shutdown().await;
    }

    #[test]
    fn test_m9_fts5_and_single_cte_list_works_2000_works_30000_tracks_r22() {
        check_large_library(false);
    }

    #[test]
    #[ignore = "160 ms wall-clock budget; run alone with --ignored --test-threads=1"]
    fn large_library_isolated_latency_budget() {
        check_large_library(true);
    }

    fn check_large_library(enforce_latency: bool) {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();

        // Populate 2,000 works & 30,000 tracks (15 tracks per work) inside a single transaction (T901, T902, T903, R22)
        db.with_conn(|conn| {
            let tx = conn.transaction()?;
            {
                let mut w_stmt = tx.prepare_cached(
                    "INSERT INTO works (id, title, description, rj_code, circle_name, voice_actors_json, tags_json, cover_ref, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, '2026-01-01T00:00:00Z', ?8)",
                )?;
                let mut t_stmt = tx.prepare_cached(
                    "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                     VALUES (?1, ?2, ?3, 'WAV', 'WAV', ?4, '2026-01-01T00:00:00Z')",
                )?;
                let mut tv_stmt = tx.prepare_cached(
                    "INSERT INTO track_versions (id, track_id, version_label, duration_ms, association_state, created_at)
                     VALUES (?1, ?2, 'WAV', 180000, 'confirmed', '2026-01-01T00:00:00Z')",
                )?;
                let mut ms_stmt = tx.prepare_cached(
                    "INSERT INTO media_sources (id, version_id, source_kind, format, byte_size, availability, created_at, updated_at)
                     VALUES (?1, ?2, 'local_imported', 'wav', 52428800, 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                )?;
                let mut col_stmt = tx.prepare_cached(
                    "INSERT INTO collected_works (work_id, created_at) VALUES (?1, ?2)",
                )?;

                for w_idx in 1..=2000 {
                    let wid = format!("w_{:04}", w_idx);
                    let rj = format!("RJ{:08}", 1000000 + w_idx);
                    let title = if w_idx == 1500 {
                        "極上耳かきと囁き睡眠導入スペシャル".to_string()
                    } else {
                        format!("ASMR Work Title {:04}", w_idx)
                    };
                    let circle = format!("Circle {:03}", w_idx % 100);
                    let vas = if w_idx == 1500 {
                        "[\"久遠千歳\",\"陽向葵ゅか\"]"
                    } else {
                        "[\"CV_Standard\"]"
                    };
                    let tags = if w_idx == 1500 {
                        "[\"耳かき\",\"バイノーラル\"]"
                    } else {
                        "[\"ASMR\"]"
                    };
                    let updated = format!("2026-01-01T00:{:02}:{:02}Z", (w_idx / 60) % 60, w_idx % 60);

                    w_stmt.execute(params![wid, title, "desc", rj, circle, vas, tags, updated])?;
                    col_stmt.execute(params![wid, updated])?;

                    for t_idx in 1..=15 {
                        let tid = format!("t_{:04}_{:02}", w_idx, t_idx);
                        let vid = format!("v_{:04}_{:02}", w_idx, t_idx);
                        let msid = format!("ms_{:04}_{:02}", w_idx, t_idx);
                        let t_title = if w_idx == 1777 && t_idx == 8 {
                            "08_秘密の竹耳かきトラック.wav".to_string()
                        } else {
                            format!("{:02}_track.wav", t_idx)
                        };
                        let sort_k = format!("{:04}", t_idx);

                        t_stmt.execute(params![tid, wid, t_title, sort_k])?;
                        tv_stmt.execute(params![vid, tid])?;
                        ms_stmt.execute(params![msid, vid])?;
                    }
                }

                // Insert an uncollected online work: must NOT appear in "all" until collected
                w_stmt.execute(params![
                    "w_uncollected_online",
                    "未收藏在线浏览作品",
                    "desc",
                    "RJ09999999",
                    "OnlineCircle",
                    "[]",
                    "[]",
                    "2026-01-02T00:00:00Z"
                ])?;
            }
            tx.commit()?;
            Ok(())
        })
        .unwrap();

        // Index subtitle cue text for work w_1888 (T902)
        db.index_subtitle_text_for_work(
            "w_1888",
            Some("t_1888_01"),
            "今夜はゆっくりおやすみなさい 特別な炭酸シャンプー音",
        )
        .unwrap();

        // 1. Benchmark full 2,000-work / 30,000-track list_works query (T901)
        let t0 = Instant::now();
        let all_works = db.list_works("all", None).unwrap();
        let list_elapsed = t0.elapsed();
        eprintln!("T1515 list_works (2,000 works / 30,000 tracks): {list_elapsed:?}");
        assert_eq!(all_works.len(), 2000);
        assert!(
            !enforce_latency || list_elapsed < Duration::from_millis(160),
            "list_works for 2,000 works / 30,000 tracks must be ultra-fast (< 160ms in parallel debug build), took {:?}",
            list_elapsed
        );

        // Verify Collected vs Favorite separation (R32):
        // Uncollected online work does not appear in "all" or "favorites"
        assert_eq!(db.list_works("favorites", None).unwrap().len(), 0);
        // Liking it puts it in "favorites", still not in "all"
        assert!(db.toggle_favorite("w_uncollected_online").unwrap());
        let favs = db.list_works("favorites", None).unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, "w_uncollected_online");
        assert!(!favs[0].is_collected);
        assert!(favs[0].is_favorite);
        assert_eq!(db.list_works("all", None).unwrap().len(), 2000);
        // Collecting it puts it into "all" (我的音频库)
        assert!(db.toggle_collected("w_uncollected_online").unwrap());
        assert_eq!(db.list_works("all", None).unwrap().len(), 2001);
        assert!(!db.toggle_collected("w_uncollected_online").unwrap());
        assert_eq!(db.list_works("all", None).unwrap().len(), 2000);

        // 2. Benchmark FTS5 search by RJ code, voice actor, track filename, and subtitle text (T902)
        let t_rj = Instant::now();
        let by_rj = db.list_works("all", Some("RJ01001500")).unwrap();
        let rj_elapsed = t_rj.elapsed();
        eprintln!("T1515 FTS5 RJ search: {rj_elapsed:?}");
        assert_eq!(by_rj.len(), 1);
        assert_eq!(by_rj[0].id, "w_1500");
        assert!(
            !enforce_latency || rj_elapsed < Duration::from_millis(160),
            "FTS5 RJ search took {:?}",
            rj_elapsed
        );

        assert!(by_rj[0].has_local);
        // Assert SQL work, not thread scheduling: a one-work search must visit
        // that work's sources rather than aggregating the entire 30,000-track library.
        db.with_conn(|conn| {
            let measure = |scope: &str| -> rusqlite::Result<i32> {
                let mut query = conn.prepare(crate::db::repo::WORK_SOURCE_FLAGS_SQL)?;
                { let mut rows = query.query([scope])?; while rows.next()?.is_some() {} }
                Ok(query.get_status(rusqlite::StatementStatus::VmStep))
            };
            let one = measure(r#"["w_1500"]"#)?;
            let all = measure(&serde_json::to_string(&all_works.iter().map(|w| &w.id).collect::<Vec<_>>()).unwrap())?;
            assert!(one * 100 < all, "search must be scoped: one={one}, all={all}");
            Ok(())
        }).unwrap();

        let by_track = db.list_works("all", Some("秘密の竹耳かき")).unwrap();
        assert_eq!(by_track.len(), 1);
        assert_eq!(by_track[0].id, "w_1777");

        let by_sub = db.list_works("all", Some("炭酸シャンプー音")).unwrap();
        assert_eq!(by_sub.len(), 1);
        assert_eq!(by_sub[0].id, "w_1888");

        // T1515: Also exercise a populated listening history, not just empty tables.
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO playback_positions(version_id,position_ms,completed,ever_completed,updated_at)
                SELECT v.id,1000,0,CASE WHEN CAST(SUBSTR(t.work_id,3) AS INTEGER)%3=0 OR t.id LIKE '%_01' THEN 1 ELSE 0 END,'now'
                FROM track_versions v JOIN tracks t ON t.id=v.track_id;")?;
            Ok(())
        }).unwrap();
        let started = Instant::now();
        let with_history = db.list_works("all", None).unwrap();
        let history_elapsed = started.elapsed();
        eprintln!("T1515 list_works with 30,000 progress records: {history_elapsed:?}");
        assert_eq!(with_history.len(), 2000);
        let searched_history = db.list_works("all", Some("RJ01001500")).unwrap();
        assert_eq!(searched_history.len(), 1);
        assert_eq!(searched_history[0].listening_status, crate::domain::ListeningStatus::Played);
        assert!(searched_history[0].is_listened && searched_history[0].has_local);
        assert_eq!(with_history.iter().filter(|w| w.listening_status == crate::domain::ListeningStatus::Played).count(), 666);
        assert_eq!(with_history.iter().filter(|w| w.listening_status == crate::domain::ListeningStatus::InProgress).count(), 1334);
        // Additional workload: report it separately from the original empty-history
        // 160 ms list/search budgets above, and verify durable completion semantics.
    }

    #[tokio::test]
    async fn test_m9_mpv_ring_buffer_tick_and_next_track_prefetch_r22() {
        // 1. Verify T908 bounded ring buffer arguments on mpv startup
        let args = MpvController::build_startup_args(
            r"\\.\pipe\asmrplayer-mpv-test",
            Some("http://127.0.0.1:7890"),
        );
        assert!(args.contains(&"--cache=yes".to_string()));
        assert!(args.contains(&"--demuxer-max-bytes=64MiB".to_string()));
        assert!(args.contains(&"--demuxer-max-back-bytes=32MiB".to_string()));
        assert!(args.contains(&"--http-proxy=http://127.0.0.1:7890".to_string()));

        // 2. Verify T905 lightweight PlaybackTickDto monotonic sequence and T906 next-track pre-resolve
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w_pf', 'Prefetch Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            for idx in 1..=2 {
                conn.execute(
                    "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                     VALUES (?1, 'w_pf', ?2, 'WAV', 'WAV', ?3, '2026-01-01T00:00:00Z')",
                    params![format!("t_pf{}", idx), format!("Track {}", idx), format!("00{}", idx)],
                )?;
                conn.execute(
                    "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                     VALUES (?1, ?2, 'WAV', 20000, '2026-01-01T00:00:00Z')",
                    params![format!("v_pf{}", idx), format!("t_pf{}", idx)],
                )?;
                conn.execute(
                    "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                     VALUES (?1, ?2, 'local_imported', ?3, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                    params![format!("ms_pf{}", idx), format!("v_pf{}", idx), fixture],
                )?;
            }
            Ok(())
        })
        .unwrap();

        playback
            .play_directory_group("w_pf", "WAV", Some("v_pf1".into()))
            .await
            .unwrap();

        let tick1 = playback.build_tick();
        let tick2 = playback.build_tick();
        assert!(tick2.event_seq > tick1.event_seq);
        assert_eq!(tick2.session_generation, tick1.session_generation);

        // Trigger T906 next-track pre-resolve
        playback.maybe_prefetch_next_track().await;
        {
            let guard = playback.inner.lock();
            let pf = guard
                .prefetched_next
                .as_ref()
                .expect("Next track v_pf2 must be pre-resolved");
            assert_eq!(pf.0, "v_pf2");
            assert_eq!(pf.1, "ms_pf2");
            assert_eq!(pf.2, fixture);
        }

        // Stepping to next track consumes the prefetched cache
        let snap_next = playback.step_queue(1).await.unwrap();
        assert_eq!(snap_next.current_version_id.as_deref(), Some("v_pf2"));
        assert!(playback.inner.lock().prefetched_next.is_none());

        playback.shutdown().await;
    }

    #[tokio::test]
    async fn test_m10_watchdog_self_heal_and_headphone_unplug_auto_pause_t1001_t1002() {
        // 1. Verify T910 (<30ms first-audio probe flags) & T911 (hardware acceleration vs software fallback)
        let hw_args = MpvController::build_startup_args_with_hw(r"\\.\pipe\test", None, true);
        assert!(hw_args.contains(&"--vid=no".to_string()));
        assert!(hw_args.contains(&"--vd=no".to_string()));
        assert!(hw_args.contains(&"--sub=no".to_string()));
        assert!(hw_args.contains(&"--demuxer-lavf-probesize=262144".to_string()));
        assert!(hw_args.contains(&"--hwdec=auto-safe".to_string()));
        assert!(hw_args.contains(&"--ad-lavc-threads=0".to_string()));

        let sw_args = MpvController::build_startup_args_with_hw(r"\\.\pipe\test", None, false);
        assert!(sw_args.contains(&"--hwdec=no".to_string()));
        assert!(sw_args.contains(&"--ad-lavc-threads=1".to_string()));

        // 2. Verify T1001 self-healing restart_engine & T1002 headphone unplug auto-pause
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w_m10', 'M10 Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t_m10', 'w_m10', 'M10 Track', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v_m10', 't_m10', 'MP3', 20000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms_m10', 'v_m10', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture],
            )?;
            Ok(())
        })
        .unwrap();

        playback.play("v_m10", None).await.unwrap();
        playback.seek_ms(7_500).await.unwrap();

        // Simulate headphone / USB DAC unplug (T1002)
        let unplug_snap = playback
            .handle_audio_device_disconnected("检测到耳机已拔出")
            .await
            .unwrap();
        assert_eq!(
            unplug_snap.state, "paused",
            "Headphone unplug must immediately pause playback to prevent speaker blast (T1002)"
        );
        assert_eq!(unplug_snap.position_ms, 7_500);
        assert!(unplug_snap.last_error.is_some());

        // Simulate T1001 self-healing engine restart: must preserve version_id, position_ms, and DSP config
        let healed_snap = playback.restart_engine().await.unwrap();
        assert_eq!(healed_snap.current_version_id.as_deref(), Some("v_m10"));
        assert_eq!(healed_snap.position_ms, 7_500);
        assert_eq!(healed_snap.state, "paused");
        assert!(healed_snap.last_error.is_none());

        playback.shutdown().await;
    }

    #[tokio::test]
    async fn test_m11_sleep_fade_binaural_wasapi_exclusive_and_smtc_privacy_r24_r25_r26_r29_r31() {
        // R24: Logarithmic equal-loudness sleep fade multiplier
        assert_eq!(compute_sleep_fade_multiplier(45, 30), 1.0);
        assert_eq!(compute_sleep_fade_multiplier(30, 30), 1.0);
        let mid_fade = compute_sleep_fade_multiplier(15, 30);
        assert!(mid_fade > 0.05 && mid_fade < 0.35);
        assert_eq!(compute_sleep_fade_multiplier(0, 30), 0.0);

        // R25: Binaural L/R channel swap & BS2B crossfeed in lavfi filter graph
        let mut cfg = DspConfigDto::default();
        cfg.swap_channels = true;
        cfg.bs2b_enabled = true;
        let graph = build_lavfi_graph(&cfg);
        assert!(graph.contains("pan=stereo|c0=c1|c1=c0"));
        assert!(graph.contains("bs2b=fcut=700:feed=45"));
        let summary = summarize_dsp_chain(&cfg);
        assert!(summary.contains("L/R互换"));
        assert!(summary.contains("BS2B"));

        // R26, R29, R31: WASAPI exclusive mode, per-work DSP profile auto-switch, and SMTC Privacy Mask
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, circle_name, cover_ref, created_at, updated_at)
                 VALUES ('w_m11', 'Sensitive ASMR Title', 'Secret Circle', 'C:/cover.jpg', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at)
                 VALUES ('t_m11', 'w_m11', 'Sensitive Whisper Track', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v_m11', 't_m11', 'WAV', 20000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('ms_m11', 'v_m11', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture],
            )?;
            Ok(())
        })
        .unwrap();

        playback.play("v_m11", None).await.unwrap();

        // R24: Sleep timer with custom fade_out_seconds
        let snap_sleep = playback
            .set_sleep_timer_with_fade("minutes", Some(15), Some(45))
            .unwrap();
        assert_eq!(snap_sleep.sleep_timer.mode, "minutes");
        assert_eq!(snap_sleep.sleep_timer.fade_out_seconds, 45);

        // R31: SMTC full vs privacy mode
        let smtc_full = playback.get_smtc_display_state().unwrap();
        assert_eq!(smtc_full.title, "Sensitive Whisper Track");
        assert_eq!(smtc_full.artist, "Secret Circle");

        playback.set_smtc_mode("privacy").unwrap();
        let smtc_priv = playback.get_smtc_display_state().unwrap();
        assert_eq!(smtc_priv.title, "ASMR Audio Track");
        assert_eq!(smtc_priv.artist, "ASMR Player");
        assert!(
            smtc_priv.cover_path.is_none(),
            "Privacy mode must strip cover path from OS media overlay"
        );

        playback.shutdown().await;
    }

    #[tokio::test]
    async fn test_play_favorites_queue_prefers_with_se_and_wav_r37() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            // Work 1: Contains SEあり/WAV, SEあり/MP3, and SEなし/WAV groups
            conn.execute(
                "INSERT INTO works (id, title, circle_name, created_at, updated_at)
                 VALUES ('w_fav_1', 'Favorite Work 1', 'Circle A', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO favorites (work_id, created_at) VALUES ('w_fav_1', '2026-01-01T00:00:00Z')",
                [],
            )?;
            // Group 1: SEなし/WAV (should be excluded when SEあり exists)
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                 VALUES ('t1_nose_wav', 'w_fav_1', '01 耳かき (SEなし)', 'SEなし/WAV', 'SEなし/WAV', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v1_nose_wav', 't1_nose_wav', 'WAV', 20000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('s1_nose_wav', 'v1_nose_wav', 'local_imported', ?1, 'wav', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture],
            )?;

            // Group 2: SEあり/MP3 (should lose to SEあり/WAV)
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                 VALUES ('t1_se_mp3', 'w_fav_1', '01 耳かき', 'SEあり/MP3', 'SEあり/MP3', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v1_se_mp3', 't1_se_mp3', 'MP3', 20000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('s1_se_mp3', 'v1_se_mp3', 'local_imported', ?1, 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture],
            )?;

            // Group 3: SEあり/WAV (must be chosen!)
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                 VALUES ('t1_se_wav', 'w_fav_1', '01 耳かき (SEあり)', 'SEあり/WAV', 'SEあり/WAV', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v1_se_wav', 't1_se_wav', 'WAV', 20000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('s1_se_wav', 'v1_se_wav', 'local_imported', ?1, 'wav', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture],
            )?;

            // Work 2: Only MP3 available (fallback should include it in queue order)
            conn.execute(
                "INSERT INTO works (id, title, circle_name, created_at, updated_at)
                 VALUES ('w_fav_2', 'Favorite Work 2', 'Circle B', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO favorites (work_id, created_at) VALUES ('w_fav_2', '2026-01-02T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                 VALUES ('t2_mp3', 'w_fav_2', '01 囁き', 'MP3', 'MP3', '001', '2026-01-02T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v2_mp3', 't2_mp3', 'MP3', 20000, '2026-01-02T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('s2_mp3', 'v2_mp3', 'local_imported', ?1, 'mp3', 'available', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
                params![fixture],
            )?;
            Ok(())
        })
        .unwrap();

        let snap = playback
            .play_favorites_queue(Some(vec!["w_fav_1".into(), "w_fav_2".into()]))
            .await
            .unwrap();

        assert_eq!(snap.queue.len(), 2, "Must enqueue exactly 1 preferred group per work without mixing MP3/WAV or SEなし");
        assert_eq!(snap.queue[0].version_id, "v1_se_wav");
        assert_eq!(snap.queue[1].version_id, "v2_mp3");
        assert_eq!(snap.current_version_id.as_deref(), Some("v1_se_wav"));

        playback.shutdown().await;
    }

    #[tokio::test]
    async fn test_smtc_seek_timeline_and_pause_preserves_position() {
        // Section I.1 & II.1: Verify sanitize_smtc_track_title strips redundant audio/video file suffixes for WinIsland lyrics matching
        assert_eq!(sanitize_smtc_track_title("01_耳かき.mp3"), "01_耳かき");
        assert_eq!(sanitize_smtc_track_title("Track 02 (SEあり).FLAC"), "Track 02 (SEあり)");
        assert_eq!(sanitize_smtc_track_title("囁きボイス.wav.mp3"), "囁きボイス");

        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider);

        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3")
            .to_string_lossy()
            .to_string();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, rj_code, circle_name, voice_actors_json, cover_ref, created_at, updated_at)
                 VALUES ('w_smtc', 'SMTC Test Work', 'RJ01999999', 'SMTC Circle', '[\"CV A\", \"CV B\"]', 'https://example.com/cover.jpg', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at)
                 VALUES ('t_smtc', 'w_smtc', 'SMTC Track 01.flac', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v_smtc', 't_smtc', 'WAV', 20000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, absolute_path, format, availability, created_at, updated_at)
                 VALUES ('s_smtc', 'v_smtc', 'local_imported', ?1, 'wav', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![fixture],
            )?;
            Ok(())
        })
        .unwrap();

        playback.play("v_smtc", None).await.unwrap();

        // 1. Verify Seek interaction permission, DisplayUpdater metadata (Title stripped of .flac, Artist, AlbumTitle, AlbumArtist, Cover), and TimelineProperties
        let initial_smtc = playback.get_smtc_display_state().unwrap();
        assert!(initial_smtc.enabled);
        assert!(initial_smtc.is_playback_position_change_enabled);
        assert!(initial_smtc.is_play_enabled);
        assert!(initial_smtc.is_pause_enabled);
        assert!(initial_smtc.is_stop_enabled);
        assert_eq!(initial_smtc.title, "SMTC Track 01");
        assert_eq!(initial_smtc.artist, "CV A / CV B");
        assert_eq!(initial_smtc.album_title, "[RJ01999999] SMTC Test Work");
        assert_eq!(initial_smtc.album_artist, "SMTC Circle");
        assert_eq!(
            initial_smtc.cover_path.as_deref(),
            Some("https://example.com/cover.jpg")
        );
        assert_eq!(initial_smtc.start_time_ms, 0);
        assert_eq!(initial_smtc.min_seek_time_ms, 0);
        assert_eq!(initial_smtc.end_time_ms, 20000);
        assert_eq!(initial_smtc.max_seek_time_ms, 20000);
        assert!(!initial_smtc.clear_all_called);

        // 2. External Seek request (PlaybackPositionChangeRequested -> handle_smtc_action("seek"))
        let snap_after_seek = playback
            .handle_smtc_action("seek", Some(12_345))
            .await
            .unwrap();
        assert_eq!(snap_after_seek.position_ms, 12_345);
        let smtc_after_seek = playback.get_smtc_display_state().unwrap();
        assert_eq!(smtc_after_seek.position_ms, 12_345);
        assert_eq!(smtc_after_seek.end_time_ms, 20000);

        // 3. Pause guardrail: Pausing must ONLY set PlaybackStatus = Paused,
        // NEVER reset position_ms to 0, and NEVER call DisplayUpdater.ClearAll().
        let snap_paused = playback.handle_smtc_action("pause", None).await.unwrap();
        assert_eq!(snap_paused.state, "paused");
        assert_eq!(
            snap_paused.position_ms, 12_345,
            "Snapshot position_ms must not reset to 0 on pause"
        );
        let smtc_paused = playback.get_smtc_display_state().unwrap();
        assert_eq!(smtc_paused.playback_status, "Paused");
        assert_eq!(
            smtc_paused.position_ms, 12_345,
            "SMTC TimelineProperties.Position must remain at last playback timestamp on pause"
        );
        assert_eq!(smtc_paused.end_time_ms, 20000);
        assert!(
            !smtc_paused.clear_all_called,
            "DisplayUpdater.ClearAll() must NEVER be called on pause"
        );
        assert_eq!(smtc_paused.title, "SMTC Track 01");

        // 4. Stop guardrail: Stopping must set PlaybackStatus = Stopped and trigger DisplayUpdater.ClearAll().
        let snap_stopped = playback.handle_smtc_action("stop", None).await.unwrap();
        assert_eq!(snap_stopped.state, "stopped");
        let smtc_stopped = playback.get_smtc_display_state().unwrap();
        assert_eq!(smtc_stopped.playback_status, "Stopped");
        assert!(
            smtc_stopped.clear_all_called,
            "DisplayUpdater.ClearAll() must be called when track is stopped"
        );

        playback.shutdown().await;
    }
}


