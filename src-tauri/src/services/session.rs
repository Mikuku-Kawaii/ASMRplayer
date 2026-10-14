use crate::db::Database;
use crate::domain::*;
use crate::player::resolve_mpv_binary;
use crate::providers::AsmrOneProvider;
use crate::services::download::DownloadService;
use crate::services::playback::PlaybackService;

#[derive(Clone)]
pub struct SessionService {
    pub diagnostics: super::diagnostics::DiagnosticService,
    pub backups: super::backup::BackupService,
    pub storage: super::storage::StorageService,
    db: Database,
    provider: AsmrOneProvider,
    playback: PlaybackService,
    downloads: DownloadService,
}

impl SessionService {
    pub fn new(
        db: Database,
        provider: AsmrOneProvider,
        playback: PlaybackService,
        downloads: DownloadService,
    ) -> Self {
        Self {
            diagnostics: super::diagnostics::DiagnosticService::new(db.clone()),
            backups: super::backup::BackupService::new(db.clone()),
            storage: super::storage::StorageService::new(db.clone(), downloads.clone(), playback.clone()),
            db,
            provider,
            playback,
            downloads,
        }
    }

    pub fn get_settings(&self) -> AppResult<AppSettingsDto> {
        let download_dir = self
            .downloads
            .get_download_root()
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        let api_base_url = self.provider.api_base();
        let proxy_url = self.provider.proxy_url();
        let close_behavior = self
            .db
            .get_setting("close_behavior")?
            .unwrap_or_else(|| "minimize_to_tray".to_string());
        let hardware_acceleration = self
            .db
            .get_setting("hardware_acceleration")?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let audio_exclusive = self
            .db
            .get_setting("audio_exclusive")?
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let smtc_mode = self
            .db
            .get_setting("smtc_mode")?
            .filter(|m| matches!(m.as_str(), "full" | "privacy" | "disabled"))
            .unwrap_or_else(|| "full".to_string());
        let realtime_watch_enabled = self
            .db
            .get_setting("realtime_watch_enabled")?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let webdav_url = self
            .db
            .get_setting("webdav_url")?
            .filter(|s| !s.trim().is_empty());
        let nsfw_enabled = self.provider.nsfw_enabled();
        let island_fullscreen_avoidance = self
            .db
            .get_setting("island_fullscreen_avoidance")?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let island_edge_snap = self
            .db
            .get_setting("island_edge_snap")?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let island_always_on_top = self
            .db
            .get_setting("island_always_on_top")?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let island_scroll_volume = self
            .db
            .get_setting("island_scroll_volume")?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let island_toast_enabled = self
            .db
            .get_setting("island_toast_enabled")?
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        let island_toast_duration_sec = self
            .db
            .get_setting("island_toast_duration_sec")?
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(3.0);
        let island_privacy_mode = self
            .db
            .get_setting("island_privacy_mode")?
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let has_auth_token = self.provider.has_auth_token();
        let (mpv_available, mpv_path) = match resolve_mpv_binary() {
            Ok(p) => (
                true,
                p.to_string_lossy().trim_start_matches(r"\\?\").to_string(),
            ),
            Err(_) => (false, String::new()),
        };

        Ok(AppSettingsDto {
            download_dir,
            api_base_url,
            proxy_url,
            close_behavior,
            hardware_acceleration,
            audio_exclusive,
            smtc_mode,
            realtime_watch_enabled,
            webdav_url,
            nsfw_enabled,
            island_fullscreen_avoidance,
            island_edge_snap,
            island_always_on_top,
            island_scroll_volume,
            island_toast_enabled,
            island_toast_duration_sec,
            island_privacy_mode,
            has_auth_token,
            mpv_available,
            mpv_path,
        })
    }

    pub fn update_settings(
        &self,
        download_dir: &str,
        api_base_url: &str,
        proxy_url: Option<String>,
        close_behavior: &str,
        hardware_acceleration: Option<bool>,
        auth_token: Option<String>,
        audio_exclusive: Option<bool>,
        smtc_mode: Option<String>,
        realtime_watch_enabled: Option<bool>,
        webdav_url: Option<String>,
        nsfw_enabled: Option<bool>,
        island_fullscreen_avoidance: Option<bool>,
        island_edge_snap: Option<bool>,
        island_always_on_top: Option<bool>,
        island_scroll_volume: Option<bool>,
        island_toast_enabled: Option<bool>,
        island_toast_duration_sec: Option<f64>,
        island_privacy_mode: Option<bool>,
    ) -> AppResult<AppSettingsDto> {
        if close_behavior != "minimize_to_tray" && close_behavior != "quit" {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "close_behavior 必须为 minimize_to_tray 或 quit",
            ));
        }
        self.downloads.set_download_root(download_dir)?;
        self.provider.set_api_base(api_base_url)?;
        self.provider.set_proxy_url(proxy_url.clone())?;
        self.playback.set_proxy(proxy_url);
        self.db.set_setting("close_behavior", close_behavior)?;

