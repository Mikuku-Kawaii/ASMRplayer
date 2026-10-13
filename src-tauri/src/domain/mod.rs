use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    NetworkUnavailable,
    AuthRequired,
    RateLimited,
    SourceNotFound,
    SourceChanged,
    ProviderSchemaChanged,
    DatabaseError,
    MigrationError,
    InvalidInput,
    PathSecurityViolation,
    DiskOffline,
    DiskFull,
    FileLocked,
    MediaDecodeFailed,
    MpvMissing,
    MpvIpcError,
    EncodingUnsupported,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
#[error("[{code:?}] {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub hint: Option<String>,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let retryable = matches!(
            code,
            ErrorCode::NetworkUnavailable
                | ErrorCode::RateLimited
                | ErrorCode::MpvIpcError
                | ErrorCode::FileLocked
        );
        let msg = redact_sensitive(&message.into());
        let hint = default_hint(code);
        Self {
            code,
            message: msg,
            retryable,
            hint,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(redact_sensitive(&hint.into()));
        self
    }
}

fn default_hint(code: ErrorCode) -> Option<String> {
    match code {
        ErrorCode::NetworkUnavailable => Some("请检查网络连接、API 地址或代理设置后重试。".into()),
        ErrorCode::AuthRequired => Some("该作品或接口需要有效凭据，请在设置中配置或更新站点凭据。".into()),
        ErrorCode::RateLimited => Some("请求过于频繁，请稍候再试。".into()),
        ErrorCode::SourceNotFound => Some("未找到对应作品或媒体文件，请确认作品号或本地文件路径。".into()),
        ErrorCode::SourceChanged => Some("远端目录或文件已发生变更，请刷新并确认版本关联。".into()),
        ErrorCode::ProviderSchemaChanged => Some("在线接口返回结构发生变化，请检查接口版本或更新客户端。".into()),
        ErrorCode::MigrationError => Some("数据库迁移失败，已保留原数据库文件与备份，请查看诊断信息。".into()),
        ErrorCode::PathSecurityViolation => Some("目标路径超出允许范围或包含非法/保留名称。".into()),
        ErrorCode::DiskOffline => Some("所属磁盘或目录当前不可访问，可在设置或根目录管理中重新定位。".into()),
        ErrorCode::DiskFull => Some("目标磁盘空间不足，请清理空间或更换下载目录后恢复任务。".into()),
        ErrorCode::FileLocked => Some("文件正被其他程序占用，请关闭占用程序后重试。".into()),
        ErrorCode::MpvMissing => Some("未找到 mpv 播放引擎或依赖 DLL，请检查 src-tauri/binaries 目录或重新安装。".into()),
        ErrorCode::MpvIpcError => Some("mpv 播放进程通信中断，可点击重启播放引擎恢复。".into()),
        ErrorCode::EncodingUnsupported => Some("字幕文件非 UTF-8 编码，请先转换为 UTF-8 SRT/VTT 后重新绑定。".into()),
        _ => None,
    }
}

