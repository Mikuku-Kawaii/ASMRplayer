//! T1401–T1406: host-owned, signed updates. Checking/downloading never stops audio.
use crate::db::Database;
use crate::domain::{AppError, AppResult, ErrorCode};
use crate::services::playback::PlaybackService;
use parking_lot::Mutex;
use semver::Version;
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{oneshot, Mutex as AsyncMutex};

pub const RELEASES_URL: &str = "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/latest";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Verifying,
    Ready,
    Installing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub event_seq: u64,
    pub phase: UpdatePhase,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub notes: Option<String>,
    pub checked_at: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub auto_check: bool,
    pub skipped_version: Option<String>,
    pub dismissed: bool,
    pub error: Option<AppError>,
}

struct UpdateState {
    snapshot: UpdateSnapshot,
    update: Option<Update>,
    verified_bytes: Option<Vec<u8>>,
    cancel: Option<oneshot::Sender<()>>,
}

pub struct UpdateService<R: tauri::Runtime = tauri::Wry> {
    db: Database,
    app: AppHandle<R>,
    inner: Arc<Mutex<UpdateState>>,
    operation: Arc<AsyncMutex<()>>,
}

impl<R: tauri::Runtime> Clone for UpdateService<R> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            app: self.app.clone(),
            inner: self.inner.clone(),
            operation: self.operation.clone(),
        }
    }
}

fn is_newer_stable(current: &Version, candidate: &Version) -> bool {
    candidate.pre.is_empty() && candidate.cmp_precedence(current).is_gt()
}

// Only installers belonging to this repository may be offered. Redirects from GitHub's
// release endpoint to its asset CDN are handled by the updater's HTTPS client.
fn validate_download_url(url: &url::Url) -> AppResult<()> {
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url
            .path()
            .starts_with("/Mikuku-Kawaii/ASMRplayer/releases/download/")
    {
        return Err(update_error("更新包地址不属于此项目的 GitHub Release"));
    }
    Ok(())
}

fn update_error(message: &str) -> AppError {
    AppError::new(ErrorCode::InternalError, message)
        .with_hint("请重试，或在 GitHub 发布页手动下载安装包。当前版本和用户记录会保留。")
}

impl<R: tauri::Runtime> UpdateService<R> {
    pub fn new(db: Database, app: AppHandle<R>) -> AppResult<Self> {
        let snapshot = UpdateSnapshot {
            event_seq: 0,
            phase: UpdatePhase::Idle,
            current_version: app.package_info().version.to_string(),
            latest_version: None,
            notes: None,
            checked_at: db.get_setting("update_checked_at")?,
            auto_check: db.get_setting("update_auto_check")?.as_deref() != Some("false"),
            skipped_version: db
                .get_setting("update_skipped_version")?
                .filter(|v| !v.is_empty()),
            downloaded_bytes: 0,
            total_bytes: None,
            dismissed: false,
            error: None,
        };
        Ok(Self {
            db,
            app,
            inner: Arc::new(Mutex::new(UpdateState {
                snapshot,
                update: None,
                verified_bytes: None,
                cancel: None,
            })),
            operation: Arc::new(AsyncMutex::new(())),
        })
    }

    pub fn snapshot(&self) -> UpdateSnapshot {
        self.inner.lock().snapshot.clone()
    }

    fn publish(&self, change: impl FnOnce(&mut UpdateState)) -> UpdateSnapshot {
        let mut state = self.inner.lock();
        change(&mut state);
        state.snapshot.event_seq += 1;
        let snapshot = state.snapshot.clone();
        let _ = self.app.emit("updater:snapshot", &snapshot);
        snapshot
    }

    fn fail(&self, message: &str, detail: impl std::fmt::Display) -> AppError {
        self.db.append_log("WARN", &format!("Updater: {detail}"));
        let error = update_error(message);
        self.publish(|s| {
            s.snapshot.phase = if s.verified_bytes.is_some() {
                UpdatePhase::Ready
            } else {
                UpdatePhase::Error
            };
            s.snapshot.error = Some(error.clone());
        });
        error
    }

