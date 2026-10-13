use crate::domain::{AppError, AppResult, ErrorCode};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

pub mod amll;
pub mod libmpv_ffi;
pub mod smtc;
pub use amll::AmllBroadcastHub;
pub use smtc::NativeSmtcController;

use libmpv_ffi::{
    get_libmpv_api, mpv_event_end_file, mpv_event_property, mpv_event_start_file, mpv_handle,
    property_event_to_json, resolve_libmpv_library, MPV_END_FILE_REASON_EOF,
    MPV_END_FILE_REASON_ERROR, MPV_END_FILE_REASON_QUIT, MPV_END_FILE_REASON_REDIRECT,
    MPV_END_FILE_REASON_STOP, MPV_EVENT_END_FILE, MPV_EVENT_FILE_LOADED, MPV_EVENT_NONE,
    MPV_EVENT_PROPERTY_CHANGE, MPV_EVENT_SHUTDOWN, MPV_EVENT_START_FILE, MPV_FORMAT_DOUBLE,
    MPV_FORMAT_FLAG, MPV_FORMAT_INT64, MPV_FORMAT_NODE, MPV_FORMAT_STRING,
};

/// Maximum volume ceiling (200%) configured via `volume-max=200` on `libmpv` so quiet ASMR whispers can be amplified cleanly.
pub const MAX_MPV_VOLUME: f64 = 200.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MpvEvent {
    FileLoaded {
        playlist_entry_id: Option<i64>,
    },
    EndFile {
        reason: String,
        error: Option<String>,
        playlist_entry_id: Option<i64>,
    },
    TimePosChanged(f64),
    DurationChanged(f64),
    PauseChanged(bool),
    BufferingChanged(bool),
    AudioCodecChanged(Option<String>),
    AudioSampleRateChanged(Option<i64>),
    AudioFormatChanged(Option<String>),
    AudioChannelsChanged(Option<String>),
    AudioBitrateChanged(Option<i64>),
    AudioDeviceDisconnected(String),
    ProcessExited(String),
}

/// Resolves the pinned `libmpv` (`mpv-2.dll`) dynamic library path and verifies its companion `vulkan-1.dll`.
pub fn resolve_mpv_binary() -> AppResult<PathBuf> {
    resolve_libmpv_library()
}

struct ActiveLibMpv {
    ctx_raw: usize,
    stop_flag: Arc<AtomicBool>,
    alive_flag: Arc<AtomicBool>,
    event_thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ActiveLibMpv {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        self.alive_flag.store(false, Ordering::SeqCst);
        if self.ctx_raw != 0 {
            if let Ok(api) = get_libmpv_api() {
                let ctx = self.ctx_raw as *mut mpv_handle;
                unsafe {
                    (api.mpv_wakeup)(ctx);
                }
                if let Some(handle) = self.event_thread.take() {
                    let _ = handle.join();
                }
                unsafe {
                    (api.mpv_terminate_destroy)(ctx);
                }
            }
            self.ctx_raw = 0;
        }
    }
}

#[derive(Clone)]
pub struct MpvController {
    inner: Arc<tokio::sync::Mutex<Option<ActiveLibMpv>>>,
    event_tx: mpsc::UnboundedSender<MpvEvent>,
    proxy_url: Arc<Mutex<Option<String>>>,
    af_graph: Arc<Mutex<String>>,
    hardware_acceleration: Arc<Mutex<bool>>,
    audio_exclusive: Arc<Mutex<bool>>,
}