/// Redacts tokens, cookies, passwords, and daily signed query/path secrets from logs and error strings.
pub fn redact_sensitive(input: &str) -> String {
    let mut out = input.to_string();
    for key in ["token=", "auth=", "authorization:", "bearer ", "cookie:", "password="] {
        let lower = out.to_ascii_lowercase();
        if let Some(idx) = lower.find(key) {
            let val_start = idx + key.len();
            let val_end = out[val_start..]
                .find(|c: char| c.is_whitespace() || c == '&' || c == '"' || c == '\'' || c == ';')
                .map(|off| val_start + off)
                .unwrap_or(out.len());
            if val_end > val_start {
                out.replace_range(val_start..val_end, "[REDACTED]");
            }
        }
    }
    out
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::new(ErrorCode::DatabaseError, format!("数据库操作错误: {}", err))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        let code = match err.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::SourceNotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::FileLocked,
            _ => ErrorCode::InternalError,
        };
        AppError::new(code, format!("IO 错误: {}", err))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub page_size: u32,
    pub total_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSummary {
    pub id: String,
    pub title: String,
    pub rj_code: Option<String>,
    pub circle_name: String,
    pub voice_actors: Vec<String>,
    pub tags: Vec<String>,
    pub cover_ref: Option<String>,
    pub has_local: bool,
    pub has_online: bool,
    pub has_downloaded: bool,
    pub availability: String,
    pub is_collected: bool,
    pub is_favorite: bool,
    pub last_version_id: Option<String>,
    pub last_track_title: Option<String>,
    pub last_position_ms: Option<i64>,
    pub last_played_at: Option<String>,
    pub source_work_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkOverrideDto {
    pub field: String,
    pub override_mode: String, // "set" | "cleared"
    pub value_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSourceDto {
    pub id: String,
    pub version_id: String,
    pub source_kind: String, // "local_imported" | "local_downloaded" | "online_provider"
    pub provider: Option<String>,
    pub source_work_id: Option<String>,
    pub remote_file_id: Option<String>,
    pub remote_relative_path: Option<String>,
    pub root_id: Option<String>,
    pub relative_path: Option<String>,
    pub absolute_path: Option<String>,
    pub format: String,
    pub byte_size: Option<i64>,
    pub content_sha256: Option<String>,
    pub availability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackVersionDto {
    pub id: String,
    pub track_id: String,
    pub version_label: String,
    pub duration_ms: Option<i64>,
    pub content_fingerprint: Option<String>,
    pub association_state: String,
    pub position_ms: i64,
    pub completed: bool,
    pub sources: Vec<MediaSourceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackItemDto {
    pub id: String,
    pub work_id: String,
    pub title: String,
    pub logical_dir_path: String,
    pub group_label: String,
    pub sort_key: String,
    pub versions: Vec<TrackVersionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDto {
    pub id: String,
    pub work_id: String,
    pub version_id: Option<String>,
    pub attachment_type: String, // "cover" | "subtitle" | "text"
    pub title: String,
    pub relative_path: Option<String>,
    pub absolute_path: Option<String>,
    pub remote_file_id: Option<String>,
    pub subtitle_offset_ms: i64,
    pub encoding: String,
    pub availability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSourceInfoDto {
    pub id: String,
    pub provider: String,
    pub source_work_id: String,
    pub sync_status: String,
    pub refreshed_at: String,
    pub expires_at: Option<String>,
    pub is_expired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryGroupOption {
    pub group_path: String,
    pub label: String,
    pub track_count: usize,
    pub formats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDetailDto {
    pub id: String,
    pub title: String,
    pub description: String,
    pub rj_code: Option<String>,
    pub circle_name: String,
    pub voice_actors: Vec<String>,
    pub tags: Vec<String>,
    pub cover_ref: Option<String>,
    pub is_collected: bool,
    pub is_favorite: bool,
    pub overrides: Vec<WorkOverrideDto>,
    pub online_sources: Vec<WorkSourceInfoDto>,
    pub tracks: Vec<TrackItemDto>,
    pub attachments: Vec<AttachmentDto>,
    pub directory_groups: Vec<DirectoryGroupOption>,
    pub last_version_id: Option<String>,
    pub last_position_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRootDto {
    pub id: String,
    pub absolute_path: String,
    pub display_name: String,
    pub root_mode: String,
    pub availability: String,
    pub last_scanned_at: Option<String>,
    pub work_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidateDto {
    pub relative_work_path: String,
    pub proposed_title: String,
    pub detected_rj_code: Option<String>,
    pub audio_count: usize,
    pub subtitle_count: usize,
    pub cover_detected: bool,
    pub already_indexed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewDto {
    pub root_path: String,
    pub root_mode: String,
    pub overlapping_roots: Vec<String>,
    pub candidates: Vec<ImportCandidateDto>,
    #[serde(default)]
    pub selected_input_path: Option<String>,
    #[serde(default)]
    pub detection_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReportDto {
    pub root_id: String,
    pub works_added: usize,
    pub works_updated: usize,
    pub tracks_added: usize,
    pub corrupted_files: Vec<String>,
    pub canceled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItemDto {
    pub id: String,
    pub sort_order: i64,
    pub version_id: String,
    pub track_id: String,
    pub work_id: String,
    pub work_title: String,
    pub track_title: String,
    pub logical_dir_path: String,
    pub duration_ms: Option<i64>,
    pub position_ms: i64,
    pub completed: bool,
    pub preferred_source_id: Option<String>,
    pub has_local: bool,
    pub has_online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbLoopDto {
    pub id: String,
    pub version_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkDto {
    pub id: String,
    pub version_id: String,
    pub track_title: String,
    pub work_id: String,
    pub position_ms: i64,
    pub note: String,
    pub created_at: String,
}

fn default_fade_out_seconds() -> i64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepTimerDto {
    pub mode: String, // "off" | "minutes" | "end_of_track"
    pub deadline_utc: Option<String>,
    pub remaining_seconds: Option<i64>,
    pub target_generation: Option<u64>,
    #[serde(default = "default_fade_out_seconds")]
    pub fade_out_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCueDto {
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMatchedSubtitleDto {
    pub attachment: AttachmentDto,
    pub cues: Vec<SubtitleCueDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PeqBandDto {
    pub enabled: bool,
    pub filter_type: String, // "PK" | "LSC" | "HSC"
    pub frequency_hz: f64,
    pub gain_db: f64,
    pub q_factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DspConfigDto {
    pub master_bypass: bool,
    // Stage 1: Preamp & Headroom Compensation (-15.0 ~ +6.0 dB)
    pub user_preamp_db: f64,
    pub autoeq_preamp_db: f64,
    // Stage 2: ASMR Sub-Bass Rumble Cut (40-60 Hz, 12 dB/oct Butterworth Q=0.7071)
    pub hpf_enabled: bool,
    pub hpf_cutoff_hz: f64,
    // Stage 3: AutoEQ Headphone Calibration PEQ
    pub autoeq_enabled: bool,
    pub autoeq_name: Option<String>,
    pub autoeq_bands: Vec<PeqBandDto>,
    // Stage 4: ASMR Sibilance & Harshness Suppressor (5-8 kHz Narrow-Q De-Esser)
    pub de_esser_enabled: bool,
    pub de_esser_freq_hz: f64,
    pub de_esser_q: f64,
    pub de_esser_gain_db: f64,
    // Stage 5: ASMR Whisper & Breath Air Enhancer (10-14 kHz High-Shelf Q=0.7071)
    pub air_enabled: bool,
    pub air_freq_hz: f64,
    pub air_gain_db: f64,
    // Stage 6: Binaural Channel Control (L/R Balance Trim, Swap L/R, BS2B Crossfeed — T1102)
    pub balance_db: f64,
    #[serde(default)]
    pub swap_channels: bool,
    #[serde(default)]
    pub bs2b_enabled: bool,
    // Stage 7: Anti-Ear-Blast Lookahead Soft Peak Limiter (3-5 ms lookahead, -1.0 dBFS ceiling)
    pub limiter_enabled: bool,
    pub limiter_threshold_db: f64,
    pub limiter_lookahead_ms: f64,
    pub limiter_release_ms: f64,
}

impl Default for DspConfigDto {
    fn default() -> Self {
        // Default to "asmr_pure_guard": 50Hz HPF + -1.0 dBFS 4ms Lookahead Soft Limiter
        Self {
            master_bypass: false,
            user_preamp_db: 0.0,
            autoeq_preamp_db: 0.0,
            hpf_enabled: true,
            hpf_cutoff_hz: 50.0,
            autoeq_enabled: false,
            autoeq_name: None,
            autoeq_bands: Vec::new(),
            de_esser_enabled: false,
            de_esser_freq_hz: 6500.0,
            de_esser_q: 4.0,
            de_esser_gain_db: -3.5,
            air_enabled: false,
            air_freq_hz: 12000.0,
            air_gain_db: 1.5,
            balance_db: 0.0,
            swap_channels: false,
            bs2b_enabled: false,
            limiter_enabled: true,
            limiter_threshold_db: -1.0,
            limiter_lookahead_ms: 4.0,
            limiter_release_ms: 80.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DspPresetDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub config: DspConfigDto,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamTelemetryDto {
    pub codec: Option<String>,
    pub sample_rate_hz: Option<i64>,
    pub sample_format: Option<String>,
    pub channels: Option<String>,
    pub bitrate_kbps: Option<i64>,
    pub active_dsp_summary: String,
    pub active_filter_graph: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DspStateDto {
    pub config: DspConfigDto,
    pub active_preset_id: Option<String>,
    pub work_override_enabled: bool,
    pub work_override_work_id: Option<String>,
    pub presets: Vec<DspPresetDto>,
    pub telemetry: AudioStreamTelemetryDto,
}

pub fn builtin_dsp_presets() -> Vec<DspPresetDto> {
    vec![
        DspPresetDto {
            id: "bypass".into(),
            name: "原声直通 (Bit-Perfect)".into(),
            description: "不做任何滤波与压限，100% 还原双声道录音原貌".into(),
            config: DspConfigDto {
                master_bypass: true,
                hpf_enabled: false,
                de_esser_enabled: false,
                air_enabled: false,
                limiter_enabled: false,
                ..DspConfigDto::default()
            },
            is_builtin: true,
        },
        DspPresetDto {
            id: "asmr_pure_guard".into(),
            name: "纯净防炸耳 (推荐日常)".into(),
            description: "滤除 50Hz 以下麦克风震动并开启 -1.0 dBFS 4ms 前瞻防炸耳保护，不改变人声音色".into(),
            config: DspConfigDto::default(),
            is_builtin: true,
        },
        DspPresetDto {
            id: "asmr_de_esser".into(),
            name: "柔和去齿音 (口腔音/掏耳)".into(),
            description: "针对高刺激咀嚼音、舌音、竹签敲击，精准衰减 6.5 kHz 尖锐瞬态，降低听觉疲劳".into(),
            config: DspConfigDto {
                master_bypass: false,
                hpf_enabled: true,
                hpf_cutoff_hz: 55.0,
                de_esser_enabled: true,
                de_esser_freq_hz: 6500.0,
                de_esser_q: 4.0,
                de_esser_gain_db: -4.5,
                air_enabled: false,
                limiter_enabled: true,
                limiter_threshold_db: -1.5,
                limiter_lookahead_ms: 4.0,
                limiter_release_ms: 80.0,
                ..DspConfigDto::default()
            },
            is_builtin: true,
        },
        DspPresetDto {
            id: "asmr_whisper_sleep".into(),
            name: "贴耳助眠耳语 (夜间深度)".into(),
            description: "增强 12 kHz 呼吸空气感、柔化 6.5 kHz 齿音，并将防炸耳天花板收紧至 -3.0 dBFS 安心入睡".into(),
            config: DspConfigDto {
                master_bypass: false,
                hpf_enabled: true,
                hpf_cutoff_hz: 50.0,
                de_esser_enabled: true,
                de_esser_freq_hz: 6500.0,
                de_esser_q: 4.0,
                de_esser_gain_db: -3.0,
                air_enabled: true,
                air_freq_hz: 12000.0,
                air_gain_db: 1.5,
                limiter_enabled: true,
                limiter_threshold_db: -3.0,
                limiter_lookahead_ms: 4.5,
                limiter_release_ms: 90.0,
                ..DspConfigDto::default()
            },
            is_builtin: true,
        },
    ]
}

/// Validates and clamps/rejects out-of-range DSP parameters to prevent Biquad instability (T803, T804, T805).
pub fn validate_dsp_config(cfg: &mut DspConfigDto) -> AppResult<()> {
    if !cfg.user_preamp_db.is_finite() || cfg.user_preamp_db < -15.0 || cfg.user_preamp_db > 6.0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "前置增益 (Preamp) 超出安全范围 [-15.0 dB, +6.0 dB]",
        ));
    }
    if !cfg.autoeq_preamp_db.is_finite()
        || cfg.autoeq_preamp_db < -24.0
        || cfg.autoeq_preamp_db > 12.0
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "AutoEQ Preamp 超出安全范围 [-24.0 dB, +12.0 dB]",
        ));
    }
    if !cfg.hpf_cutoff_hz.is_finite() || cfg.hpf_cutoff_hz < 20.0 || cfg.hpf_cutoff_hz > 100.0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "超低频截断频率超出安全范围 [20 Hz, 100 Hz]（推荐 40–60 Hz）",
        ));
    }
    if !cfg.de_esser_freq_hz.is_finite()
        || cfg.de_esser_freq_hz < 5000.0
        || cfg.de_esser_freq_hz > 8000.0
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "齿音压制中心频率必须在 [5000 Hz, 8000 Hz] 范围内",
        ));
    }
    if !cfg.de_esser_q.is_finite() || cfg.de_esser_q < 2.5 || cfg.de_esser_q > 8.0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "齿音压制窄带 Q 值必须在 [2.5, 8.0] 范围内",
        ));
    }
    if !cfg.de_esser_gain_db.is_finite()
        || cfg.de_esser_gain_db < -8.0
        || cfg.de_esser_gain_db > 0.0
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "齿音压制只能进行负向衰减 [-8.0 dB, 0.0 dB]，严禁输入正增益",
        ));
    }
    if !cfg.air_freq_hz.is_finite() || cfg.air_freq_hz < 10000.0 || cfg.air_freq_hz > 14000.0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "空气感增强转折频率必须在 [10000 Hz, 14000 Hz] 范围内",
        ));
    }
    if !cfg.air_gain_db.is_finite() || cfg.air_gain_db < 0.0 || cfg.air_gain_db > 3.0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "空气感增强增益必须在 [0.0 dB, +3.0 dB] 范围内",
        ));
    }
    if !cfg.balance_db.is_finite() || cfg.balance_db < -6.0 || cfg.balance_db > 6.0 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "左右耳平衡微调必须在 [-6.0 dB, +6.0 dB] 范围内",
        ));
    }
    if !cfg.limiter_threshold_db.is_finite()
        || cfg.limiter_threshold_db < -12.0
        || cfg.limiter_threshold_db > -0.5
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "防炸耳限制器天花板阈值必须在 [-12.0 dBFS, -0.5 dBFS] 范围内",
        ));
    }
    if !cfg.limiter_lookahead_ms.is_finite()
        || cfg.limiter_lookahead_ms < 3.0
        || cfg.limiter_lookahead_ms > 5.0
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "防炸耳限制器前瞻时间必须在 [3.0 ms, 5.0 ms] 范围内",
        ));
    }
    if !cfg.limiter_release_ms.is_finite()
        || cfg.limiter_release_ms < 50.0
        || cfg.limiter_release_ms > 150.0
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "防炸耳限制器释放时间必须在 [50.0 ms, 150.0 ms] 范围内",
        ));
    }

    for (idx, band) in cfg.autoeq_bands.iter().enumerate() {
        if !band.frequency_hz.is_finite()
            || band.frequency_hz < 20.0
            || band.frequency_hz > 20000.0
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!("AutoEQ 第 {} 段频率超出范围 [20 Hz, 20000 Hz]", idx + 1),
            ));
        }
        if !band.gain_db.is_finite() || band.gain_db < -24.0 || band.gain_db > 24.0 {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!("AutoEQ 第 {} 段增益超出范围 [-24 dB, +24 dB]", idx + 1),
            ));
        }
        if !band.q_factor.is_finite() || band.q_factor < 0.1 || band.q_factor > 20.0 {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!("AutoEQ 第 {} 段 Q 值超出范围 [0.1, 20.0]", idx + 1),
            ));
        }
    }
    Ok(())
}