    pub fn set_auto_check(&self, enabled: bool) -> AppResult<UpdateSnapshot> {
        self.db
            .set_setting("update_auto_check", if enabled { "true" } else { "false" })?;
        Ok(self.publish(|s| s.snapshot.auto_check = enabled))
    }

    pub async fn check(&self, manual: bool) -> AppResult<UpdateSnapshot> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| update_error("更新操作正在进行，请稍候"))?;
        if !manual && !self.snapshot().auto_check {
            return Ok(self.snapshot());
        }
        // A verified package remains available until the user installs or dismisses it.
        if self.inner.lock().verified_bytes.is_some() {
            return Ok(self.snapshot());
        }
        self.publish(|s| {
            s.update = None;
            s.snapshot.phase = UpdatePhase::Checking;
            s.snapshot.latest_version = None;
            s.snapshot.notes = None;
            s.snapshot.downloaded_bytes = 0;
            s.snapshot.total_bytes = None;
            s.snapshot.dismissed = false;
            s.snapshot.error = None;
        });
        let result = async {
            let mut builder = self
                .app
                .updater_builder()
                .timeout(Duration::from_secs(30))
                .version_comparator(|current, release| is_newer_stable(&current, &release.version));
            if let Some(proxy) = self
                .db
                .get_setting("proxy_url")?
                .filter(|v| !v.trim().is_empty())
            {
                builder = builder.proxy(
                    url::Url::parse(&proxy)
                        .map_err(|_| update_error("代理地址无效，请在设置中修正"))?,
                );
            }
            let update = builder
                .build()
                .map_err(|e| update_error(&e.to_string()))?
                .check()
                .await
                .map_err(|e| update_error(&e.to_string()))?;
            // GitHub's /releases/latest excludes drafts and prereleases; also reject
            // metadata explicitly marked as either, even if it uses a stable version.
            let update = update.filter(|u| {
                u.raw_json.get("draft").and_then(|v| v.as_bool()) != Some(true)
                    && u.raw_json.get("prerelease").and_then(|v| v.as_bool()) != Some(true)
            });
            if let Some(ref update) = update {
                validate_download_url(&update.download_url)?;
            }
            let checked_at = chrono::Utc::now().to_rfc3339();
            self.db.set_setting("update_checked_at", &checked_at)?;
            Ok::<_, AppError>((update, checked_at))
        }
        .await;
        match result {
            Ok((update, checked_at)) => Ok(self.publish(|s| {
                s.snapshot.checked_at = Some(checked_at);
                s.snapshot.phase = if update.is_some() {
                    UpdatePhase::Available
                } else {
                    UpdatePhase::UpToDate
                };
                if let Some(ref update) = update {
                    s.snapshot.latest_version = Some(update.version.clone());
                    s.snapshot.notes = update.body.clone();
                    s.snapshot.dismissed =
                        !manual && s.snapshot.skipped_version.as_deref() == Some(&update.version);
                }
                s.update = update;
            })),
            Err(error) => {
                Err(self.fail("无法检查更新：网络不可用或发布版本尚未提供更新信息", error))
            }
        }
    }

    pub async fn download(&self) -> AppResult<UpdateSnapshot> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| update_error("更新操作正在进行，请稍候"))?;
        if self.inner.lock().verified_bytes.is_some() {
            return Ok(self.snapshot());
        }
        let mut update = self
            .inner
            .lock()
            .update
            .clone()
            .ok_or_else(|| update_error("请先检查更新"))?;
        // Downloads of the bundled audio engine may take longer than the metadata request.
        update.timeout = Some(Duration::from_secs(30 * 60));
        let (cancel, canceled) = oneshot::channel();
        self.publish(|s| {
            s.cancel = Some(cancel);
            s.snapshot.phase = UpdatePhase::Downloading;
            s.snapshot.downloaded_bytes = 0;
            s.snapshot.total_bytes = None;
            s.snapshot.error = None;
        });
        let mut last_emit = Instant::now();
        let download = update.download(
            |chunk, total| {
                {
                    let mut state = self.inner.lock();
                    state.snapshot.downloaded_bytes += chunk as u64;
                    state.snapshot.total_bytes = total;
                }
                if last_emit.elapsed() >= Duration::from_millis(150) {
                    self.publish(|_| {});
                    last_emit = Instant::now();
                }
            },
            || {
                self.publish(|s| s.snapshot.phase = UpdatePhase::Verifying);
            },
        );
        let result = tokio::select! { biased;
            _ = canceled => None,
            result = download => Some(result),
        };
        self.inner.lock().cancel = None;
        match result {
            None => Ok(self.publish(|s| {
                s.snapshot.phase = UpdatePhase::Available;
                s.snapshot.downloaded_bytes = 0;
                s.snapshot.total_bytes = None;
            })),
            Some(Ok(bytes)) => Ok(self.publish(|s| {
                s.snapshot.total_bytes = Some(bytes.len() as u64);
                s.snapshot.downloaded_bytes = bytes.len() as u64;
                s.verified_bytes = Some(bytes);
                s.snapshot.phase = UpdatePhase::Ready;
            })),
            Some(Err(error)) => Err(self.fail("更新下载或签名验证失败，请重试下载", error)),
        }
    }

    pub fn cancel_download(&self) -> UpdateSnapshot {
        if let Some(cancel) = self.inner.lock().cancel.take() {
            let _ = cancel.send(());
        }
        self.snapshot()
    }

    pub async fn dismiss(&self, skip: bool) -> AppResult<UpdateSnapshot> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| update_error("请先取消正在进行的更新操作"))?;
        if skip {
            let version = self
                .snapshot()
                .latest_version
                .ok_or_else(|| update_error("当前没有可跳过的版本"))?;
            self.db.set_setting("update_skipped_version", &version)?;
        }
        Ok(self.publish(|s| {
            s.snapshot.dismissed = true;
            if skip {
                s.snapshot.skipped_version = s.snapshot.latest_version.clone();
                s.verified_bytes = None;
                s.snapshot.phase = UpdatePhase::Available;
                s.snapshot.downloaded_bytes = 0;
                s.snapshot.total_bytes = None;
            }
        }))
    }
}