        if let Some(hw_accel) = hardware_acceleration {
            self.db.set_setting(
                "hardware_acceleration",
                if hw_accel { "true" } else { "false" },
            )?;
            self.playback.set_hardware_acceleration(hw_accel);
        }

        if let Some(exclusive) = audio_exclusive {
            self.db.set_setting(
                "audio_exclusive",
                if exclusive { "true" } else { "false" },
            )?;
        }

        if let Some(ref mode) = smtc_mode {
            let _ = self.playback.set_smtc_mode(mode)?;
        }

        if let Some(watch_en) = realtime_watch_enabled {
            self.db.set_setting(
                "realtime_watch_enabled",
                if watch_en { "true" } else { "false" },
            )?;
        }

        if let Some(wd) = webdav_url {
            let trimmed = wd.trim();
            if trimmed.is_empty() {
                self.db.set_setting("webdav_url", "")?;
            } else {
                self.db.set_setting("webdav_url", trimmed)?;
            }
        }

        if let Some(nsfw) = nsfw_enabled {
            self.provider.set_nsfw_enabled(nsfw)?;
        }

        if let Some(fa) = island_fullscreen_avoidance {
            self.db.set_setting(
                "island_fullscreen_avoidance",
                if fa { "true" } else { "false" },
            )?;
        }

        if let Some(es) = island_edge_snap {
            self.db
                .set_setting("island_edge_snap", if es { "true" } else { "false" })?;
        }

        if let Some(aot) = island_always_on_top {
            self.db
                .set_setting("island_always_on_top", if aot { "true" } else { "false" })?;
        }

        if let Some(sv) = island_scroll_volume {
            self.db
                .set_setting("island_scroll_volume", if sv { "true" } else { "false" })?;
        }

        if let Some(te) = island_toast_enabled {
            self.db
                .set_setting("island_toast_enabled", if te { "true" } else { "false" })?;
        }

        if let Some(td) = island_toast_duration_sec {
            self.db
                .set_setting("island_toast_duration_sec", &td.clamp(2.0, 5.0).to_string())?;
        }

        if let Some(pm) = island_privacy_mode {
            self.db
                .set_setting("island_privacy_mode", if pm { "true" } else { "false" })?;
        }

        if let Some(tok) = auth_token {
            if tok == "__CLEAR__" {
                self.provider.set_auth_token(None)?;
            } else if !tok.trim().is_empty() {
                self.provider.set_auth_token(Some(tok))?;
            }
        }