/// Parses EqualizerAPO / AutoEQ `ParametricEQ.txt` text into `(preamp_db, Vec<PeqBandDto>)` (T804).
pub fn parse_autoeq_text(input: &str) -> AppResult<(f64, Vec<PeqBandDto>)> {
    let mut preamp_db: f64 = 0.0;
    let mut found_preamp = false;
    let mut bands = Vec::new();

    for (line_idx, raw_line) in input.lines().enumerate() {
        let line_no = line_idx + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        let lower = line.to_ascii_lowercase();
        if lower.starts_with("preamp:") {
            let rest = line["Preamp:".len()..].trim();
            let num_str = rest
                .trim_end_matches(|c: char| c.is_whitespace() || c.eq_ignore_ascii_case(&'d') || c.eq_ignore_ascii_case(&'b'))
                .trim();
            let val: f64 = num_str.parse().map_err(|_| {
                AppError::new(
                    ErrorCode::InvalidInput,
                    format!("第 {} 行 Preamp 数值格式无效: {}", line_no, line),
                )
            })?;
            if !val.is_finite() || !(-24.0..=12.0).contains(&val) {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    format!(
                        "第 {} 行 Preamp ({} dB) 超出安全范围 [-24.0 dB, +12.0 dB]",
                        line_no, val
                    ),
                ));
            }
            preamp_db = val;
            found_preamp = true;
            continue;
        }

        if lower.starts_with("filter") {
            let colon_pos = line.find(':').ok_or_else(|| {
                AppError::new(
                    ErrorCode::InvalidInput,
                    format!("第 {} 行 Filter 缺少冒号分隔符: {}", line_no, line),
                )
            })?;
            let tokens: Vec<&str> = line[colon_pos + 1..].split_whitespace().collect();
            if tokens.len() < 9 {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    format!(
                        "第 {} 行 Filter 语法不完整（期望 ON/OFF <PK|LSC|HSC> Fc <Hz> Hz Gain <dB> dB Q <Q>）: {}",
                        line_no, line
                    ),
                ));
            }

            let state_upper = tokens[0].to_ascii_uppercase();
            let enabled = match state_upper.as_str() {
                "ON" => true,
                "OFF" => false,
                _ => {
                    return Err(AppError::new(
                        ErrorCode::InvalidInput,
                        format!("第 {} 行 Filter 开关状态无效 ({})", line_no, tokens[0]),
                    ))
                }
            };

            let raw_type = tokens[1].to_ascii_uppercase();
            let filter_type = match raw_type.as_str() {
                "PK" | "PEQ" => "PK".to_string(),
                "LSC" | "LS" => "LSC".to_string(),
                "HSC" | "HS" => "HSC".to_string(),
                _ => {
                    return Err(AppError::new(
                        ErrorCode::InvalidInput,
                        format!(
                            "第 {} 行不支持的滤波器类型 {}（仅支持 PK / LSC / HSC）",
                            line_no, tokens[1]
                        ),
                    ))
                }
            };

            // Locate Fc, Gain, Q tokens
            let mut fc_opt: Option<f64> = None;
            let mut gain_opt: Option<f64> = None;
            let mut q_opt: Option<f64> = None;

            let mut i = 2;
            while i < tokens.len() {
                let t_up = tokens[i].to_ascii_uppercase();
                if t_up == "FC" && i + 1 < tokens.len() {
                    let num = tokens[i + 1].trim_end_matches(|c: char| c.is_ascii_alphabetic());
                    fc_opt = Some(num.parse().map_err(|_| {
                        AppError::new(
                            ErrorCode::InvalidInput,
                            format!("第 {} 行 Fc 频率数值无效", line_no),
                        )
                    })?);
                    i += 2;
                } else if t_up == "GAIN" && i + 1 < tokens.len() {
                    let num = tokens[i + 1].trim_end_matches(|c: char| c.is_ascii_alphabetic());
                    gain_opt = Some(num.parse().map_err(|_| {
                        AppError::new(
                            ErrorCode::InvalidInput,
                            format!("第 {} 行 Gain 增益数值无效", line_no),
                        )
                    })?);
                    i += 2;
                } else if t_up == "Q" && i + 1 < tokens.len() {
                    q_opt = Some(tokens[i + 1].parse().map_err(|_| {
                        AppError::new(
                            ErrorCode::InvalidInput,
                            format!("第 {} 行 Q 值无效", line_no),
                        )
                    })?);
                    i += 2;
                } else {
                    i += 1;
                }
            }

            let frequency_hz = fc_opt.ok_or_else(|| {
                AppError::new(ErrorCode::InvalidInput, format!("第 {} 行缺少 Fc 频率", line_no))
            })?;
            let gain_db = gain_opt.ok_or_else(|| {
                AppError::new(ErrorCode::InvalidInput, format!("第 {} 行缺少 Gain 增益", line_no))
            })?;
            let q_factor = q_opt.ok_or_else(|| {
                AppError::new(ErrorCode::InvalidInput, format!("第 {} 行缺少 Q 值", line_no))
            })?;

            if !frequency_hz.is_finite() || !(20.0..=20000.0).contains(&frequency_hz) {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    format!(
                        "第 {} 行频率 ({} Hz) 超出安全边界 [20 Hz, 20000 Hz]",
                        line_no, frequency_hz
                    ),
                ));
            }
            if !gain_db.is_finite() || !(-24.0..=24.0).contains(&gain_db) {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    format!(
                        "第 {} 行增益 ({} dB) 超出安全边界 [-24 dB, +24 dB]",
                        line_no, gain_db
                    ),
                ));
            }
            if !q_factor.is_finite() || !(0.1..=20.0).contains(&q_factor) {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    format!("第 {} 行 Q 值 ({}) 超出安全边界 [0.1, 20.0]", line_no, q_factor),
                ));
            }

            bands.push(PeqBandDto {
                enabled,
                filter_type,
                frequency_hz,
                gain_db,
                q_factor,
            });
            continue;
        }

        return Err(AppError::new(
            ErrorCode::InvalidInput,
            format!("第 {} 行无法识别的 AutoEQ 语法: {}", line_no, line),
        ));
    }

    if !found_preamp && bands.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "未解析到任何有效的 Preamp 或 Filter 配置行",
        ));
    }

    Ok((preamp_db, bands))
}

