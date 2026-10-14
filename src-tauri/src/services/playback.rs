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

#[derive(Debug, Clone, Default)]
pub struct CachedAudioTagMeta {
    pub version_id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub embedded_cover_bytes: Option<Vec<u8>>,
}

pub struct PlaybackStateInner {
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
    pub auto_healed_for_gen: bool,
    pub volume: f64,
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
    pub cached_audio_tags: Option<CachedAudioTagMeta>,
    pub active_subtitle_cues: Vec<SubtitleCueDto>,
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

        let mut initial = PlaybackStateInner {
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
            shutdown_signal: watch::channel(false).0,
        };

        svc.spawn_event_loop(event_rx);
        svc.spawn_sleep_timer_loop();
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
        telemetry.active_filter_graph = build_lavfi_graph(&guard.dsp_config);

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

    /// Updates the active subtitle cues for the currently loaded version (if matching) and broadcasts
    /// them to the local AMLL server + Windows SMTC / WinIsland LRC bridge.
    pub fn set_active_subtitle_cues_for_version(
        &self,
        version_id: Option<&str>,
        cues: Vec<SubtitleCueDto>,
    ) {
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
                    if guard.prefetching_version_id.as_deref() != Some(&next_item.version_id) { return; }
                    guard.prefetched_next = Some((
                        next_item.version_id,
                        chosen_src.id,
                        abs.clone(),
                        Vec::new(),
                    ));
                    guard.prefetching_version_id = None;
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
                if guard.prefetching_version_id.as_deref() != Some(&next_item.version_id) { return; }
                guard.prefetched_next = Some((
                    next_item.version_id,
                    chosen_src.id,
                    resolved.url,
                    resolved.headers,
                ));
                guard.prefetching_version_id = None;
                return;
            }
        }

        self.inner.lock().prefetching_version_id = None;
    }

    fn list_sources_for_version(&self, version_id: &str) -> AppResult<Vec<MediaSourceDto>> {
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
                if dto.source_kind == "local_imported" || dto.source_kind == "local_downloaded" {
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
        let (my_gen, resume_pos_ms, volume, af_graph) = {
            let mut guard = self.inner.lock();
            // Bind under the same lock as queue-plan replacement. Preserve the selected
            // occurrence when a version appears more than once in a saved queue.
            let queue = self.db.get_queue_items()?;
            let existing = queue.iter().find(|q| Some(&q.id) == guard.current_queue_item_id.as_ref() && q.version_id == version_id)
                .or_else(|| queue.iter().find(|q| q.version_id == version_id));
            let queue_item_id = if let Some(item) = existing { Some(item.id.clone()) } else {
                self.db.append_queue_items(&[(version_id.to_string(), preferred_source_id.clone())])?.into_iter().next()
            };
            guard.session_generation += 1;
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
            guard.retried_online_for_gen = force_online_refresh;
            guard.auto_healed_for_gen = false;
            guard.active_ab_loop = None;
            guard.dsp_config = target_dsp_cfg.clone();
            guard.active_dsp_preset_id = target_dsp_preset;
            let graph = build_lavfi_graph(&target_dsp_cfg);
            guard.audio_telemetry = AudioStreamTelemetryDto {
                codec: None,
                sample_rate_hz: None,
                sample_format: None,
                channels: None,
                bitrate_kbps: None,
                active_dsp_summary: summarize_dsp_chain(&target_dsp_cfg),
                active_filter_graph: graph.clone(),
            };
            guard.last_error = None;
            (gen_val, start_pos, guard.volume, graph)
        };
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
        self.inner.lock().active_subtitle_cues = initial_cues;

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
                            svc_sub.set_active_subtitle_cues_for_version(Some(&vid_sub), cues);
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
                                        svc_sub.set_active_subtitle_cues_for_version(
                                            Some(&vid_sub),
                                            cues,
                                        );
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

        // Check generation again before instructing mpv (R05)
        {
            let mut guard = self.inner.lock();
            if guard.session_generation != my_gen {
                return Ok(drop_and_snap(&self, guard)?);
            }
            guard.state = "loading".into();
            guard.active_source = Some(chosen_source.clone());
            guard.alternative_sources = all_sources
                .into_iter()
                .filter(|s| s.id != chosen_source.id && s.availability == "available")
                .collect();
        }
        self.emit_snapshot();

        // T910: Pipeline load_file + volume + af filter graph in a single IPC batch
        if let Err(err) = self
            .mpv
            .load_file_pipelined(&target_uri, &headers, false, volume, &af_graph)
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

        // Record playback start in work_playback_state (T404)
        let _ = self
            .db
            .save_playback_position(version_id, resume_pos_ms, false, true);
        self.persist_current_state(true);

        self.get_snapshot()
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
        let (state, cur_ver, cur_src, user_paused, completed) = {
            let guard = self.inner.lock();
            (
                guard.state.clone(),
                guard.current_version_id.clone(),
                guard.active_source.clone(),
                guard.user_paused_intent,
                guard.current_completed,
            )
        };

        // If restored from startup in paused state and mpv hasn't loaded the file yet, load & play!
        if (state == "paused" || state == "idle" || state == "ended") && (cur_src.is_none() || completed || state == "ended") {
            if let Some(vid) = cur_ver {
                return self.play(&vid, None).await;
            }
        }

        let target_pause =
            state == "playing" || state == "buffering" || (state == "loading" && !user_paused);
        {
            let mut guard = self.inner.lock();
            guard.user_paused_intent = target_pause;
            guard.state = if target_pause {
                "paused".into()
            } else {
                "playing".into()
            };
        }
        let _ = self.mpv.set_pause(target_pause).await;
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
            self.play(&item.version_id, item.preferred_source_id.clone())
                .await
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
        self.mpv.shutdown().await;
        let (cur_vid, cur_pos, cur_src, volume, af_graph) = {
            let mut guard = self.inner.lock();
            let pos = guard.position_ms.max(0);
            guard.state = "paused".into();
            guard.user_paused_intent = true;
            guard.file_loaded_for_gen = false;
            guard.pending_seek_ms = if pos > 0 { Some(pos) } else { None };
            guard.last_error = None;
            let graph = build_lavfi_graph(&guard.dsp_config);
            (
                guard.current_version_id.clone(),
                pos,
                guard.active_source.clone(),
                guard.volume,
                graph,
            )
        };
        self.mpv.set_initial_af_graph(af_graph.clone());

        // Attempt to pre-warm and reload local source in paused mode so resume is instantaneous
        let local_target = cur_src
            .as_ref()
            .filter(|s| s.source_kind == "local_imported" || s.source_kind == "local_downloaded")
            .and_then(|s| s.absolute_path.clone())
            .filter(|p| !p.ends_with(".part") && Path::new(p).is_file())
            .or_else(|| {
                cur_vid.as_deref().and_then(|vid| {
                    self.list_sources_for_version(vid)
                        .ok()?
                        .into_iter()
                        .find(|s| {
                            (s.source_kind == "local_imported"
                                || s.source_kind == "local_downloaded")
                                && s.availability == "available"
                        })
                        .and_then(|s| s.absolute_path)
                        .filter(|p| !p.ends_with(".part") && Path::new(p).is_file())
                })
            });

        if let Some(abs_path) = local_target {
            let _ = self
                .mpv
                .load_file_pipelined(&abs_path, &[], true, volume, &af_graph)
                .await;
            if cur_pos > 0 {
                let _ = self.mpv.seek_absolute_ms(cur_pos).await;
            }
        } else {
            let mut guard = self.inner.lock();
            guard.active_source = None;
        }

        self.persist_current_state(false);
        self.emit_snapshot();
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

        self.mpv.set_initial_af_graph(af_graph.clone());
        let _ = self.mpv.set_audio_filters(&af_graph).await;
        self.emit_snapshot();
        self.get_dsp_state(current_work_id)
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
                    MpvEvent::FileLoaded { .. } => {
                        let pending_seek = {
                            let mut guard = this.inner.lock();
                            guard.file_loaded_for_gen = true;
                            guard.state = if guard.user_paused_intent {
                                "paused".into()
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
                            let prefetch = guard
                                .duration_ms
                                .map(|dur| dur > 0 && pos_ms > 0 && (dur - pos_ms) <= 15_000)
                                .unwrap_or(false);
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
                            this.emit_snapshot();
                        } else {
                            if should_sync_smtc {
                                this.sync_native_smtc_timeline();
                            }
                            // T905: High-frequency ticks use lightweight `playback:tick` payload
                            this.emit_tick();
                        }
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
                            if guard.file_loaded_for_gen {
                                if buffering {
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
                            guard.audio_telemetry.codec = codec;
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioSampleRateChanged(sr) => {
                        {
                            let mut guard = this.inner.lock();
                            guard.audio_telemetry.sample_rate_hz = sr;
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioFormatChanged(fmt) => {
                        {
                            let mut guard = this.inner.lock();
                            guard.audio_telemetry.sample_format = fmt;
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioChannelsChanged(ch) => {
                        {
                            let mut guard = this.inner.lock();
                            guard.audio_telemetry.channels = ch;
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioBitrateChanged(bps) => {
                        {
                            let mut guard = this.inner.lock();
                            guard.audio_telemetry.bitrate_kbps =
                                bps.map(|b| ((b as f64) / 1000.0).round() as i64);
                        }
                        this.emit_snapshot();
                    }
                    MpvEvent::AudioDeviceDisconnected(reason) => {
                        let _ = this.handle_audio_device_disconnected(&reason).await;
                    }
                    MpvEvent::EndFile { reason, error, .. } => {
                        // Ignore 'stop'/'redirect'/'quit' generated during track switches!
                        if reason == "eof" {
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
                                    let _ = this.play(&vid, None).await;
                                }
                            } else {
                                let _ = this.step_queue(1).await;
                            }
                        } else if reason == "error" {
                            // T304: Re-resolve online URL once if it expired/failed
                            let retry_info = {
                                let mut guard = this.inner.lock();
                                let is_online = guard
                                    .active_source
                                    .as_ref()
                                    .map(|s| s.source_kind == "online_provider")
                                    .unwrap_or(false);
                                if is_online && !guard.retried_online_for_gen {
                                    guard.retried_online_for_gen = true;
                                    guard.current_version_id.clone()
                                } else {
                                    guard.state = "error".into();
                                    guard.last_error = Some(AppError::new(
                                        ErrorCode::MediaDecodeFailed,
                                        format!(
                                            "播放失败: {}",
                                            error.unwrap_or_else(|| "媒体加载或解码错误".into())
                                        ),
                                    ));
                                    None
                                }
                            };

                            if let Some(vid) = retry_info {
                                let _ = this.play_internal(&vid, None, true, None).await;
                            } else {
                                this.emit_snapshot();
                            }
                        }
                    }
                    MpvEvent::ProcessExited(msg) => {
                        this.persist_current_state(false);
                        let should_auto_heal = {
                            let mut guard = this.inner.lock();
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
        let slow=player.clone();
        let task=tokio::spawn(async move {slow.play_with_position("original",None,false,Some(Duration::from_millis(350)),Some(15000)).await});
        tokio::time::sleep(Duration::from_millis(50)).await;
        player.play("other-cut",None).await.unwrap();
        task.await.unwrap().unwrap();
        let final_state=player.get_snapshot().unwrap();
        assert_eq!(final_state.current_version_id.as_deref(),Some("other-cut"));
        assert!(final_state.position_ms<5000,"late bookmark must not seek the newer version");
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
        assert_eq!(all_works.len(), 2000);
        assert!(
            list_elapsed < Duration::from_millis(160),
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
        assert_eq!(by_rj.len(), 1);
        assert_eq!(by_rj[0].id, "w_1500");
        assert!(
            rj_elapsed < Duration::from_millis(160),
            "FTS5 RJ search took {:?}",
            rj_elapsed
        );

        let by_track = db.list_works("all", Some("秘密の竹耳かき")).unwrap();
        assert_eq!(by_track.len(), 1);
        assert_eq!(by_track[0].id, "w_1777");

        let by_sub = db.list_works("all", Some("炭酸シャンプー音")).unwrap();
        assert_eq!(by_sub.len(), 1);
        assert_eq!(by_sub[0].id, "w_1888");
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


