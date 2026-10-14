//! T1509: export an allowlisted, frozen diagnostic snapshot, never raw logs or DB rows.
use crate::{db::Database, domain::*};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticPreview {
    pub token: String,
    pub generated_at: String,
    pub bytes: usize,
    pub sha256: String,
    pub content: String,
}
struct Frozen {
    preview: DiagnosticPreview,
    created: Instant,
}
#[derive(Clone)]
pub struct DiagnosticService {
    db: Database,
    previews: Arc<Mutex<HashMap<String, Frozen>>>,
}
fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

// No replace flag: publish a complete file, including on FAT/exFAT volumes.
#[cfg(windows)]
fn publish_new(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
    }
    let source: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn publish_new(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::hard_link(source, target)
}
fn allowed<'a>(value: &'a str, choices: &[&str]) -> &'a str {
    if choices.contains(&value) {
        value
    } else {
        "unknown"
    }
}
const ERROR_CODES: &[ErrorCode] = &[
    ErrorCode::NetworkUnavailable,
    ErrorCode::AuthRequired,
    ErrorCode::RateLimited,
    ErrorCode::SourceNotFound,
    ErrorCode::SourceChanged,
    ErrorCode::ProviderSchemaChanged,
    ErrorCode::DatabaseError,
    ErrorCode::MigrationError,
    ErrorCode::InvalidInput,
    ErrorCode::PathSecurityViolation,
    ErrorCode::DiskOffline,
    ErrorCode::DiskFull,
    ErrorCode::FileLocked,
    ErrorCode::MediaDecodeFailed,
    ErrorCode::MpvMissing,
    ErrorCode::MpvIpcError,
    ErrorCode::EncodingUnsupported,
    ErrorCode::InternalError,
];
fn error_category(text: &str) -> Value {
    for code in ERROR_CODES {
        let serialized = serde_json::to_value(code).unwrap();
        if text
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|word| word == format!("{code:?}") || Some(word) == serialized.as_str())
        {
            return serialized;
        }
    }
    json!("UNCLASSIFIED")
}
fn log_event(line: &str) -> Option<Value> {
    let (timestamp, rest) = line.strip_prefix('[')?.split_once("] ")?;
    let timestamp = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Utc)
        .to_rfc3339();
    let (level, message) = rest.strip_prefix('[')?.split_once("] ")?;
    if !["WARN", "ERROR"].contains(&level) {
        return None;
    }
    Some(json!({"time":timestamp,"level":level,"category":error_category(message)}))
}
fn recent_events(db: &Database) -> Value {
    let mut events = Vec::new();
    let mut unreadable = 0;
    let mut truncated = false;
    for name in ["app.log.1", "app.log"] {
        let result = (|| -> std::io::Result<()> {
            let mut file = File::open(db.log_dir.join(name))?;
            let len = file.metadata()?.len();
            let start = len.saturating_sub(128 * 1024);
            file.seek(SeekFrom::Start(start))?;
            let mut bytes = Vec::new();
            file.take(128 * 1024).read_to_end(&mut bytes)?;
            let text = String::from_utf8_lossy(&bytes);
            let text = if start > 0 {
                truncated = true;
                text.split_once('\n').map(|(_, tail)| tail).unwrap_or("")
            } else {
                &text
            };
            events.extend(text.lines().filter_map(log_event));
            Ok(())
        })();
        if let Err(e) = result {
            if e.kind() != std::io::ErrorKind::NotFound {
                unreadable += 1;
            }
        }
    }
    events.sort_by(|a, b| a["time"].as_str().cmp(&b["time"].as_str()));
    if events.len() > 100 {
        truncated = true;
        events.drain(..events.len() - 100);
    }
    json!({"entries":events,"truncated":truncated,"unreadableFiles":unreadable,"rawMessagesIncluded":false})
}
fn audio_summary(output: AppResult<AudioOutputStateDto>) -> Value {
    match output {
        Ok(output) => json!({
            "status":"available", "deviceCount":output.devices.len(),
            "devices":output.devices.iter().take(64).enumerate().map(|(i,d)|json!({
                "label":format!("设备 {}",i+1),
                "backend":if d.id=="auto" {"auto"} else {allowed(d.id.split('/').next().unwrap_or(""), &["wasapi","alsa","pulse","coreaudio","jack"])},
                "selected":d.id==output.selected_device,
            })).collect::<Vec<_>>(),
            "selectedDevicePresent":output.devices.iter().any(|d|d.id==output.selected_device),
            "namesAndIdentifiersIncluded":false,
        }),
        Err(e) => json!({"status":"unavailable","errorCode":e.code}),
    }
}
#[cfg(windows)]
fn os_version() -> Option<String> {
    #[repr(C)]
    struct Version {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut Version) -> i32;
    }
    let mut v = Version {
        size: std::mem::size_of::<Version>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    (unsafe { RtlGetVersion(&mut v) } == 0).then(|| format!("{}.{}.{}", v.major, v.minor, v.build))
}
#[cfg(not(windows))]
fn os_version() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{providers::AsmrOneProvider, services::playback::PlaybackService};
    use tempfile::tempdir;
    fn snapshot(db: &Database) -> AppResult<PlaybackSnapshot> {
        PlaybackService::new(db.clone(), AsmrOneProvider::new(db.clone())).get_snapshot()
    }
    fn audio() -> AppResult<AudioOutputStateDto> {
        Ok(AudioOutputStateDto {
            selected_device: "wasapi/private-guid".into(),
            devices: vec![AudioOutputDeviceDto {
                id: "wasapi/private-guid".into(),
                name: "Alice's private headphones".into(),
            }],
        })
    }
    #[test]
    fn allowlist_removes_secrets_paths_titles_and_free_text_t1509() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        db.append_log("ERROR", r#"NetworkUnavailable https://user:pw@host/secret?sig=private-token C:\Users\Alice\Private-title.mp3 token=a token=b Cookie: hidden"#);
        db.append_log("WARN", "私密作品名称及播放列表内容");
        db.with_conn(|c| {
            c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('private-work','私密作品名称','now','now');
            INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','private-work','private-track','1','now');
            INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');
            INSERT INTO media_sources(id,version_id,source_kind,format,created_at,updated_at) VALUES('s','v','online_provider','mp3','now','now');
            INSERT INTO downloads(id,work_id,version_id,source_id,status,temp_relative_path,target_relative_path,error_code,error_message,created_at,updated_at) VALUES('private-task','private-work','v','s','failed','secret.part','secret.mp3','NetworkUnavailable','secret-token','now','now');")?;Ok(())
        }).unwrap();
        let service = DiagnosticService::new(db.clone());
        let mut playback = snapshot(&db).unwrap();
        playback.current_work_title = Some("私密作品名称".into());
        playback.last_error = Some(AppError::new(ErrorCode::DiskFull, "Private-title.mp3"));
        let p = service.preview(audio(), Ok(playback)).unwrap();
        for secret in [
            "Alice",
            "private-guid",
            "Private-title",
            "私密作品",
            "private-work",
            "private-task",
            "private-track",
            "secret-token",
            "token=a",
            "token=b",
            "https://",
            "C:\\Users",
            "hidden",
        ] {
            assert!(!p.content.contains(secret), "leaked {secret}");
        }
        let v: Value = serde_json::from_str(&p.content).unwrap();
        assert_eq!(v["audio"]["devices"][0]["backend"], "wasapi");
        assert_eq!(v["audio"]["devices"][0]["selected"], true);
        assert_eq!(v["downloads"]["statusCounts"]["failed"], 1);
        assert_eq!(v["downloads"]["errorCounts"]["NETWORK_UNAVAILABLE"], 1);
        assert_eq!(
            v["recentErrors"]["entries"][0]["category"],
            "NETWORK_UNAVAILABLE"
        );
        assert_eq!(v["playback"]["lastErrorCode"], "DISK_FULL");
    }
    #[test]
    fn export_uses_frozen_bytes_and_never_replaces_existing_files_t1509() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let service = DiagnosticService::new(db.clone());
        let p = service.preview(audio(), snapshot(&db)).unwrap();
        db.append_log("ERROR", "DiskFull new error after preview");
        let path = dir.path().join("report.json");
        service.export(&p.token, &path).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes, p.content.as_bytes());
        assert_eq!(p.bytes, bytes.len());
        assert_eq!(p.sha256, hex::encode(Sha256::digest(&bytes)));
        assert!(service.export(&p.token, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(service
            .export("unknown", &dir.path().join("unknown.json"))
            .is_err());
        assert!(service
            .export(&p.token, &dir.path().join("missing").join("report.json"))
            .is_err());
        assert!(service
            .export(&p.token, Path::new("relative.json"))
            .is_err());
        assert!(!fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .path()
            .extension()
            .is_some_and(|x| x == "part")));
        service.previews.lock().get_mut(&p.token).unwrap().created =
            Instant::now() - Duration::from_secs(901);
        assert!(service
            .export(&p.token, &dir.path().join("expired.json"))
            .is_err());
        assert!(!dir.path().join("expired.json").exists());
    }
    #[test]
    fn missing_audio_and_bounded_recent_events_are_explicit_t1509() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let mut log = "X".repeat(160 * 1024);
        log.push('\n');
        for _ in 0..120 {
            log.push_str("[2026-10-14T01:00:00Z] [WARN] RateLimited secret\n");
        }
        fs::write(db.log_dir.join("app.log"), log).unwrap();
        let service = DiagnosticService::new(db.clone());
        let p = service
            .preview(
                Err(AppError::new(ErrorCode::MpvMissing, "C:/secret/path")),
                snapshot(&db),
            )
            .unwrap();
        let v: Value = serde_json::from_str(&p.content).unwrap();
        assert_eq!(v["audio"]["status"], "unavailable");
        assert_eq!(v["audio"]["errorCode"], "MPV_MISSING");
        assert_eq!(v["recentErrors"]["entries"].as_array().unwrap().len(), 100);
        assert_eq!(v["recentErrors"]["truncated"], true);
        assert!(!p.content.contains("secret"));
        assert!(p.bytes < 64 * 1024);
        assert!(log_event("[secret] [ERROR] DiskFull").is_none());
        assert!(log_event("[2026-10-14T01:00:00Z] [INFO] secret").is_none());
        assert_eq!(
            error_category("UserControlledErrorSecret"),
            json!("UNCLASSIFIED")
        );
    }
    #[test]
    fn previews_have_bounded_capacity_and_independent_tokens_t1509() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let service = DiagnosticService::new(db.clone());
        let first = service.preview(audio(), snapshot(&db)).unwrap();
        let mut last = first.clone();
        for _ in 0..4 {
            last = service.preview(audio(), snapshot(&db)).unwrap();
        }
        assert_eq!(service.previews.lock().len(), 4);
        assert!(service
            .export(&first.token, &dir.path().join("old.json"))
            .is_err());
        service
            .export(&last.token, &dir.path().join("new.json"))
            .unwrap();
    }
}