/// Computes the effective Stage 1 Preamp (including Auto Headroom compensation for positive Air shelf or AutoEQ bands).
pub fn compute_effective_preamp_db(cfg: &DspConfigDto) -> f64 {
    let autoeq_pre = if cfg.autoeq_enabled {
        cfg.autoeq_preamp_db
    } else {
        0.0
    };
    let air_headroom = if cfg.air_enabled && cfg.air_gain_db > 0.0 {
        cfg.air_gain_db
    } else {
        0.0
    };
    (autoeq_pre - air_headroom + cfg.user_preamp_db).clamp(-24.0, 12.0)
}

/// Builds the deterministic 8-stage `mpv` `lavfi=[...]` audio filter graph string (T802, T803, T804, T805).
/// Returns `""` when `master_bypass == true` or no DSP stage is active (100% Bit-Perfect passthrough).
pub fn build_lavfi_graph(cfg: &DspConfigDto) -> String {
    if cfg.master_bypass {
        return String::new();
    }

    let mut stages: Vec<String> = Vec::new();

    // Stage 1: Preamp & Headroom Compensation
    let effective_preamp = compute_effective_preamp_db(cfg);
    if effective_preamp.abs() >= 0.01 {
        stages.push(format!("volume={:.2}dB", effective_preamp));
    }

    // Stage 2: ASMR Sub-Bass Rumble Cut (2nd-Order Butterworth High-Pass, 12 dB/oct, Q = 0.7071)
    if cfg.hpf_enabled {
        stages.push(format!(
            "highpass=f={:.1}:poles=2:width_type=q:width=0.7071",
            cfg.hpf_cutoff_hz.clamp(20.0, 100.0)
        ));
    }

    // Stage 3: AutoEQ Headphone Calibration PEQ
    if cfg.autoeq_enabled {
        for band in &cfg.autoeq_bands {
            if !band.enabled || band.gain_db.abs() < 0.01 {
                continue;
            }
            let fc = band.frequency_hz.clamp(20.0, 20000.0);
            let q = band.q_factor.clamp(0.1, 20.0);
            let g = band.gain_db.clamp(-24.0, 24.0);
            let node = match band.filter_type.as_str() {
                "LSC" | "LS" => format!("lowshelf=f={:.1}:width_type=q:width={:.2}:g={:.2}", fc, q, g),
                "HSC" | "HS" => format!("highshelf=f={:.1}:width_type=q:width={:.2}:g={:.2}", fc, q, g),
                _ => format!("equalizer=f={:.1}:width_type=q:width={:.2}:g={:.2}", fc, q, g),
            };
            stages.push(node);
        }
    }

    // Stage 4: ASMR Sibilance & Harshness Suppressor (Narrow-Q Peaking Attenuation / De-Esser)
    if cfg.de_esser_enabled && cfg.de_esser_gain_db < -0.01 {
        stages.push(format!(
            "equalizer=f={:.0}:width_type=q:width={:.2}:g={:.2}",
            cfg.de_esser_freq_hz.clamp(5000.0, 8000.0),
            cfg.de_esser_q.clamp(2.5, 8.0),
            cfg.de_esser_gain_db.clamp(-8.0, 0.0)
        ));
    }

    // Stage 5: ASMR Whisper & Breath Air Enhancer (High-Shelf Filter, Q = 0.7071)
    if cfg.air_enabled && cfg.air_gain_db > 0.01 {
        stages.push(format!(
            "highshelf=f={:.0}:width_type=q:width=0.7071:g={:.2}",
            cfg.air_freq_hz.clamp(10000.0, 14000.0),
            cfg.air_gain_db.clamp(0.0, 3.0)
        ));
    }

    // Stage 6: Binaural Channel Control (L/R Swap, Balance Trim, BS2B Crossfeed — T1102)
    if cfg.swap_channels && cfg.balance_db.abs() >= 0.05 {
        let bal = cfg.balance_db.clamp(-6.0, 6.0);
        if bal < 0.0 {
            let r_gain = 10.0_f64.powf(bal / 20.0);
            stages.push(format!("pan=stereo|c0=1.0*c1|c1={:.4}*c0", r_gain));
        } else {
            let l_gain = 10.0_f64.powf(-bal / 20.0);
            stages.push(format!("pan=stereo|c0={:.4}*c1|c1=1.0*c0", l_gain));
        }
    } else if cfg.swap_channels {
        stages.push("pan=stereo|c0=c1|c1=c0".to_string());
    } else if cfg.balance_db.abs() >= 0.05 {
        let bal = cfg.balance_db.clamp(-6.0, 6.0);
        if bal < 0.0 {
            let r_gain = 10.0_f64.powf(bal / 20.0);
            stages.push(format!("pan=stereo|c0=1.0*c0|c1={:.4}*c1", r_gain));
        } else {
            let l_gain = 10.0_f64.powf(-bal / 20.0);
            stages.push(format!("pan=stereo|c0={:.4}*c0|c1=1.0*c1", l_gain));
        }
    }

    if cfg.bs2b_enabled {
        stages.push("bs2b=fcut=700:feed=45".to_string());
    }

    // Stage 7: Anti-Ear-Blast Lookahead Soft Peak Limiter (stereo-linked, auto-level=0)
    if cfg.limiter_enabled {
        let thresh_db = cfg.limiter_threshold_db.clamp(-12.0, -0.5);
        let limit_linear = 10.0_f64.powf(thresh_db / 20.0).clamp(0.0625, 1.0);
        let attack_ms = cfg.limiter_lookahead_ms.clamp(3.0, 5.0);
        let release_ms = cfg.limiter_release_ms.clamp(50.0, 150.0);
        stages.push(format!(
            "alimiter=level_in=1.0:level_out=1.0:limit={:.6}:attack={:.1}:release={:.1}:asc=1:level=0",
            limit_linear, attack_ms, release_ms
        ));
    }

    if stages.is_empty() {
        String::new()
    } else {
        format!("lavfi=[{}]", stages.join(","))
    }
}