impl UpdateService {
    pub async fn install(&self, playback: &PlaybackService) -> AppResult<UpdateSnapshot> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| update_error("更新操作正在进行，请稍候"))?;
        let update = {
            let state = self.inner.lock();
            if state.verified_bytes.is_none() {
                return Err(update_error("请先下载并验证更新包"));
            }
            state
                .update
                .clone()
                .ok_or_else(|| update_error("请先检查更新"))?
        };
        self.publish(|s| {
            s.snapshot.phase = UpdatePhase::Installing;
            s.snapshot.error = None;
        });
        if let Some(window) = self.app.get_webview_window("main") {
            crate::commands::capture_and_save_window_placement(&window, &self.db);
        }
        // A failed checkpoint/save must abort installation rather than losing listening progress.
        if let Err(error) = playback.prepare_for_update().await {
            return Err(self.fail("保存收听状态失败，已停止安装", error));
        }
        let bytes = self
            .inner
            .lock()
            .verified_bytes
            .take()
            .expect("verified package under operation lock");
        let service = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            let result = update.restart_after_install(true).install(&bytes);
            // On Windows success exits the process. Keep the verified package on failure.
            service.inner.lock().verified_bytes = Some(bytes);
            result
        })
        .await;
        match result {
            Ok(Ok(())) => {
                self.app.restart();
            }
            Ok(Err(error)) => Err(self.fail("无法启动更新安装程序，可重试安装", error)),
            Err(error) => Err(self.fail("更新安装任务异常，可重新下载更新", error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    #[ignore = "requires locally built signed release artifacts"]
    async fn verify_packaged_installers_against_embedded_public_key_t1403() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("src-tauri/tauri.conf.json")).unwrap())
                .unwrap();
        let release_dir = root
            .join("output/release")
            .join(format!("v{}", config["version"].as_str().unwrap()));
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(release_dir.join("latest.json")).unwrap())
                .unwrap();
        let server = MockServer::start().await;
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "pubkey": config["plugins"]["updater"]["pubkey"], "requireSignedVersion": true,
                "dangerousInsecureTransportProtocol": true
            }),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        for platform in ["windows-x86_64-nsis", "windows-x86_64-msi"] {
            server.reset().await;
            let entry = &manifest["platforms"][platform];
            let url = url::Url::parse(entry["url"].as_str().unwrap()).unwrap();
            let filename =
                urlencoding::decode(url.path_segments().unwrap().next_back().unwrap()).unwrap();
            let bytes = std::fs::read(release_dir.join(filename.as_ref())).unwrap();
            let metadata = serde_json::json!({ "version": manifest["version"], "platforms": {
                "windows-x86_64": { "url": format!("{}/installer", server.uri()), "signature": entry["signature"] }
            }});
            Mock::given(path("/latest.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(metadata))
                .mount(&server)
                .await;
            Mock::given(path("/installer"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
                .mount(&server)
                .await;
            let update = app
                .updater_builder()
                .endpoints(vec![format!("{}/latest.json", server.uri())
                    .parse()
                    .unwrap()])
                .unwrap()
                .version_comparator(|_, _| true)
                .build()
                .unwrap()
                .check()
                .await
                .unwrap()
                .unwrap();
            let verified = update.download(|_, _| {}, || {}).await.unwrap();
            assert!(verified.len() > 1024 * 1024, "expected a real installer");
        }
    }

    fn test_service(
        db: Database,
        endpoint: &str,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        UpdateService<tauri::test::MockRuntime>,
    ) {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert("updater".into(), serde_json::json!({
            "endpoints": [endpoint], "pubkey": include_str!("../../../tests/fixtures/updater/public.key.txt").trim(),
            "requireSignedVersion": true, "dangerousInsecureTransportProtocol": true
        }));
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let service = UpdateService::new(db, app.handle().clone()).unwrap();
        (app, service)
    }

    fn manifest(version: &str) -> serde_json::Value {
        serde_json::json!({"version": version, "notes": "Release notes", "platforms": {
            "windows-x86_64": {"url": "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v99.0.0/setup.exe", "signature": "invalid-test-signature"}
        }})
    }

    #[tokio::test]
    async fn valid_signature_becomes_ready_but_tampered_version_is_rejected_t1403() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let (_app, service) = test_service(db, &format!("{}/latest.json", server.uri()));
        Mock::given(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(manifest("99.0.0")))
            .mount(&server)
            .await;
        Mock::given(path("/signed.exe"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(
                include_bytes!("../../../tests/fixtures/updater/payload.txt").as_slice(),
            ))
            .mount(&server)
            .await;
        service.check(true).await.unwrap();
        {
            let mut state = service.inner.lock();
            let update = state.update.as_mut().unwrap();
            update.download_url = format!("{}/signed.exe", server.uri()).parse().unwrap();
            update.signature = include_str!("../../../tests/fixtures/updater/payload.txt.sig")
                .trim()
                .into();
        }
        assert_eq!(service.download().await.unwrap().phase, UpdatePhase::Ready);
        assert!(service.inner.lock().verified_bytes.is_some());
        {
            let mut state = service.inner.lock();
            state.verified_bytes = None;
            state.update.as_mut().unwrap().version = "100.0.0".into();
        }
        assert!(service.download().await.is_err());
        assert!(service.inner.lock().verified_bytes.is_none());
    }

    #[tokio::test]
    async fn check_preferences_skip_errors_and_verified_download_gate_t1406() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let endpoint = format!("{}/latest.json", server.uri());
        let (_app, service) = test_service(db.clone(), &endpoint);
        service.set_auto_check(false).unwrap();
        assert_eq!(service.check(false).await.unwrap().phase, UpdatePhase::Idle);
        assert!(server.received_requests().await.unwrap().is_empty());

        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(manifest("99.0.0")))
            .mount(&server)
            .await;
        let available = service.check(true).await.unwrap();
        assert_eq!(available.phase, UpdatePhase::Available);
        assert!(available.checked_at.is_some());
        service.dismiss(true).await.unwrap();
        service.set_auto_check(true).unwrap();
        assert!(service.check(false).await.unwrap().dismissed);
        assert!(!service.check(true).await.unwrap().dismissed);
        assert_eq!(
            db.get_setting("update_skipped_version").unwrap().as_deref(),
            Some("99.0.0")
        );
        let (_other, restored) = test_service(db.clone(), &endpoint);
        assert_eq!(
            restored.snapshot().skipped_version.as_deref(),
            Some("99.0.0")
        );

        Mock::given(method("GET"))
            .and(path("/bad.exe"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"MZ unsigned executable"))
            .mount(&server)
            .await;
        service.inner.lock().update.as_mut().unwrap().download_url =
            format!("{}/bad.exe", server.uri()).parse().unwrap();
        assert!(service.download().await.is_err());
        assert_eq!(service.snapshot().phase, UpdatePhase::Error);
        assert!(
            service.inner.lock().verified_bytes.is_none(),
            "unsigned bytes must never become installable"
        );

        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(service.check(true).await.is_err());
        assert!(service.snapshot().latest_version.is_none());
    }

    #[tokio::test]
    async fn no_update_prerelease_and_cancel_download_t1406() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let (_app, service) = test_service(db, &format!("{}/latest.json", server.uri()));
        for version in [service.snapshot().current_version, "99.0.0-beta.1".into()] {
            server.reset().await;
            Mock::given(path("/latest.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(manifest(&version)))
                .mount(&server)
                .await;
            assert_eq!(
                service.check(true).await.unwrap().phase,
                UpdatePhase::UpToDate
            );
        }
        server.reset().await;
        Mock::given(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(manifest("99.0.0")))
            .mount(&server)
            .await;
        service.check(true).await.unwrap();
        Mock::given(path("/slow.exe"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(10))
                    .set_body_bytes(b"MZ"),
            )
            .mount(&server)
            .await;
        service.inner.lock().update.as_mut().unwrap().download_url =
            format!("{}/slow.exe", server.uri()).parse().unwrap();
        let worker = service.clone();
        let download = tokio::spawn(async move { worker.download().await });
        tokio::time::timeout(Duration::from_secs(3), async {
            while service.snapshot().phase != UpdatePhase::Downloading {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            service.check(true).await.is_err(),
            "concurrent operations must be rejected"
        );
        service.cancel_download();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), download)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .phase,
            UpdatePhase::Available
        );
        assert!(service.inner.lock().verified_bytes.is_none());
    }
    #[test]
    fn stable_versions_use_semver_precedence_t1401() {
        let current = Version::parse("1.9.0").unwrap();
        for version in ["1.10.0", "2.0.0"] {
            assert!(is_newer_stable(&current, &Version::parse(version).unwrap()));
        }
        for version in ["1.8.0", "1.9.0", "1.9.0+build2", "2.0.0-beta.1"] {
            assert!(!is_newer_stable(
                &current,
                &Version::parse(version).unwrap()
            ));
        }
    }
    #[test]
    fn only_project_https_release_assets_are_allowed_t1403() {
        assert!(validate_download_url(
            &url::Url::parse(
                "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1.2.0/setup.exe"
            )
            .unwrap()
        )
        .is_ok());
        for value in ["http://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1/setup.exe",
            "https://evil.example/setup.exe", "https://github.com/other/repo/releases/download/v1/setup.exe",
            "https://user:secret@github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1/setup.exe"] {
            assert!(validate_download_url(&url::Url::parse(value).unwrap()).is_err());
        }
    }
}
