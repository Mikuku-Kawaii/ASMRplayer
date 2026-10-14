//! T1510: release text is informational, never installer metadata or executable HTML.
use crate::db::Database;
use crate::domain::{AppError, AppResult, ErrorCode};
use parking_lot::Mutex;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const CACHE_KEY: &str = "update_release_history_v1";
const VISIT_KEY: &str = "update_version_visit_v1";
const API: &str = "https://api.github.com/repos/Mikuku-Kawaii/ASMRplayer/releases";
const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const MAX_RELEASES: usize = 30;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseEntry {
    pub version: String,
    pub title: String,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub url: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Cache {
    entries: Vec<ReleaseEntry>,
    fetched_at: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct Visit {
    highest_version: String,
    pending_version: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseHistory {
    pub current_version: String,
    pub entries: Vec<ReleaseEntry>,
    pub fetched_at: Option<String>,
    pub pending_version: Option<String>,
    pub error: Option<AppError>,
}

pub struct ReleaseHistoryService {
    db: Database,
    current: String,
    storage: Mutex<()>,
    refresh: tokio::sync::Mutex<()>,
}

fn error(message: &str) -> AppError {
    AppError::new(ErrorCode::InternalError, message)
        .with_hint("已缓存的发布说明仍可查看；联网后可刷新重试。")
}

fn stable(value: &str) -> Option<Version> {
    Version::parse(value.strip_prefix('v').unwrap_or(value))
        .ok()
        .filter(|v| v.pre.is_empty())
}

fn release_url(tag: &str) -> String {
    format!(
        "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/tag/{}",
        urlencoding::encode(tag)
    )
}

fn normalize(mut entry: ReleaseEntry) -> Option<ReleaseEntry> {
    entry.version = stable(&entry.version)?.to_string();
    let url = super::updater::validate_web_link(&entry.url).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url
            .path()
            .starts_with("/Mikuku-Kawaii/ASMRplayer/releases/tag/")
    {
        return None;
    }
    entry.title = entry.title.chars().take(180).collect();
    // Keep one extra character so ReleaseNotes can show its truncation notice.
    entry.notes = entry.notes.map(|s| s.chars().take(50_001).collect());
    entry.published_at = entry
        .published_at
        .filter(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok());
    Some(entry)
}

fn parse_release(value: &serde_json::Value) -> Option<ReleaseEntry> {
    if value.get("draft")?.as_bool()? || value.get("prerelease")?.as_bool()? {
        return None;
    }
    let tag = value.get("tag_name")?.as_str()?;
    normalize(ReleaseEntry {
        version: stable(tag)?.to_string(),
        title: value
            .get("name")
            .and_then(|s| s.as_str())
            .unwrap_or(tag)
            .to_owned(),
        notes: value
            .get("body")
            .and_then(|s| s.as_str())
            .map(str::to_owned),
        published_at: value
            .get("published_at")
            .and_then(|s| s.as_str())
            .map(str::to_owned),
        url: release_url(tag),
    })
}

impl ReleaseHistoryService {
    pub fn new(db: Database, current: String) -> AppResult<Self> {
        let service = Self {
            db,
            current,
            storage: Mutex::new(()),
            refresh: tokio::sync::Mutex::new(()),
        };
        let _guard = service.storage.lock();
        let mut visit = service.visit()?;
        if let (Ok(current), Ok(highest)) = (
            Version::parse(&service.current),
            Version::parse(&visit.highest_version),
        ) {
            if current.cmp_precedence(&highest).is_gt() {
                visit.highest_version = service.current.clone();
                visit.pending_version = Some(service.current.clone());
            }
        }
        service.save_visit(&visit)?;
        drop(_guard);
        Ok(service)
    }

    fn visit(&self) -> AppResult<Visit> {
        Ok(self
            .db
            .get_setting(VISIT_KEY)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Visit {
                highest_version: self.current.clone(),
                pending_version: None,
            }))
    }

    fn save_visit(&self, visit: &Visit) -> AppResult<()> {
        self.db.set_setting(
            VISIT_KEY,
            &serde_json::to_string(visit).map_err(|_| error("无法保存版本提醒状态"))?,
        )
    }

    fn cache(&self) -> AppResult<Cache> {
        let raw = self.db.get_setting(CACHE_KEY)?;
        let cache = raw
            .filter(|s| s.len() <= 8 * 1024 * 1024)
            .and_then(|s| serde_json::from_str::<Cache>(&s).ok())
            .unwrap_or_default();
        Ok(Cache {
            entries: cache
                .entries
                .into_iter()
                .filter_map(normalize)
                .take(MAX_RELEASES + 1)
                .collect(),
            fetched_at: cache.fetched_at,
        })
    }

    pub fn snapshot(&self) -> AppResult<ReleaseHistory> {
        let _guard = self.storage.lock();
        let cache = self.cache()?;
        Ok(ReleaseHistory {
            current_version: self.current.clone(),
            entries: cache.entries,
            fetched_at: cache.fetched_at,
            pending_version: self.visit()?.pending_version.filter(|v| v == &self.current),
            error: None,
        })
    }

    /// Returns false if another mount/window already claimed this installed version.
    pub fn claim_notice(&self, version: &str) -> AppResult<bool> {
        let _guard = self.storage.lock();
        let mut visit = self.visit()?;
        if version != self.current || visit.pending_version.as_deref() != Some(version) {
            return Ok(false);
        }
        visit.pending_version = None;
        self.save_visit(&visit)?;
        Ok(true)
    }

    pub fn remember(
        &self,
        entries: Vec<ReleaseEntry>,
        fetched_at: Option<String>,
    ) -> AppResult<()> {
        let _guard = self.storage.lock();
        let mut cache = self.cache()?;
        for entry in entries.into_iter().filter_map(normalize) {
            cache.entries.retain(|old| old.version != entry.version);
            cache.entries.push(entry);
        }
        cache
            .entries
            .sort_by(|a, b| stable(&b.version).cmp(&stable(&a.version)));
        let current = cache
            .entries
            .iter()
            .find(|e| e.version == self.current)
            .cloned();
        cache.entries.truncate(MAX_RELEASES);
        if let Some(entry) =
            current.filter(|e| !cache.entries.iter().any(|v| v.version == e.version))
        {
            cache.entries.push(entry);
        }
        if fetched_at.is_some() {
            cache.fetched_at = fetched_at;
        }
        self.db.set_setting(
            CACHE_KEY,
            &serde_json::to_string(&cache).map_err(|_| error("无法缓存发布说明"))?,
        )
    }

    pub fn remember_update(
        &self,
        version: &str,
        notes: Option<String>,
        published_at: Option<String>,
    ) -> AppResult<()> {
        self.remember(
            vec![ReleaseEntry {
                version: version.to_owned(),
                title: format!("v{version}"),
                notes,
                published_at,
                url: release_url(&format!("v{version}")),
            }],
            None,
        )
    }

    pub async fn refresh(&self) -> AppResult<ReleaseHistory> {
        self.refresh_from(API).await
    }

    async fn refresh_from(&self, endpoint: &str) -> AppResult<ReleaseHistory> {
        let _operation = self
            .refresh
            .try_lock()
            .map_err(|_| error("正在刷新更新历史，请稍候"))?;
        let result = async {
            let mut builder = reqwest::Client::builder()
                .user_agent("ASMR-Player-Release-History")
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none());
            if let Some(proxy) = self
                .db
                .get_setting("proxy_url")?
                .filter(|s| !s.trim().is_empty())
            {
                builder =
                    builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| error("代理地址无效"))?);
            }
            let client = builder
                .build()
                .map_err(|_| error("无法初始化发布说明请求"))?;
            let value = fetch_json(&client, &format!("{endpoint}?per_page=30")).await?;
            let values = value
                .as_array()
                .ok_or_else(|| error("发布说明响应格式无效"))?;
            if values.len() > MAX_RELEASES {
                return Err(error("发布说明条目过多"));
            }
            let mut entries: Vec<_> = values.iter().filter_map(parse_release).collect();
            let mut warning = None;
            // Keep the installed release accessible even if it falls outside the recent page.
            if !entries.iter().any(|e| e.version == self.current) && stable(&self.current).is_some()
            {
                match fetch_json(&client, &format!("{endpoint}/tags/v{}", self.current)).await {
                    Ok(value) => {
                        if let Some(entry) =
                            parse_release(&value).filter(|e| e.version == self.current)
                        {
                            entries.push(entry);
                        }
                    }
                    Err(_) => {
                        warning = Some(error("历史已刷新，当前版本说明暂未获取；已有缓存已保留"))
                    }
                }
            }
            self.remember(entries, Some(chrono::Utc::now().to_rfc3339()))?;
            Ok::<_, AppError>(warning)
        }
        .await;
        let mut snapshot = self.snapshot()?;
        snapshot.error = match result {
            Ok(warning) => warning,
            Err(e) => Some(e),
        };
        Ok(snapshot)
    }
}