/// Generates a concise human-readable summary of the active DSP chain for telemetry readout (T806, T1102).
pub fn summarize_dsp_chain(cfg: &DspConfigDto) -> String {
    if cfg.master_bypass {
        return "原声直通 (Bit-Perfect)".into();
    }
    let mut parts = Vec::new();
    if cfg.hpf_enabled {
        parts.push(format!("HPF {:.0}Hz", cfg.hpf_cutoff_hz));
    }
    if cfg.autoeq_enabled {
        let active_count = cfg.autoeq_bands.iter().filter(|b| b.enabled).count();
        if active_count > 0 {
            if let Some(ref name) = cfg.autoeq_name {
                parts.push(format!("AutoEQ ({})", name));
            } else {
                parts.push(format!("AutoEQ {}段", active_count));
            }
        }
    }
    if cfg.de_esser_enabled && cfg.de_esser_gain_db < -0.01 {
        parts.push(format!(
            "去齿音 {:.1}kHz ({:.1}dB)",
            cfg.de_esser_freq_hz / 1000.0,
            cfg.de_esser_gain_db
        ));
    }
    if cfg.air_enabled && cfg.air_gain_db > 0.01 {
        parts.push(format!(
            "空气感 {:.0}kHz (+{:.1}dB)",
            cfg.air_freq_hz / 1000.0,
            cfg.air_gain_db
        ));
    }
    if cfg.swap_channels {
        parts.push("L/R互换".into());
    }
    if cfg.bs2b_enabled {
        parts.push("BS2B".into());
    }
    if cfg.balance_db.abs() >= 0.05 {
        if cfg.balance_db < 0.0 {
            parts.push(format!("左偏 {:.1}dB", cfg.balance_db.abs()));
        } else {
            parts.push(format!("右偏 +{:.1}dB", cfg.balance_db));
        }
    }
    if cfg.limiter_enabled {
        parts.push(format!("防炸耳 {:.1}dBFS", cfg.limiter_threshold_db));
    }

    if parts.is_empty() {
        "原声直通 (Bit-Perfect)".into()
    } else {
        parts.join(" · ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAcousticMetricsDto {
    pub version_id: String,
    pub integrated_lufs: f64,
    pub true_peak_dbtp: f64,
    pub loudness_range_lra: f64,
    pub dynamic_range_dr: f64,
    pub waveform_peaks: Vec<f32>,
}

fn default_smtc_mode() -> String {
    "full".into()
}

/// Strips redundant local audio/video file extensions (e.g. `.mp3`, `.flac`, `.wav`, `.m4a`)
/// from a track title so external SMTC hosts (such as WinIsland) can accurately match lyrics by title.
pub fn sanitize_smtc_track_title(raw: &str) -> String {
    const MEDIA_EXTENSIONS: &[&str] = &[
        ".mp3", ".flac", ".wav", ".m4a", ".ogg", ".opus", ".aac", ".wma", ".ape",
        ".alac", ".aiff", ".aif", ".dsf", ".dff", ".wv", ".mka", ".weba", ".mp4",
        ".mkv", ".webm", ".avi", ".mov", ".m4v", ".ts",
    ];
    let mut cleaned = raw.trim();
    loop {
        let lower = cleaned.to_ascii_lowercase();
        let mut matched_ext_len = None;
        for ext in MEDIA_EXTENSIONS {
            if lower.ends_with(ext) && cleaned.len() > ext.len() {
                matched_ext_len = Some(ext.len());
                break;
            }
        }
        if let Some(ext_len) = matched_ext_len {
            let candidate = cleaned[..cleaned.len() - ext_len].trim_end();
            if !candidate.is_empty() {
                cleaned = candidate;
                continue;
            }
        }
        break;
    }
    cleaned.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SmtcDisplayStateDto {
    pub enabled: bool,
    pub mode: String, // "full" | "privacy" | "disabled"
    pub playback_status: String, // "Playing" | "Paused" | "Stopped" | "Closed"
    pub title: String,
    pub artist: String,
    pub album_title: String,
    pub album_artist: String,
    pub cover_path: Option<String>,
    #[serde(skip)]
    pub cover_bytes: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub video_subtitle: Option<String>,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub start_time_ms: i64,
    pub min_seek_time_ms: i64,
    pub end_time_ms: i64,
    pub max_seek_time_ms: i64,
    pub is_play_enabled: bool,
    pub is_pause_enabled: bool,
    pub is_stop_enabled: bool,
    pub is_next_enabled: bool,
    pub is_previous_enabled: bool,
    pub is_playback_position_change_enabled: bool,
    pub clear_all_called: bool,
}

/// T1101: Computes equal-loudness logarithmic fade-out multiplier in `[0.0, 1.0]`
/// for the last `fade_out_seconds` before sleep timer expiration.
pub fn compute_sleep_fade_multiplier(remaining_seconds: i64, fade_out_seconds: i64) -> f64 {
    if fade_out_seconds <= 0 || remaining_seconds >= fade_out_seconds {
        return 1.0;
    }
    if remaining_seconds <= 0 {
        return 0.0;
    }
    let t = (remaining_seconds as f64) / (fade_out_seconds as f64);
    // Equal-loudness psychoacoustic logarithmic attenuation curve (-40 dB floor smoothly tapered to 0)
    let log_curve = (10.0_f64.powf((t - 1.0) * 1.8) - 10.0_f64.powf(-1.8))
        / (1.0 - 10.0_f64.powf(-1.8));
    (log_curve * t.sqrt()).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub event_seq: u64,
    pub session_generation: u64,
    pub state: String, // "idle" | "resolving" | "loading" | "playing" | "paused" | "buffering" | "ended" | "error"
    pub current_queue_item_id: Option<String>,
    pub current_work_id: Option<String>,
    pub current_work_title: Option<String>,
    pub current_track_id: Option<String>,
    pub current_track_title: Option<String>,
    pub current_version_id: Option<String>,
    pub active_source: Option<MediaSourceDto>,
    pub alternative_sources: Vec<MediaSourceDto>,
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    pub volume: f64,
    pub loop_mode: String, // "none" | "single" | "queue"
    pub active_ab_loop: Option<AbLoopDto>,
    pub sleep_timer: SleepTimerDto,
    pub dsp_config: DspConfigDto,
    pub active_dsp_preset_id: Option<String>,
    #[serde(default)]
    pub audio_exclusive: bool,
    #[serde(default = "default_smtc_mode")]
    pub smtc_mode: String,
    pub audio_telemetry: AudioStreamTelemetryDto,
    pub last_error: Option<AppError>,
    pub queue: Vec<QueueItemDto>,
}

/// T905: Lightweight tick payload emitted on high-frequency `TimePosChanged` events
/// to avoid serializing the full `queue` and `alternative_sources` on every 200ms tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTickDto {
    pub event_seq: u64,
    pub session_generation: u64,
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    pub state: String,
    pub sleep_remaining_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPreviewItemDto {
    pub source_id: String,
    pub version_id: String,
    pub track_title: String,
    pub format: String,
    pub byte_size: Option<i64>,
    pub target_relative_path: String,
    pub already_downloaded: bool,
    pub already_queued: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPreviewDto {
    pub download_root: String,
    pub work_id: String,
    pub work_title: String,
    pub items: Vec<DownloadPreviewItemDto>,
    pub total_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTaskDto {
    pub id: String,
    pub work_id: String,
    pub work_title: String,
    pub version_id: String,
    pub track_title: String,
    pub source_id: String,
    pub status: String,
    pub target_relative_path: String,
    pub downloaded_bytes: i64,
    pub total_bytes: Option<i64>,
    pub speed_bps: i64,
    pub integrity_mode: String,
    pub retry_count: i64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub updated_at: String,
}

fn default_true() -> bool {
    true
}

fn default_realtime_watch() -> bool {
    true
}

fn default_toast_duration() -> f64 {
    3.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandPlacementDto {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsDto {
    pub download_dir: String,
    pub api_base_url: String,
    pub proxy_url: Option<String>,
    pub close_behavior: String, // "minimize_to_tray" | "quit"
    pub hardware_acceleration: bool,
    #[serde(default)]
    pub audio_exclusive: bool,
    #[serde(default = "default_smtc_mode")]
    pub smtc_mode: String,
    #[serde(default = "default_realtime_watch")]
    pub realtime_watch_enabled: bool,
    #[serde(default)]
    pub webdav_url: Option<String>,
    #[serde(default)]
    pub nsfw_enabled: bool,
    #[serde(default = "default_true")]
    pub island_fullscreen_avoidance: bool,
    #[serde(default = "default_true")]
    pub island_edge_snap: bool,
    #[serde(default = "default_true")]
    pub island_always_on_top: bool,
    #[serde(default = "default_true")]
    pub island_scroll_volume: bool,
    #[serde(default = "default_true")]
    pub island_toast_enabled: bool,
    #[serde(default = "default_toast_duration")]
    pub island_toast_duration_sec: f64,
    #[serde(default)]
    pub island_privacy_mode: bool,
    pub has_auth_token: bool,
    pub mpv_available: bool,
    pub mpv_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFavoriteRecordDto {
    pub work_id: String,
    pub rj_code: Option<String>,
    pub work_title: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPositionRecordDto {
    pub version_id: String,
    pub rj_code: Option<String>,
    pub track_title: String,
    pub version_label: String,
    pub position_ms: i64,
    pub completed: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBookmarkRecordDto {
    pub id: String,
    pub version_id: String,
    pub rj_code: Option<String>,
    pub track_title: String,
    pub position_ms: i64,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupAbLoopRecordDto {
    pub id: String,
    pub version_id: String,
    pub rj_code: Option<String>,
    pub track_title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupWorkOverrideRecordDto {
    pub work_id: String,
    pub rj_code: Option<String>,
    pub field: String,
    pub override_mode: String,
    pub value_text: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupWorkDspOverrideRecordDto {
    pub work_id: String,
    pub rj_code: Option<String>,
    pub preset_id: Option<String>,
    pub config: DspConfigDto,
    pub enabled: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBackupBundleDto {
    pub schema_version: u32,
    pub exported_at: String,
    pub favorites: Vec<BackupFavoriteRecordDto>,
    pub playback_positions: Vec<BackupPositionRecordDto>,
    pub bookmarks: Vec<BackupBookmarkRecordDto>,
    pub ab_loops: Vec<BackupAbLoopRecordDto>,
    pub work_overrides: Vec<BackupWorkOverrideRecordDto>,
    pub custom_dsp_presets: Vec<DspPresetDto>,
    pub work_dsp_overrides: Vec<BackupWorkDspOverrideRecordDto>,
    pub dsp_global_config: DspConfigDto,
    pub dsp_global_preset_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreReportDto {
    pub favorites_restored: usize,
    pub positions_restored: usize,
    pub bookmarks_restored: usize,
    pub ab_loops_restored: usize,
    pub overrides_restored: usize,
    pub dsp_presets_restored: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationCandidateDto {
    pub local_work_id: String,
    pub local_title: String,
    pub detected_rj_code: Option<String>,
    pub online_source_work_id: String,
    pub online_title: String,
    pub online_rj_code: Option<String>,
    pub confidence_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationHistoryDto {
    pub id: String,
    pub entity_type: String,
    pub primary_id: String,
    pub secondary_id: String,
    pub conflict_note: Option<String>,
    pub undone: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedSyncEnvelopeDto {
    pub version: u32,
    pub kdf_rounds: u32,
    pub salt_hex: String,
    pub nonce_hex: String,
    pub ciphertext_hex: String,
    pub mac_hex: String,
}

fn derive_sync_keys(passphrase: &str, salt: &[u8], rounds: u32) -> ([u8; 32], [u8; 32]) {
    use sha2::{Digest, Sha256};
    let mut state = [0u8; 32];
    let mut hasher = Sha256::new();
    hasher.update(b"asmrplayer-sync-kdf-v1:");
    hasher.update(salt);
    hasher.update(passphrase.as_bytes());
    state.copy_from_slice(&hasher.finalize());

    for round in 0..rounds.max(1) {
        let mut h = Sha256::new();
        h.update(state);
        h.update(salt);
        h.update(round.to_le_bytes());
        h.update(passphrase.as_bytes());
        state.copy_from_slice(&h.finalize());
    }

    let mut enc_hasher = Sha256::new();
    enc_hasher.update(b"enc-key:");
    enc_hasher.update(state);
    let mut enc_key = [0u8; 32];
    enc_key.copy_from_slice(&enc_hasher.finalize());

    let mut mac_hasher = Sha256::new();
    mac_hasher.update(b"mac-key:");
    mac_hasher.update(state);
    let mut mac_key = [0u8; 32];
    mac_key.copy_from_slice(&mac_hasher.finalize());

    (enc_key, mac_key)
}

fn apply_sha256_ctr_stream(enc_key: &[u8; 32], nonce: &[u8], data: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut out = Vec::with_capacity(data.len());
    for (block_idx, chunk) in data.chunks(32).enumerate() {
        let mut h = Sha256::new();
        h.update(b"asmrplayer-ctr-v1:");
        h.update(enc_key);
        h.update(nonce);
        h.update((block_idx as u64).to_le_bytes());
        let keystream = h.finalize();
        for (b, k) in chunk.iter().zip(keystream.iter()) {
            out.push(b ^ k);
        }
    }
    out
}

fn compute_sync_mac(mac_key: &[u8; 32], salt: &[u8], nonce: &[u8], ciphertext: &[u8], rounds: u32) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut inner = Sha256::new();
    inner.update(b"asmrplayer-mac-inner:");
    inner.update(mac_key);
    inner.update(rounds.to_le_bytes());
    inner.update(salt);
    inner.update(nonce);
    inner.update(ciphertext);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(b"asmrplayer-mac-outer:");
    outer.update(mac_key);
    outer.update(inner_digest);
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&outer.finalize());
    tag
}

/// T1203: Encrypts a `UserBackupBundleDto` with a user passphrase into a portable authenticated `EncryptedSyncEnvelopeDto`.
pub fn encrypt_backup_bundle(
    bundle: &UserBackupBundleDto,
    passphrase: &str,
) -> AppResult<EncryptedSyncEnvelopeDto> {
    if passphrase.trim().len() < 4 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "加密同步口令长度不能少于 4 个字符",
        ));
    }
    let plaintext = serde_json::to_vec(bundle).map_err(|e| {
        AppError::new(
            ErrorCode::InternalError,
            format!("序列化同步数据包失败: {}", e),
        )
    })?;
    let salt = uuid::Uuid::new_v4().into_bytes();
    let nonce = uuid::Uuid::new_v4().into_bytes();
    let kdf_rounds = 2048;
    let (enc_key, mac_key) = derive_sync_keys(passphrase.trim(), &salt, kdf_rounds);
    let ciphertext = apply_sha256_ctr_stream(&enc_key, &nonce, &plaintext);
    let mac = compute_sync_mac(&mac_key, &salt, &nonce, &ciphertext, kdf_rounds);

    Ok(EncryptedSyncEnvelopeDto {
        version: 1,
        kdf_rounds,
        salt_hex: hex::encode(salt),
        nonce_hex: hex::encode(nonce),
        ciphertext_hex: hex::encode(ciphertext),
        mac_hex: hex::encode(mac),
    })
}

/// T1203: Verifies MAC and decrypts an `EncryptedSyncEnvelopeDto` back into `UserBackupBundleDto`.
pub fn decrypt_backup_bundle(
    envelope: &EncryptedSyncEnvelopeDto,
    passphrase: &str,
) -> AppResult<UserBackupBundleDto> {
    if passphrase.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "解密口令不能为空",
        ));
    }
    let salt = hex::decode(&envelope.salt_hex).map_err(|_| {
        AppError::new(ErrorCode::InvalidInput, "加密同步包 salt 格式损坏")
    })?;
    let nonce = hex::decode(&envelope.nonce_hex).map_err(|_| {
        AppError::new(ErrorCode::InvalidInput, "加密同步包 nonce 格式损坏")
    })?;
    let ciphertext = hex::decode(&envelope.ciphertext_hex).map_err(|_| {
        AppError::new(ErrorCode::InvalidInput, "加密同步包密文格式损坏")
    })?;
    let expected_mac = hex::decode(&envelope.mac_hex).map_err(|_| {
        AppError::new(ErrorCode::InvalidInput, "加密同步包校验标签格式损坏")
    })?;

    let kdf_rounds = envelope.kdf_rounds.clamp(1, 65536);
    let (enc_key, mac_key) = derive_sync_keys(passphrase.trim(), &salt, kdf_rounds);
    let actual_mac = compute_sync_mac(&mac_key, &salt, &nonce, &ciphertext, kdf_rounds);

    if actual_mac.as_slice() != expected_mac.as_slice() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "加密同步包口令错误或数据已被篡改（MAC 校验未通过）",
        ));
    }

    let plaintext = apply_sha256_ctr_stream(&enc_key, &nonce, &ciphertext);
    serde_json::from_slice(&plaintext).map_err(|e| {
        AppError::new(
            ErrorCode::InvalidInput,
            format!("解析解密后的同步数据失败: {}", e),
        )
    })
}

pub fn spawn_bg_task<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
    } else {
        tauri::async_runtime::spawn(future);
    }
}

/// Descriptor for a physical display monitor in virtual desktop coordinates (T1119 / R39).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDescriptorDto {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

/// Persisted window placement including target monitor identity and physical bounds (T1119 / R39).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowPlacementDto {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    #[serde(default)]
    pub monitor_name: Option<String>,
    #[serde(default)]
    pub monitor_x: Option<i32>,
    #[serde(default)]
    pub monitor_y: Option<i32>,
    #[serde(default)]
    pub monitor_width: Option<u32>,
    #[serde(default)]
    pub monitor_height: Option<u32>,
}

/// Checks whether a window's physical outer bounds represent a valid normal (non-minimized, non-mini-mode)
/// window state suitable for overwriting normal window size/position coordinates (T1119 / R39).
pub fn is_valid_normal_window_bounds(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    is_minimized: bool,
) -> bool {
    !is_minimized && x > -25_000 && y > -25_000 && width >= 800 && height >= 520
}

/// Resolves the window placement to apply on application startup across multi-monitor setups (T1119 / R39).
///
/// Guarantees:
/// 1. If the window was closed on a secondary monitor (`副屏`, whether at positive coordinates to the right
///    or negative coordinates to the left/top), it restores onto that exact secondary monitor.
/// 2. If the window was maximized on the secondary monitor, `x` and `y` are clamped inside the secondary
///    monitor's interior so `set_position` followed by `maximize()` maximizes on the secondary monitor.
/// 3. If the secondary monitor was unplugged since last close, safely falls back and centers on the
///    primary/first available monitor so the window is never stranded off-screen.
pub fn resolve_window_restore_placement(
    saved: &WindowPlacementDto,
    monitors: &[MonitorDescriptorDto],
) -> WindowPlacementDto {
    let default_w = saved.width.max(960);
    let default_h = saved.height.max(640);

    if monitors.is_empty() {
        return WindowPlacementDto {
            x: saved.x.max(-16_000),
            y: saved.y.max(-16_000),
            width: default_w,
            height: default_h,
            maximized: saved.maximized,
            monitor_name: saved.monitor_name.clone(),
            monitor_x: saved.monitor_x,
            monitor_y: saved.monitor_y,
            monitor_width: saved.monitor_width,
            monitor_height: saved.monitor_height,
        };
    }

    let center_x = saved.x.saturating_add((default_w as i32) / 2);
    let center_y = saved.y.saturating_add((default_h as i32) / 2);

    // 1. Exact monitor name + origin match
    let by_name_and_origin = monitors.iter().find(|m| {
        saved.monitor_name.is_some()
            && m.name == saved.monitor_name
            && saved.monitor_x == Some(m.x)
            && saved.monitor_y == Some(m.y)
    });

    // 2. Monitor containing the saved window's center point
    let by_center = monitors.iter().find(|m| {
        let right = m.x.saturating_add(m.width as i32);
        let bottom = m.y.saturating_add(m.height as i32);
        center_x >= m.x && center_x < right && center_y >= m.y && center_y < bottom
    });

    // 3. Monitor matching the saved monitor origin (e.g. secondary screen at x=1920 or x=-1920)
    let by_origin = monitors.iter().find(|m| {
        saved.monitor_x == Some(m.x) && saved.monitor_y == Some(m.y)
    });

    // 4. Monitor matching saved monitor name even if virtual desktop coordinates shifted
    let by_name_only = monitors.iter().find(|m| {
        saved.monitor_name.is_some() && m.name == saved.monitor_name
    });

    let matched_monitor = by_name_and_origin
        .or(by_center)
        .or(by_origin)
        .or(by_name_only);

    let (target_mon, is_matched) = match matched_monitor {
        Some(m) => (m, true),
        None => (&monitors[0], false),
    };

    let mon_w = target_mon.width.max(960);
    let mon_h = target_mon.height.max(640);
    let width = default_w.min(mon_w);
    let height = default_h.min(mon_h);

    let (x, y) = if is_matched {
        // Translate relative to monitor origin if the monitor's virtual position shifted
        let rel_x = match saved.monitor_x {
            Some(orig_mx) if orig_mx != target_mon.x => saved.x.saturating_sub(orig_mx),
            _ => saved.x.saturating_sub(target_mon.x),
        };
        let rel_y = match saved.monitor_y {
            Some(orig_my) if orig_my != target_mon.y => saved.y.saturating_sub(orig_my),
            _ => saved.y.saturating_sub(target_mon.y),
        };
        let raw_x = target_mon.x.saturating_add(rel_x);
        let raw_y = target_mon.y.saturating_add(rel_y);

        let max_x = target_mon
            .x
            .saturating_add((mon_w as i32).saturating_sub(width as i32).max(0));
        let max_y = target_mon
            .y
            .saturating_add((mon_h as i32).saturating_sub(height as i32).max(0));

        // If maximized, ensure the anchor point is strictly inside the target monitor's interior
        // so Windows `MonitorFromWindow` / `maximize()` targets the secondary monitor accurately.
        let clamped_x = raw_x.clamp(target_mon.x, max_x);
        let clamped_y = raw_y.clamp(target_mon.y, max_y);
        if saved.maximized {
            let inner_x = clamped_x.max(target_mon.x.saturating_add(24)).min(max_x);
            let inner_y = clamped_y.max(target_mon.y.saturating_add(24)).min(max_y);
            (inner_x, inner_y)
        } else {
            (clamped_x, clamped_y)
        }
    } else {
        // Secondary monitor was disconnected -> center cleanly on primary fallback monitor
        let centered_x = target_mon
            .x
            .saturating_add(((mon_w as i32).saturating_sub(width as i32) / 2).max(0));
        let centered_y = target_mon
            .y
            .saturating_add(((mon_h as i32).saturating_sub(height as i32) / 2).max(0));
        (centered_x, centered_y)
    };

    WindowPlacementDto {
        x,
        y,
        width,
        height,
        maximized: saved.maximized,
        monitor_name: target_mon.name.clone(),
        monitor_x: Some(target_mon.x),
        monitor_y: Some(target_mon.y),
        monitor_width: Some(target_mon.width),
        monitor_height: Some(target_mon.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_sensitive_removes_tokens_and_passwords() {
        let raw = "Request failed: https://example.com/api?token=secret12345&page=1 Authorization: Bearer my_jwt_token_abc";
        let clean = redact_sensitive(raw);
        assert!(!clean.contains("secret12345"));
        assert!(!clean.contains("my_jwt_token_abc"));
        assert!(clean.contains("[REDACTED]"));
    }

    #[test]
    fn test_dsp_filter_graph_and_autoeq_parser_r18_r19() {
        // 1. Bypass mode -> af="" (Bit-Perfect passthrough)
        let mut bypass_cfg = DspConfigDto::default();
        bypass_cfg.master_bypass = true;
        assert_eq!(build_lavfi_graph(&bypass_cfg), "");

        // 2. AutoEQ parser with Preamp, LSC, PK, HSC, and OFF filter
        let autoeq_txt = r#"
            # Sennheiser HD 650 Harman Target
            Preamp: -6.4 dB
            Filter 1: ON LSC Fc 105 Hz Gain 6.6 dB Q 0.70
            Filter 2: ON PK Fc 1450 Hz Gain -2.4 dB Q 1.80
            Filter 3: OFF PK Fc 3000 Hz Gain 1.5 dB Q 2.00
            Filter 4: ON HSC Fc 10000 Hz Gain -3.2 dB Q 0.70
        "#;
        let (preamp, bands) = parse_autoeq_text(autoeq_txt).unwrap();
        assert!((preamp - (-6.4)).abs() < 1e-6);
        assert_eq!(bands.len(), 4);
        assert!(bands[0].enabled && bands[0].filter_type == "LSC");
        assert!(!bands[2].enabled);

        // Out-of-bounds frequency or Q must be rejected with InvalidInput
        let bad_freq = "Filter 1: ON PK Fc 45000 Hz Gain 2.0 dB Q 1.0";
        assert_eq!(
            parse_autoeq_text(bad_freq).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let bad_q = "Filter 1: ON PK Fc 1000 Hz Gain 2.0 dB Q 0.01";
        assert_eq!(
            parse_autoeq_text(bad_q).unwrap_err().code,
            ErrorCode::InvalidInput
        );

        // 3. Full 8-stage ASMR DSP chain construction (R18 + R19 + T1102 Swap L/R & BS2B)
        let mut full_cfg = DspConfigDto {
            master_bypass: false,
            user_preamp_db: 0.0,
            autoeq_preamp_db: preamp,
            hpf_enabled: true,
            hpf_cutoff_hz: 50.0,
            autoeq_enabled: true,
            autoeq_name: Some("HD650".into()),
            autoeq_bands: bands,
            de_esser_enabled: true,
            de_esser_freq_hz: 6500.0,
            de_esser_q: 4.0,
            de_esser_gain_db: -3.5,
            air_enabled: true,
            air_freq_hz: 12000.0,
            air_gain_db: 1.5,
            balance_db: 0.0,
            swap_channels: false,
            bs2b_enabled: false,
            limiter_enabled: true,
            limiter_threshold_db: -1.0,
            limiter_lookahead_ms: 4.0,
            limiter_release_ms: 80.0,
        };

        // Effective preamp = -6.4 - 1.5 (Air boost headroom compensation) = -7.90 dB
        let graph = build_lavfi_graph(&full_cfg);
        assert!(graph.starts_with("lavfi=[volume=-7.90dB,highpass=f=50.0:poles=2:width_type=q:width=0.7071,"));
        assert!(graph.contains("lowshelf=f=105.0:width_type=q:width=0.70:g=6.60"));
        assert!(graph.contains("equalizer=f=1450.0:width_type=q:width=1.80:g=-2.40"));
        assert!(!graph.contains("f=3000.0"), "OFF filter must not be emitted");
        assert!(graph.contains("equalizer=f=6500:width_type=q:width=4.00:g=-3.50"));
        assert!(graph.contains("highshelf=f=12000:width_type=q:width=0.7071:g=1.50"));
        // -1.0 dBFS -> 10^(-1/20) = 0.891251, level=0 (zero upward pumping!)
        assert!(graph.ends_with("alimiter=level_in=1.0:level_out=1.0:limit=0.891251:attack=4.0:release=80.0:asc=1:level=0]"));

        // Verify T1102 Swap L/R and BS2B filter injection
        full_cfg.swap_channels = true;
        full_cfg.bs2b_enabled = true;
        let swapped_graph = build_lavfi_graph(&full_cfg);
        assert!(swapped_graph.contains("pan=stereo|c0=c1|c1=c0"));
        assert!(swapped_graph.contains("bs2b=fcut=700:feed=45"));
    }

    #[test]
    fn test_encrypted_sync_bundle_roundtrip_and_tamper_rejection_t1203() {
        let bundle = UserBackupBundleDto {
            schema_version: 1,
            exported_at: "2026-10-12T12:00:00Z".into(),
            favorites: vec![BackupFavoriteRecordDto {
                work_id: "w_sync".into(),
                rj_code: Some("RJ01999999".into()),
                work_title: "加密同步测试作品".into(),
                created_at: "2026-10-12T12:00:00Z".into(),
            }],
            playback_positions: vec![],
            bookmarks: vec![],
            ab_loops: vec![],
            work_overrides: vec![],
            custom_dsp_presets: vec![],
            work_dsp_overrides: vec![],
            dsp_global_config: DspConfigDto::default(),
            dsp_global_preset_id: Some("preset_sleep_whisper".into()),
        };

        let env = encrypt_backup_bundle(&bundle, "my-secret-passphrase").unwrap();
        assert!(!env.ciphertext_hex.contains("加密同步测试作品"));

        // Wrong passphrase must fail MAC verification
        let wrong_res = decrypt_backup_bundle(&env, "wrong-passphrase");
        assert!(wrong_res.is_err());

        // Tampered ciphertext must fail MAC verification
        let mut tampered = env.clone();
        tampered.ciphertext_hex.replace_range(0..2, "ff");
        if tampered.ciphertext_hex != env.ciphertext_hex {
            assert!(decrypt_backup_bundle(&tampered, "my-secret-passphrase").is_err());
        }

        // Correct passphrase restores exact bundle
        let restored = decrypt_backup_bundle(&env, "my-secret-passphrase").unwrap();
        assert_eq!(restored.favorites.len(), 1);
        assert_eq!(restored.favorites[0].work_title, "加密同步测试作品");
        assert_eq!(
            restored.dsp_global_preset_id.as_deref(),
            Some("preset_sleep_whisper")
        );
    }

    #[test]
    fn test_multi_monitor_window_placement_restore_r39_t1119() {
        let primary = MonitorDescriptorDto {
            name: Some("\\\\.\\DISPLAY1".into()),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        };
        let secondary_right = MonitorDescriptorDto {
            name: Some("\\\\.\\DISPLAY2".into()),
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            scale_factor: 1.25,
        };
        let secondary_left = MonitorDescriptorDto {
            name: Some("\\\\.\\DISPLAY3".into()),
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        };
        let monitors = vec![
            primary.clone(),
            secondary_right.clone(),
            secondary_left.clone(),
        ];

        // 1. Closed on right secondary monitor (副屏) -> restores to exact position on DISPLAY2
        let saved_on_right = WindowPlacementDto {
            x: 2150,
            y: 140,
            width: 1280,
            height: 820,
            maximized: false,
            monitor_name: Some("\\\\.\\DISPLAY2".into()),
            monitor_x: Some(1920),
            monitor_y: Some(0),
            monitor_width: Some(2560),
            monitor_height: Some(1440),
        };
        let restored_right = resolve_window_restore_placement(&saved_on_right, &monitors);
        assert_eq!(restored_right.monitor_name.as_deref(), Some("\\\\.\\DISPLAY2"));
        assert_eq!(restored_right.x, 2150);
        assert_eq!(restored_right.y, 140);
        assert_eq!(restored_right.width, 1280);
        assert_eq!(restored_right.height, 820);
        assert!(!restored_right.maximized);

        // 2. Closed on left secondary monitor with negative coordinates (x = -1600) -> restores on DISPLAY3
        let saved_on_left = WindowPlacementDto {
            x: -1600,
            y: 100,
            width: 1280,
            height: 820,
            maximized: false,
            monitor_name: Some("\\\\.\\DISPLAY3".into()),
            monitor_x: Some(-1920),
            monitor_y: Some(0),
            monitor_width: Some(1920),
            monitor_height: Some(1080),
        };
        let restored_left = resolve_window_restore_placement(&saved_on_left, &monitors);
        assert_eq!(restored_left.monitor_name.as_deref(), Some("\\\\.\\DISPLAY3"));
        assert_eq!(restored_left.x, -1600);
        assert_eq!(restored_left.y, 100);

        // 3. Closed while MAXIMIZED on secondary monitor (where Windows outer_position had -8px border at x=1912, y=-8)
        // Must clamp anchor strictly inside DISPLAY2 (>= 1920, >= 0) so `maximize()` targets DISPLAY2!
        let saved_max_secondary = WindowPlacementDto {
            x: 1912,
            y: -8,
            width: 1280,
            height: 820,
            maximized: true,
            monitor_name: Some("\\\\.\\DISPLAY2".into()),
            monitor_x: Some(1920),
            monitor_y: Some(0),
            monitor_width: Some(2560),
            monitor_height: Some(1440),
        };
        let restored_max = resolve_window_restore_placement(&saved_max_secondary, &monitors);
        assert_eq!(restored_max.monitor_name.as_deref(), Some("\\\\.\\DISPLAY2"));
        assert!(restored_max.x >= 1920 && restored_max.x < 1920 + 2560);
        assert!(restored_max.y >= 0 && restored_max.y < 1440);
        assert!(restored_max.maximized);

        // 4. Secondary monitor unplugged (only primary connected) -> safely centers on primary monitor
        let single_monitor = vec![primary];
        let fallback = resolve_window_restore_placement(&saved_on_right, &single_monitor);
        assert_eq!(fallback.monitor_name.as_deref(), Some("\\\\.\\DISPLAY1"));
        assert_eq!(fallback.x, (1920 - 1280) / 2);
        assert_eq!(fallback.y, (1080 - 820) / 2);

        // 5. Minimized (-32000, -32000) or mini-mode bounds must be rejected by capture guard
        assert!(!is_valid_normal_window_bounds(-32000, -32000, 1280, 820, false));
        assert!(!is_valid_normal_window_bounds(100, 100, 1280, 820, true));
        assert!(!is_valid_normal_window_bounds(100, 100, 340, 88, false));
        assert!(is_valid_normal_window_bounds(2150, 140, 1280, 820, false));
    }

    #[test]
    fn test_desktop_island_settings_defaults_and_placement_r46_r47() {
        // T1308 / R47: Verify JSON deserialization of AppSettingsDto with default island preferences
        let minimal_json = r#"{
            "downloadDir": "C:\\Downloads",
            "apiBaseUrl": "https://api.asmr.one",
            "proxyUrl": null,
            "closeBehavior": "minimize_to_tray",
            "hardwareAcceleration": true,
            "hasAuthToken": false,
            "mpvAvailable": true,
            "mpvPath": "mpv.exe"
        }"#;
        let parsed: AppSettingsDto = serde_json::from_str(minimal_json).expect("should deserialize");
        assert!(parsed.island_fullscreen_avoidance);
        assert!(parsed.island_edge_snap);
        assert!(parsed.island_always_on_top);
        assert!(parsed.island_scroll_volume);
        assert!(parsed.island_toast_enabled);
        assert!((parsed.island_toast_duration_sec - 3.0).abs() < f64::EPSILON);
        assert!(!parsed.island_privacy_mode);

        // T1306 / R46: Verify IslandPlacementDto serialization & deserialization
        let placement = IslandPlacementDto { x: 830, y: 8 };
        let encoded = serde_json::to_string(&placement).expect("should serialize placement");
        let decoded: IslandPlacementDto = serde_json::from_str(&encoded).expect("should deserialize placement");
        assert_eq!(decoded.x, 830);
        assert_eq!(decoded.y, 8);
    }
}