impl DiagnosticService {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            previews: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn preview(
        &self,
        audio: AppResult<AudioOutputStateDto>,
        playback: AppResult<PlaybackSnapshot>,
    ) -> AppResult<DiagnosticPreview> {
        let (tasks, schema) = self.db.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT status,error_code,COUNT(*) FROM downloads GROUP BY status,error_code",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, u64>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut states = BTreeMap::<String, u64>::new();
            let mut errors = BTreeMap::<String, u64>::new();
            for (state, code, count) in rows {
                *states
                    .entry(
                        allowed(
                            &state,
                            &[
                                "queued",
                                "resolving",
                                "downloading",
                                "verifying",
                                "paused",
                                "failed",
                                "canceled",
                                "completed",
                            ],
                        )
                        .into(),
                    )
                    .or_default() += count;
                if let Some(code) = code {
                    *errors
                        .entry(error_category(&code).as_str().unwrap().into())
                        .or_default() += count;
                }
            }
            let schema: i64 = c.query_row(
                "SELECT COALESCE(MAX(version),0) FROM _schema_migrations",
                [],
                |r| r.get(0),
            )?;
            Ok((json!({"statusCounts":states,"errorCounts":errors}), schema))
        })?;
        let playback = match playback {
            Ok(p) => {
                json!({"status":allowed(&p.state,&["idle","resolving","loading","playing","paused","buffering","ended","error"]),
                "queueItems":p.queue.len(),"audioExclusive":p.audio_exclusive,
                "sampleRateHz":p.audio_telemetry.sample_rate_hz,"bitrateKbps":p.audio_telemetry.bitrate_kbps,
                "lastErrorCode":p.last_error.map(|e|e.code)})
            }
            Err(e) => json!({"status":"unavailable","errorCode":e.code}),
        };
        let generated_at = Utc::now().to_rfc3339();
        let content=serde_json::to_string_pretty(&json!({
            "schemaVersion":1,"generatedAt":generated_at,
            "application":{"version":env!("CARGO_PKG_VERSION"),"build":if cfg!(debug_assertions){"debug"}else{"release"},"databaseSchema":schema,"engine":"libmpv"},
            "system":{"os":std::env::consts::OS,"version":os_version(),"architecture":std::env::consts::ARCH,"logicalProcessors":std::thread::available_parallelism().ok().map(|n|n.get())},
            "audio":audio_summary(audio),"playback":playback,"downloads":tasks,"recentErrors":recent_events(&self.db),
            "privacy":{"mode":"allowlist","excluded":["credentials","urls","paths","titles","deviceNamesAndIds","rawLogMessages","media","database","environmentVariables"],"uploaded":false},
        })).map_err(|_|invalid("诊断内容生成失败"))?;
        let preview = DiagnosticPreview {
            token: uuid::Uuid::new_v4().to_string(),
            generated_at,
            bytes: content.len(),
            sha256: hex::encode(Sha256::digest(content.as_bytes())),
            content,
        };
        let mut previews = self.previews.lock();
        previews.retain(|_, v| v.created.elapsed() < Duration::from_secs(900));
        if previews.len() >= 4 {
            if let Some(key) = previews
                .iter()
                .min_by_key(|(_, v)| v.created)
                .map(|(k, _)| k.clone())
            {
                previews.remove(&key);
            }
        }
        previews.insert(
            preview.token.clone(),
            Frozen {
                preview: preview.clone(),
                created: Instant::now(),
            },
        );
        Ok(preview)
    }
    pub fn export(&self, token: &str, target: &Path) -> AppResult<()> {
        let preview = {
            let guard = self.previews.lock();
            let frozen = guard
                .get(token)
                .ok_or_else(|| invalid("诊断预览已失效，请重新生成"))?;
            if frozen.created.elapsed() >= Duration::from_secs(900) {
                return Err(invalid("诊断预览已过期，请重新生成"));
            }
            frozen.preview.clone()
        };
        if !target.is_absolute()
            || !target
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            || target.components().any(|c| match c {
                std::path::Component::Normal(n) => n.to_string_lossy().contains([':', '\0']),
                std::path::Component::ParentDir => true,
                _ => false,
            })
        {
            return Err(invalid("请选择完整的 JSON 文件保存路径"));
        }
        // Never overwrite existing files, including media, databases or an earlier report.
        let parent = target.parent().ok_or_else(|| invalid("导出目录无效"))?;
        let pending = parent.join(format!(".asmr-diagnostic-{}.part", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&pending)?;
            f.write_all(preview.content.as_bytes())?;
            f.sync_all()?;
            drop(f);
            publish_new(&pending, target)?;
            Ok(())
        })();
        let _ = fs::remove_file(&pending);
        result.map_err(|e| {
            AppError::new(
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    ErrorCode::InvalidInput
                } else {
                    ErrorCode::FileLocked
                },
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "文件已存在，请选择新的文件名"
                } else {
                    "诊断包保存失败，请检查目录权限和磁盘空间后重试"
                },
            )
        })
    }
}