async fn fetch_json(client: &reqwest::Client, url: &str) -> AppResult<serde_json::Value> {
    let mut response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| error("无法刷新更新历史：网络不可用或发布服务暂不可用"))?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE as u64)
    {
        return Err(error("发布说明响应过大"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| error("发布说明接收中断"))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE {
            return Err(error("发布说明响应过大"));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| error("发布说明响应格式无效"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

    fn database() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        (dir, db)
    }
    fn release(version: &str) -> serde_json::Value {
        serde_json::json!({"tag_name":format!("v{version}"),"name":"新增功能与设置变化","body":"## 新增\n- 下载时段\n## 设置变化\n- 新增启动恢复偏好","draft":false,"prerelease":false,"published_at":"2026-10-14T00:00:00Z"})
    }

    #[test]
    fn installed_version_notice_survives_restart_and_is_claimed_once() {
        let (_dir, db) = database();
        let first = ReleaseHistoryService::new(db.clone(), "1.0.0".into()).unwrap();
        assert!(first.snapshot().unwrap().pending_version.is_none());
        first
            .remember_update("2.0.0", Some("cached before installation".into()), None)
            .unwrap();
        // Discovering/downloading an update does not mark it as installed.
        assert!(!first.claim_notice("2.0.0").unwrap());
        let upgrade = ReleaseHistoryService::new(db.clone(), "2.0.0".into()).unwrap();
        assert_eq!(
            upgrade.snapshot().unwrap().pending_version.as_deref(),
            Some("2.0.0")
        );
        drop(upgrade);
        let restart = ReleaseHistoryService::new(db.clone(), "2.0.0".into()).unwrap();
        assert!(!restart.claim_notice("1.0.0").unwrap());
        assert!(restart.claim_notice("2.0.0").unwrap());
        assert!(!restart.claim_notice("2.0.0").unwrap());
        assert_eq!(
            restart.snapshot().unwrap().entries[0].notes.as_deref(),
            Some("cached before installation")
        );
        let downgrade = ReleaseHistoryService::new(db.clone(), "1.0.0".into()).unwrap();
        assert!(downgrade.snapshot().unwrap().pending_version.is_none());
        let revisit = ReleaseHistoryService::new(db.clone(), "2.0.0".into()).unwrap();
        assert!(revisit.snapshot().unwrap().pending_version.is_none());
        let jump = ReleaseHistoryService::new(db, "4.0.0".into()).unwrap();
        assert!(jump.claim_notice("4.0.0").unwrap());
    }

    #[test]
    fn cache_bounds_deduplicates_sorts_semver_and_pins_installed_release() {
        let (_dir, db) = database();
        let service = ReleaseHistoryService::new(db, "1.0.0".into()).unwrap();
        let entries = (1..=40)
            .map(|i| parse_release(&release(&format!("{i}.0.0"))).unwrap())
            .collect();
        service
            .remember(entries, Some("2026-10-14T00:00:00Z".into()))
            .unwrap();
        let history = service.snapshot().unwrap();
        assert_eq!(history.entries.len(), 31);
        assert_eq!(history.entries[0].version, "40.0.0");
        assert_eq!(history.entries.last().unwrap().version, "1.0.0");
        service
            .remember_update("40.0.0", Some("新".repeat(60_000)), None)
            .unwrap();
        let history = service.snapshot().unwrap();
        assert_eq!(history.entries.len(), 31);
        assert_eq!(
            history.entries[0].notes.as_ref().unwrap().chars().count(),
            50_001
        );
        assert_eq!(history.fetched_at.as_deref(), Some("2026-10-14T00:00:00Z"));
    }

    #[test]
    fn rejects_nonstable_releases_and_unsafe_cached_links() {
        assert!(parse_release(&release("1.0.0-beta.1")).is_none());
        for field in ["draft", "prerelease"] {
            let mut value = release("1.0.0");
            value[field] = true.into();
            assert!(parse_release(&value).is_none());
        }
        let mut entry = parse_release(&release("1.0.0")).unwrap();
        entry.url =
            "https://github.com.evil.invalid/Mikuku-Kawaii/ASMRplayer/releases/tag/v1.0.0".into();
        assert!(normalize(entry).is_none());
        let (_dir, db) = database();
        db.set_setting(CACHE_KEY, "broken JSON").unwrap();
        let service = ReleaseHistoryService::new(db, "1.0.0".into()).unwrap();
        assert!(service.snapshot().unwrap().entries.is_empty());
    }

    #[tokio::test]
    async fn refresh_and_offline_failure_keep_cached_notes_and_timestamp() {
        let (_dir, db) = database();
        let service = ReleaseHistoryService::new(db.clone(), "1.0.0".into()).unwrap();
        let server = MockServer::start().await;
        Mock::given(path("/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![
                release("1.0.0"),
                release("1.10.0"),
                release("1.2.0"),
                release("2.0.0-beta.1"),
            ]))
            .mount(&server)
            .await;
        let fresh = service
            .refresh_from(&format!("{}/releases", server.uri()))
            .await
            .unwrap();
        assert!(fresh.error.is_none());
        assert_eq!(
            fresh
                .entries
                .iter()
                .map(|e| e.version.as_str())
                .collect::<Vec<_>>(),
            vec!["1.10.0", "1.2.0", "1.0.0"]
        );
        server.reset().await;
        Mock::given(path("/releases"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let offline = service
            .refresh_from(&format!("{}/releases", server.uri()))
            .await
            .unwrap();
        assert!(offline.error.is_some());
        assert_eq!(offline.fetched_at, fresh.fetched_at);
        assert_eq!(offline.entries.len(), 3);
        let reopened = ReleaseHistoryService::new(db, "1.0.0".into())
            .unwrap()
            .snapshot()
            .unwrap();
        assert_eq!(reopened.entries.len(), 3);
    }

    #[tokio::test]
    async fn fetches_installed_release_outside_recent_page_and_preserves_it_on_failure() {
        let (_dir, db) = database();
        let service = ReleaseHistoryService::new(db, "1.0.0".into()).unwrap();
        let server = MockServer::start().await;
        Mock::given(path("/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![release("3.0.0")]))
            .mount(&server)
            .await;
        Mock::given(path("/releases/tags/v1.0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(release("1.0.0")))
            .mount(&server)
            .await;
        let fresh = service
            .refresh_from(&format!("{}/releases", server.uri()))
            .await
            .unwrap();
        assert!(fresh.error.is_none());
        assert_eq!(fresh.entries.len(), 2);
        server.reset().await;
        Mock::given(path("/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![release("4.0.0")]))
            .mount(&server)
            .await;
        let partial = service
            .refresh_from(&format!("{}/releases", server.uri()))
            .await
            .unwrap();
        assert!(partial.error.is_some());
        assert_eq!(partial.entries.len(), 3);
        assert!(partial.entries.iter().any(|e| e.version == "1.0.0"));
    }

    #[tokio::test]
    async fn oversized_or_malformed_responses_do_not_replace_cache() {
        let (_dir, db) = database();
        let service = ReleaseHistoryService::new(db, "1.0.0".into()).unwrap();
        service
            .remember_update("1.0.0", Some("preserved".into()), None)
            .unwrap();
        let server = MockServer::start().await;
        for body in ["x".repeat(MAX_RESPONSE + 1), "{}".into()] {
            server.reset().await;
            Mock::given(path("/releases"))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(&server)
                .await;
            let snapshot = service
                .refresh_from(&format!("{}/releases", server.uri()))
                .await
                .unwrap();
            assert!(snapshot.error.is_some());
            assert!(snapshot.fetched_at.is_none());
            assert_eq!(snapshot.entries[0].notes.as_deref(), Some("preserved"));
        }
    }
}