impl MpvController {
    pub fn new(event_tx: mpsc::UnboundedSender<MpvEvent>) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(None)),
            event_tx,
            proxy_url: Arc::new(Mutex::new(None)),
            af_graph: Arc::new(Mutex::new(String::new())),
            hardware_acceleration: Arc::new(Mutex::new(true)),
            audio_exclusive: Arc::new(Mutex::new(false)),
        }
    }

    pub fn set_proxy(&self, proxy: Option<String>) {
        *self.proxy_url.lock() = proxy;
    }

    pub fn set_initial_af_graph(&self, af_graph: String) {
        *self.af_graph.lock() = af_graph;
    }

    pub fn set_hardware_acceleration(&self, enabled: bool) {
        *self.hardware_acceleration.lock() = enabled;
    }

    pub fn set_audio_exclusive_flag(&self, exclusive: bool) {
        *self.audio_exclusive.lock() = exclusive;
    }

    pub fn is_audio_exclusive(&self) -> bool {
        *self.audio_exclusive.lock()
    }

    pub fn notify_audio_device_disconnected(&self, reason: &str) {
        let _ = self
            .event_tx
            .send(MpvEvent::AudioDeviceDisconnected(reason.to_string()));
    }

    pub fn build_startup_args(pipe_name: &str, proxy: Option<&str>) -> Vec<String> {
        Self::build_startup_args_with_hw(pipe_name, proxy, true)
    }

    pub fn build_startup_args_with_hw(
        pipe_name: &str,
        proxy: Option<&str>,
        hw_accel: bool,
    ) -> Vec<String> {
        Self::build_startup_args_full(pipe_name, proxy, hw_accel, false)
    }

    pub fn build_startup_args_full(
        pipe_name: &str,
        proxy: Option<&str>,
        hw_accel: bool,
        audio_exclusive: bool,
    ) -> Vec<String> {
        let mut args = vec![
            "--no-config".to_string(),
            "--load-scripts=no".to_string(),
            "--idle=yes".to_string(),
            "--no-video".to_string(),
            // Raise default volume ceiling to 200% for quiet ASMR tracks
            "--volume-max=200".to_string(),
            // T1103: Use Windows WASAPI output backend with optional exclusive DAC pass-through
            "--ao=wasapi".to_string(),
            if audio_exclusive {
                "--audio-exclusive=yes".to_string()
            } else {
                "--audio-exclusive=no".to_string()
            },
            // T910: Disable video/subtitle demux & shrink lavf probe window for <30ms first-audio response
            "--vid=no".to_string(),
            "--vd=no".to_string(),
            "--sub=no".to_string(),
            "--audio-display=no".to_string(),
            "--demuxer-lavf-analyzeduration=0.5".to_string(),
            "--demuxer-lavf-probesize=262144".to_string(),
            "--gapless-audio=weak".to_string(),
            "--keep-open=no".to_string(),
            // T908: 50-100MB bounded streaming ring buffer for multi-GB long ASMR WAV/FLAC files
            "--cache=yes".to_string(),
            "--demuxer-max-bytes=64MiB".to_string(),
            "--demuxer-max-back-bytes=32MiB".to_string(),
            // T911: Hardware decode & multi-core lavc audio decode vs software fallback
            if hw_accel {
                "--hwdec=auto-safe".to_string()
            } else {
                "--hwdec=no".to_string()
            },
            if hw_accel {
                "--ad-lavc-threads=0".to_string()
            } else {
                "--ad-lavc-threads=1".to_string()
            },
            format!("--input-ipc-server={}", pipe_name),
        ];
        if let Some(p) = proxy {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                args.push(format!("--http-proxy={}", trimmed));
            }
        }
        args
    }

    pub async fn ensure_started(&self) -> AppResult<()> {
        let mut guard = self.inner.lock().await;
        if let Some(active) = guard.as_ref() {
            if active.alive_flag.load(Ordering::SeqCst) {
                return Ok(());
            }
            *guard = None;
        }

        let api = get_libmpv_api()?;
        let proxy = self.proxy_url.lock().clone();
        let hw_accel = *self.hardware_acceleration.lock();
        let exclusive = *self.audio_exclusive.lock();
        let current_af = self.af_graph.lock().clone();

        let ctx = unsafe { (api.mpv_create)() };
        if ctx.is_null() {
            return Err(AppError::new(
                ErrorCode::MpvIpcError,
                "创建 libmpv 实例 (mpv_create) 失败",
            ));
        }

        let configure_res: AppResult<()> = unsafe {
            let _ = api.set_option_str(ctx, "config", "no");
            let _ = api.set_option_str(ctx, "load-scripts", "no");
            let _ = api.set_option_str(ctx, "terminal", "no");
            let _ = api.set_option_str(ctx, "msg-level", "all=no");
            let _ = api.set_option_str(ctx, "idle", "yes");
            let _ = api.set_option_str(ctx, "vid", "no");
            let _ = api.set_option_str(ctx, "vd", "no");
            let _ = api.set_option_str(ctx, "sub", "no");
            let _ = api.set_option_str(ctx, "audio-display", "no");
            let _ = api.set_option_str(ctx, "ao", "wasapi");
            let _ = api.set_option_str(
                ctx,
                "audio-exclusive",
                if exclusive { "yes" } else { "no" },
            );
            // Raise maximum volume ceiling to 200%
            let _ = api.set_option_str(ctx, "volume-max", "200");
            let _ = api.set_option_str(ctx, "demuxer-lavf-analyzeduration", "0.5");
            let _ = api.set_option_str(ctx, "demuxer-lavf-probesize", "262144");
            let _ = api.set_option_str(ctx, "gapless-audio", "weak");
            let _ = api.set_option_str(ctx, "keep-open", "no");
            let _ = api.set_option_str(ctx, "cache", "yes");
            let _ = api.set_option_str(ctx, "demuxer-max-bytes", "64MiB");
            let _ = api.set_option_str(ctx, "demuxer-max-back-bytes", "32MiB");
            let _ = api.set_option_str(
                ctx,
                "hwdec",
                if hw_accel { "auto-safe" } else { "no" },
            );
            let _ = api.set_option_str(
                ctx,
                "ad-lavc-threads",
                if hw_accel { "0" } else { "1" },
            );
            if let Some(ref p) = proxy {
                let trimmed = p.trim();
                if !trimmed.is_empty() {
                    let _ = api.set_option_str(ctx, "http-proxy", trimmed);
                }
            }

            let init_rc = (api.mpv_initialize)(ctx);
            if init_rc < 0 {
                Err(AppError::new(
                    ErrorCode::MpvIpcError,
                    format!("初始化 libmpv 失败: {}", api.error_str(init_rc)),
                ))
            } else {
                let _ = api.observe_property(ctx, 1, "time-pos", MPV_FORMAT_DOUBLE);
                let _ = api.observe_property(ctx, 2, "duration", MPV_FORMAT_DOUBLE);
                let _ = api.observe_property(ctx, 3, "pause", MPV_FORMAT_FLAG);
                let _ = api.observe_property(ctx, 4, "paused-for-cache", MPV_FORMAT_FLAG);
                let _ = api.observe_property(ctx, 5, "audio-codec-name", MPV_FORMAT_STRING);
                let _ = api.observe_property(ctx, 6, "audio-params/samplerate", MPV_FORMAT_INT64);
                let _ = api.observe_property(ctx, 7, "audio-params/format", MPV_FORMAT_STRING);
                let _ = api.observe_property(ctx, 8, "audio-params/channels", MPV_FORMAT_STRING);
                let _ = api.observe_property(ctx, 9, "audio-bitrate", MPV_FORMAT_DOUBLE);
                let _ = api.observe_property(ctx, 10, "audio-device-list", MPV_FORMAT_NODE);
                let _ = api.set_property_str(ctx, "af", &current_af);
                Ok(())
            }
        };

        if let Err(err) = configure_res {
            unsafe {
                (api.mpv_terminate_destroy)(ctx);
            }
            return Err(err);
        }

        let ctx_raw = ctx as usize;
        let stop_flag = Arc::new(AtomicBool::new(false));
        let alive_flag = Arc::new(AtomicBool::new(true));
        let stop_for_thread = stop_flag.clone();
        let alive_for_thread = alive_flag.clone();
        let event_tx = self.event_tx.clone();

        let event_thread = std::thread::Builder::new()
            .name("asmr-libmpv-events".into())
            .spawn(move || {
                let ctx_ptr = ctx_raw as *mut mpv_handle;
                let mut last_audio_device_count: Option<usize> = None;
                let mut current_playlist_entry_id: Option<i64> = None;

                while !stop_for_thread.load(Ordering::SeqCst) {
                    let ev_ptr = unsafe { (api.mpv_wait_event)(ctx_ptr, 0.25) };
                    if stop_for_thread.load(Ordering::SeqCst) || ev_ptr.is_null() {
                        break;
                    }
                    let ev = unsafe { &*ev_ptr };
                    match ev.event_id {
                        MPV_EVENT_NONE => continue,
                        MPV_EVENT_SHUTDOWN => {
                            alive_for_thread.store(false, Ordering::SeqCst);
                            let _ = event_tx.send(MpvEvent::ProcessExited("libmpv 已关闭".into()));
                            break;
                        }
                        MPV_EVENT_START_FILE => {
                            if !ev.data.is_null() {
                                let sf = unsafe { &*(ev.data as *const mpv_event_start_file) };
                                current_playlist_entry_id = Some(sf.playlist_entry_id);
                            }
                        }
                        MPV_EVENT_FILE_LOADED => {
                            let _ = event_tx.send(MpvEvent::FileLoaded {
                                playlist_entry_id: current_playlist_entry_id,
                            });
                        }
                        MPV_EVENT_END_FILE => {
                            if !ev.data.is_null() {
                                let ef = unsafe { &*(ev.data as *const mpv_event_end_file) };
                                let reason = match ef.reason {
                                    MPV_END_FILE_REASON_EOF => "eof",
                                    MPV_END_FILE_REASON_STOP => "stop",
                                    MPV_END_FILE_REASON_QUIT => "quit",
                                    MPV_END_FILE_REASON_ERROR => "error",
                                    MPV_END_FILE_REASON_REDIRECT => "redirect",
                                    _ => "unknown",
                                }
                                .to_string();
                                let error = if ef.error < 0 {
                                    Some(api.error_str(ef.error))
                                } else {
                                    None
                                };
                                let _ = event_tx.send(MpvEvent::EndFile {
                                    reason,
                                    error,
                                    playlist_entry_id: Some(ef.playlist_entry_id),
                                });
                            }
                        }
                        MPV_EVENT_PROPERTY_CHANGE => {
                            if !ev.data.is_null() {
                                let prop = unsafe { &*(ev.data as *const mpv_event_property) };
                                let (prop_name, data) = unsafe { property_event_to_json(prop) };
                                match prop_name.as_str() {
                                    "time-pos" => {
                                        if let Some(pos) = data.as_f64() {
                                            let _ = event_tx.send(MpvEvent::TimePosChanged(pos));
                                        }
                                    }
                                    "duration" => {
                                        if let Some(dur) = data.as_f64() {
                                            let _ = event_tx.send(MpvEvent::DurationChanged(dur));
                                        }
                                    }
                                    "pause" => {
                                        if let Some(p) = data.as_bool() {
                                            let _ = event_tx.send(MpvEvent::PauseChanged(p));
                                        }
                                    }
                                    "paused-for-cache" => {
                                        if let Some(b) = data.as_bool() {
                                            let _ = event_tx.send(MpvEvent::BufferingChanged(b));
                                        }
                                    }
                                    "audio-codec-name" => {
                                        let val = data.as_str().map(|s| s.to_uppercase());
                                        let _ = event_tx.send(MpvEvent::AudioCodecChanged(val));
                                    }
                                    "audio-params/samplerate" => {
                                        let val = data
                                            .as_i64()
                                            .or_else(|| data.as_f64().map(|f| f as i64));
                                        let _ = event_tx.send(MpvEvent::AudioSampleRateChanged(val));
                                    }
                                    "audio-params/format" => {
                                        let val = data.as_str().map(|s| s.to_string());
                                        let _ = event_tx.send(MpvEvent::AudioFormatChanged(val));
                                    }
                                    "audio-params/channels" => {
                                        let val = data.as_str().map(|s| s.to_string());
                                        let _ = event_tx.send(MpvEvent::AudioChannelsChanged(val));
                                    }
                                    "audio-bitrate" => {
                                        let kbps = data
                                            .as_f64()
                                            .map(|bps| (bps / 1000.0).round() as i64)
                                            .filter(|&k| k > 0);
                                        let _ = event_tx.send(MpvEvent::AudioBitrateChanged(kbps));
                                    }
                                    "audio-device-list" => {
                                        if let Some(arr) = data.as_array() {
                                            let cur_count = arr.len();
                                            if let Some(prev_count) = last_audio_device_count {
                                                if cur_count < prev_count {
                                                    let _ = event_tx.send(
                                                        MpvEvent::AudioDeviceDisconnected(
                                                            "检测到耳机或音频输出设备断开，已自动暂停播放以防外放"
                                                                .into(),
                                                        ),
                                                    );
                                                }
                                            }
                                            last_audio_device_count = Some(cur_count);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|e| {
                unsafe {
                    (api.mpv_terminate_destroy)(ctx);
                }
                AppError::new(
                    ErrorCode::MpvIpcError,
                    format!("创建 libmpv 事件监听线程失败: {}", e),
                )
            })?;

        *guard = Some(ActiveLibMpv {
            ctx_raw,
            stop_flag,
            alive_flag,
            event_thread: Some(event_thread),
        });

        Ok(())
    }

    async fn send_command_raw(&self, cmd: Value) -> AppResult<Value> {
        let guard = self.inner.lock().await;
        let active = guard
            .as_ref()
            .filter(|a| a.alive_flag.load(Ordering::SeqCst))
            .ok_or_else(|| AppError::new(ErrorCode::MpvIpcError, "libmpv 未运行"))?;
        let api = get_libmpv_api()?;
        let ctx = active.ctx_raw as *mut mpv_handle;
        unsafe { api.execute_json_command(ctx, &cmd) }
    }

    /// T910: Executes multiple libmpv commands in a single locked batch in-process.
    pub async fn send_commands_pipelined(&self, cmds: Vec<Value>) -> AppResult<Vec<Value>> {
        if cmds.is_empty() {
            return Ok(Vec::new());
        }
        let guard = self.inner.lock().await;
        let active = guard
            .as_ref()
            .filter(|a| a.alive_flag.load(Ordering::SeqCst))
            .ok_or_else(|| AppError::new(ErrorCode::MpvIpcError, "libmpv 未运行"))?;
        let api = get_libmpv_api()?;
        let ctx = active.ctx_raw as *mut mpv_handle;
        let mut results = Vec::with_capacity(cmds.len());
        for cmd in cmds {
            let res = unsafe { api.execute_json_command(ctx, &cmd)? };
            results.push(res);
        }
        Ok(results)
    }

    pub async fn command(&self, cmd: Value) -> AppResult<Value> {
        self.ensure_started().await?;
        self.send_command_raw(cmd).await
    }

    pub async fn load_file(&self, target: &str, headers: &[(String, String)], paused: bool) -> AppResult<()> {
        self.ensure_started().await?;
        let fields: Vec<String> = headers
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        self.send_commands_pipelined(vec![
            json!(["set_property", "http-header-fields", fields]),
            json!(["set_property", "pause", paused]),
            json!(["loadfile", target, "replace"]),
        ])
        .await?;
        Ok(())
    }

    /// T910: Loads a file along with volume (0..=200%) and `af` filter graph in a single in-process batch.
    pub async fn load_file_pipelined(
        &self,
        target: &str,
        headers: &[(String, String)],
        paused: bool,
        volume: f64,
        af_graph: &str,
    ) -> AppResult<()> {
        *self.af_graph.lock() = af_graph.to_string();
        self.ensure_started().await?;
        let fields: Vec<String> = headers
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        self.send_commands_pipelined(vec![
            json!(["set_property", "http-header-fields", fields]),
            json!(["set_property", "pause", paused]),
            json!(["set_property", "volume", volume.clamp(0.0, MAX_MPV_VOLUME)]),
            json!(["set_property", "af", af_graph]),
            json!(["loadfile", target, "replace"]),
        ])
        .await?;
        Ok(())
    }

    /// T802: Hot-reloads the `af` lavfi filter graph in-place without restarting `libmpv` or dropping playback position.
    pub async fn set_audio_filters(&self, af_graph: &str) -> AppResult<()> {
        *self.af_graph.lock() = af_graph.to_string();
        let is_running = {
            let guard = self.inner.lock().await;
            guard
                .as_ref()
                .map(|a| a.alive_flag.load(Ordering::SeqCst))
                .unwrap_or(false)
        };
        if is_running {
            self.send_command_raw(json!(["set_property", "af", af_graph]))
                .await?;
        }
        Ok(())
    }

    /// T1103: Toggles WASAPI Exclusive Mode (`audio-exclusive=yes/no`) on `libmpv` with automatic fallback to shared mode on hardware conflict.
    pub async fn set_audio_exclusive(&self, exclusive: bool) -> AppResult<bool> {
        *self.audio_exclusive.lock() = exclusive;
        let is_running = {
            let guard = self.inner.lock().await;
            guard
                .as_ref()
                .map(|a| a.alive_flag.load(Ordering::SeqCst))
                .unwrap_or(false)
        };
        if is_running {
            let val = if exclusive { "yes" } else { "no" };
            if let Err(err) = self
                .send_command_raw(json!(["set_property", "audio-exclusive", val]))
                .await
            {
                if exclusive {
                    *self.audio_exclusive.lock() = false;
                    let _ = self
                        .send_command_raw(json!(["set_property", "audio-exclusive", "no"]))
                        .await;
                    return Err(AppError::new(
                        ErrorCode::MpvIpcError,
                        format!("开启 WASAPI 独占模式失败，已自动安全回退至共享模式: {}", err.message),
                    )
                    .with_hint("目标声卡/DAC 可能正被其他独占程序占用或不支持当前采样率，已保持共享模式继续播放"));
                }
                return Err(err);
            }
        }
        Ok(*self.audio_exclusive.lock())
    }

    pub async fn set_pause(&self, paused: bool) -> AppResult<()> {
        self.command(json!(["set_property", "pause", paused])).await?;
        Ok(())
    }

    pub async fn seek_absolute_ms(&self, position_ms: i64) -> AppResult<()> {
        let secs = (position_ms.max(0) as f64) / 1000.0;
        self.command(json!(["seek", secs, "absolute"])).await?;
        Ok(())
    }

    pub async fn set_volume(&self, volume: f64) -> AppResult<()> {
        self.command(json!([
            "set_property",
            "volume",
            volume.clamp(0.0, MAX_MPV_VOLUME)
        ]))
        .await?;
        Ok(())
    }

    pub async fn stop(&self) -> AppResult<()> {
        let _ = self.command(json!(["stop"])).await;
        Ok(())
    }

    pub async fn shutdown(&self) {
        let active = {
            let mut guard = self.inner.lock().await;
            guard.take()
        };
        if let Some(proc) = active {
            let _ = tokio::task::spawn_blocking(move || drop(proc)).await;
        }
    }
}

/// T1202: Builds the platform-appropriate mpv IPC endpoint path:
/// - Windows: Named Pipe `\\.\pipe\asmrplayer-mpv-<session_id>`
/// - macOS / Linux (Unix): Unix Domain Socket `/tmp/asmrplayer-mpv-<session_id>.sock`
pub fn build_ipc_endpoint_for_platform(os: &str, session_id: &str) -> String {
    let clean_id: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if os.eq_ignore_ascii_case("windows") {
        format!(r"\\.\pipe\asmrplayer-mpv-{}", clean_id)
    } else {
        format!("/tmp/asmrplayer-mpv-{}.sock", clean_id)
    }
}

/// T1202: Resolves the Tauri target-triple sidecar binary filename for Windows, macOS, and Linux.
pub fn resolve_sidecar_filename_for_target(os: &str, arch: &str) -> &'static str {
    match (
        os.to_ascii_lowercase().as_str(),
        arch.to_ascii_lowercase().as_str(),
    ) {
        ("windows", "aarch64") => "mpv-aarch64-pc-windows-msvc.exe",
        ("windows", _) => "mpv-x86_64-pc-windows-msvc.exe",
        ("macos" | "darwin", "aarch64" | "arm64") => "mpv-aarch64-apple-darwin",
        ("macos" | "darwin", _) => "mpv-x86_64-apple-darwin",
        ("linux", "aarch64" | "arm64") => "mpv-aarch64-unknown-linux-gnu",
        _ => "mpv-x86_64-unknown-linux-gnu",
    }
}

/// T1202 / T1203: Normalizes local paths, Windows extended-length prefixes (`\\?\`),
/// SMB/NAS UNC shares (`\\NAS\share\...`), and WebDAV HTTP(S) URLs for `mpv loadfile`.
pub fn normalize_cross_platform_media_path(raw: &str, target_os: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("webdav://") {
        return if let Some(rest) = trimmed.strip_prefix("webdav://") {
            format!("http://{}", rest)
        } else {
            trimmed.to_string()
        };
    }
    if target_os.eq_ignore_ascii_case("windows") {
        if let Some(unc) = trimmed.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{}", unc);
        }
        return trimmed.trim_start_matches(r"\\?\").to_string();
    }
    trimmed.trim_start_matches(r"\\?\").replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_platform_ipc_endpoint_and_sidecar_resolution_t1202_r33() {
        let win_pipe = build_ipc_endpoint_for_platform("windows", "abc-123");
        assert_eq!(win_pipe, r"\\.\pipe\asmrplayer-mpv-abc-123");

        let mac_sock = build_ipc_endpoint_for_platform("macos", "abc-123");
        assert_eq!(mac_sock, "/tmp/asmrplayer-mpv-abc-123.sock");

        let linux_sock = build_ipc_endpoint_for_platform("linux", "abc-123");
        assert_eq!(linux_sock, "/tmp/asmrplayer-mpv-abc-123.sock");

        assert_eq!(
            resolve_sidecar_filename_for_target("windows", "x86_64"),
            "mpv-x86_64-pc-windows-msvc.exe"
        );
        assert_eq!(
            resolve_sidecar_filename_for_target("macos", "aarch64"),
            "mpv-aarch64-apple-darwin"
        );
        assert_eq!(
            resolve_sidecar_filename_for_target("linux", "x86_64"),
            "mpv-x86_64-unknown-linux-gnu"
        );

        assert_eq!(
            normalize_cross_platform_media_path(r"\\?\D:\ASMR\RJ01000001\01.wav", "windows"),
            r"D:\ASMR\RJ01000001\01.wav"
        );
        assert_eq!(
            normalize_cross_platform_media_path(r"\\?\UNC\NAS\ASMR\01.flac", "windows"),
            r"\\NAS\ASMR\01.flac"
        );
        assert_eq!(
            normalize_cross_platform_media_path("webdav://192.168.1.10:5005/asmr/01.wav", "linux"),
            "http://192.168.1.10:5005/asmr/01.wav"
        );
    }

    #[tokio::test]
    async fn test_in_process_libmpv_ffi_and_volume_max_200() {
        let dll_path = resolve_mpv_binary().expect("mpv-2.dll must resolve cleanly");
        assert!(
            dll_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".dll"))
                .unwrap_or(false),
            "Resolved binary must be libmpv dynamic library (.dll), got: {}",
            dll_path.display()
        );

        let args = MpvController::build_startup_args(r"\\.\pipe\test", None);
        assert!(args.contains(&"--volume-max=200".to_string()));

        let (tx, _rx) = mpsc::unbounded_channel();
        let ctrl = MpvController::new(tx);
        ctrl.ensure_started()
            .await
            .expect("In-process libmpv must initialize cleanly");

        let vol_max = ctrl
            .command(json!(["get_property", "volume-max"]))
            .await
            .expect("Must query volume-max from libmpv");
        assert_eq!(vol_max.as_f64(), Some(200.0));

        ctrl.set_volume(150.0)
            .await
            .expect("Must set volume to 150% via in-process libmpv");
        let cur_vol = ctrl
            .command(json!(["get_property", "volume"]))
            .await
            .expect("Must read back volume property");
        assert_eq!(cur_vol.as_f64(), Some(150.0));

        ctrl.shutdown().await;
    }
}