        self.get_settings()
    }

    /// T509: Clears only playback/application cache directory (`cache_dir`),
    /// NEVER deleting offline downloaded files or user imported media!
    pub fn clear_playback_cache(&self) -> AppResult<u64> {
        let preview = self.storage.preview(super::storage::Category::Playback, vec![])?;
        Ok(self.storage.execute(&preview.token)?.freed_bytes)
    }

    pub fn get_diagnostics(&self) -> AppResult<Vec<String>> {
        Ok(self.db.read_recent_logs(120))
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::services::library::LibraryService;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_database_upgrade_creates_backup_and_preserves_records_t704() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let cache_dir = dir.path().join("cache");
        fs::create_dir_all(&data_dir).unwrap();
        let db_path = data_dir.join("asmr_player.db");

        // Simulate a v1 database created by an older version
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(include_str!("../../migrations/001_initial_core.sql"))
                .unwrap();
            conn.execute(
                "CREATE TABLE _schema_migrations (version INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL, applied_at TEXT NOT NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO _schema_migrations (version, name, applied_at) VALUES (1, '001_initial_core', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO works (id, title, rj_code, created_at, updated_at) VALUES ('w_legacy', 'Legacy Work', 'RJ01000001', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        }

        // Open and migrate to v4
        let db = Database::open_and_migrate(&data_dir, &cache_dir).unwrap();
        let detail = db.get_work_detail("w_legacy").unwrap();
        assert_eq!(detail.title, "Legacy Work");

        // Verify a backup file asmr_player.backup-v1-*.db was created
        let has_backup = fs::read_dir(&data_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("asmr_player.backup-v1-")
            });
        assert!(has_backup, "Upgrade from v1 must create a backup database file");
    }

    #[test]
    fn test_offline_disk_and_library_root_relocation_r07() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let lib = LibraryService::new(db.clone());

        let disk_a = dir.path().join("disk_a").join("RJ01000088 测试搬迁作品");
        fs::create_dir_all(&disk_a).unwrap();
        let fixture_mp3 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("stereo_lr_check.mp3");
        fs::copy(&fixture_mp3, disk_a.join("01_track.mp3")).unwrap();

        let report = lib
            .scan_and_import(&disk_a.to_string_lossy(), "single_work", None)
            .unwrap();
        assert_eq!(report.works_added, 1);

        let works = db.list_works("all", None).unwrap();
        let work_id = works[0].id.clone();
        db.toggle_favorite(&work_id).unwrap();

        let detail = db.get_work_detail(&work_id).unwrap();
        let ver_id = detail.tracks[0].versions[0].id.clone();
        db.save_playback_position(&ver_id, 9_500, false, true)
            .unwrap();

        // Simulate drive disconnection / move from disk_a to disk_b
        let disk_b = dir.path().join("disk_b").join("RJ01000088 测试搬迁作品");
        fs::create_dir_all(disk_b.parent().unwrap()).unwrap();
        fs::rename(&disk_a, &disk_b).unwrap();

        // Rescan missing root -> must mark root missing without deleting work, favorite, or progress
        let res_missing = lib.scan_and_import(&disk_a.to_string_lossy(), "single_work", None);
        assert!(res_missing.is_err());

        let detail_offline = db.get_work_detail(&work_id).unwrap();
        assert!(detail_offline.is_favorite, "Favorite must survive offline disk");
        assert_eq!(
            detail_offline.tracks[0].versions[0].position_ms, 9_500,
            "Progress must survive offline disk"
        );

        // Relocate root to disk_b -> source becomes available again with all records preserved
        let relocated = lib
            .relocate_root(&report.root_id, &disk_b.to_string_lossy())
            .unwrap();
        assert_eq!(relocated.availability, "available");

        let detail_restored = db.get_work_detail(&work_id).unwrap();
        assert!(detail_restored.is_favorite);
        assert_eq!(detail_restored.tracks[0].versions[0].position_ms, 9_500);
        assert_eq!(
            detail_restored.tracks[0].versions[0].sources[0].availability,
            "available"
        );
    }

    #[tokio::test]
    async fn test_online_to_download_offline_resume_and_cache_clean_r10_t509() {
        let server = MockServer::start().await;
        let fixture_mp3 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures")
            .join("audio")
            .join("stereo_lr_check.mp3");
        let audio_bytes = fs::read(&fixture_mp3).unwrap();

        Mock::given(method("GET"))
            .and(path("/api/workInfo/1705068"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 1705068,
                "title": "R10 在线转离线测试作品",
                "name": "测试社团",
                "vas": [{"name": "测试声优"}],
                "tags": [{"name": "耳かき"}]
            })))
            .mount(&server)
            .await;

        let stream_url = format!("{}/media/track01.mp3", server.uri());
        Mock::given(method("GET"))
            .and(path("/api/tracks/1705068"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "type": "audio",
                    "hash": "1705068/0",
                    "title": "01_r10_track.mp3",
                    "mediaStreamUrl": stream_url,
                    "mediaDownloadUrl": stream_url,
                    "size": audio_bytes.len(),
                    "duration": 30.0
                }
            ])))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/media/track01.mp3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Length", audio_bytes.len().to_string().as_str())
                    .set_body_bytes(audio_bytes.clone()),
            )
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let cache_dir = dir.path().join("cache");
        let dl_dir = dir.path().join("downloads");

        let db = Database::open_and_migrate(&data_dir, &cache_dir).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        provider.set_api_base(&server.uri()).unwrap();
        let playback = PlaybackService::new(db.clone(), provider.clone());
        let downloads = DownloadService::new(db.clone(), provider.clone());
        downloads
            .set_download_root(&dl_dir.to_string_lossy())
            .unwrap();
        let session = SessionService::new(
            db.clone(),
            provider.clone(),
            playback.clone(),
            downloads.clone(),
        );

        // 1. Open online work and record progress + bookmark on its version_id
        let detail = provider.open_online_work("RJ01705068").await.unwrap();
        let work_id = detail.id.clone();
        let version_id = detail.tracks[0].versions[0].id.clone();
        let online_source_id = detail.tracks[0].versions[0].sources[0].id.clone();

        db.save_playback_position(&version_id, 14_200, false, true)
            .unwrap();
        let bm = db
            .add_bookmark(&version_id, 14_200, "R10 书签位置")
            .unwrap();

        // 2. Download the online source to local disk
        downloads
            .enqueue_downloads(&work_id, Some(vec![online_source_id]))
            .unwrap();

        for _ in 0..50 {
            let tasks = downloads.list_tasks().unwrap();
            if tasks.iter().all(|t| t.status == "completed") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let tasks = downloads.list_tasks().unwrap();
        assert_eq!(tasks[0].status, "completed");

        // 3. Disconnect network by pointing provider to an unreachable port
        provider.set_api_base("http://127.0.0.1:9").unwrap();

        // 4. Verify the SAME version_id now has an available local_downloaded source,
        // preserves position_ms = 14_200, and preserves its bookmark!
        let after_detail = db.get_work_detail(&work_id).unwrap();
        let ver = &after_detail.tracks[0].versions[0];
        assert_eq!(ver.id, version_id, "version_id must remain identical after download");
        assert_eq!(ver.position_ms, 14_200, "progress must be preserved on version_id");
        assert!(
            ver.sources
                .iter()
                .any(|s| s.source_kind == "local_downloaded" && s.availability == "available"),
            "local_downloaded source must be registered and available"
        );

        let bookmarks = db.list_bookmarks(None, Some(&version_id)).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].id, bm.id);
        assert_eq!(bookmarks[0].position_ms, 14_200);

        // 5. Write a dummy cache file in cache_dir and run clear_playback_cache (T509):
        // cache file must be removed, while downloaded audio file must remain untouched!
        fs::write(cache_dir.join("temp_stream.cache"), b"cached_bytes").unwrap();
        let freed = session.clear_playback_cache().unwrap();
        assert!(freed >= 12);
        assert!(!cache_dir.join("temp_stream.cache").exists());

        let local_source = ver
            .sources
            .iter()
            .find(|s| s.source_kind == "local_downloaded")
            .unwrap();
        let local_abs = std::path::PathBuf::from(local_source.absolute_path.as_ref().unwrap());
        assert!(
            local_abs.exists(),
            "Clearing playback cache must NEVER delete downloaded media files"
        );
    }
}

