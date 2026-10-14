use super::{
    download::DownloadService,
    playback::PlaybackService,
    storage_file::{Identity, LockedFile},
};
use crate::{
    db::Database,
    domain::{AppError, AppResult, ErrorCode},
};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Playback,
    Metadata,
    Incomplete,
    Offline,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageEntry {
    pub id: String,
    pub category: Category,
    pub path: String,
    pub bytes: u64,
    pub reason: Option<String>,
    #[serde(skip)]
    root: PathBuf,
    #[serde(skip)]
    identity: Option<Identity>,
    #[serde(skip)]
    version: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOverview {
    pub entries: Vec<StorageEntry>,
    pub warnings: Vec<String>,
    pub database_metadata_bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPreview {
    pub token: String,
    pub entries: Vec<StorageEntry>,
    pub reclaimable_bytes: u64,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub deleted_count: usize,
    pub freed_bytes: u64,
    pub skipped: Vec<String>,
}
struct StoredPreview {
    created: Instant,
    entries: Vec<StorageEntry>,
}
#[derive(Clone)]
pub struct StorageService {
    db: Database,
    downloads: DownloadService,
    playback: PlaybackService,
    previews: Arc<Mutex<HashMap<String, StoredPreview>>>,
}
#[derive(Clone)]
struct Catalog {
    entries: Vec<StorageEntry>,
    protected: Vec<PathBuf>,
    references: HashSet<String>,
    queued: HashSet<String>,
    metadata_bytes: u64,
}
fn key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    value
        .strip_prefix(r"\\?\UNC\")
        .map(|p| format!(r"\\{p}"))
        .unwrap_or_else(|| value.trim_start_matches(r"\\?\").to_owned())
        .to_lowercase()
}
fn resolve_protection(c: &Catalog) -> Catalog {
    let mut resolved = c.clone();
    resolved
        .protected
        .extend(c.protected.iter().filter_map(|p| p.canonicalize().ok()));
    resolved.references.extend(
        c.references
            .iter()
            .filter_map(|p| Path::new(p).canonicalize().ok())
            .map(|p| key(&p)),
    );
    resolved
}
fn within(path: &Path, root: &Path) -> bool {
    let p = key(path);
    let r = key(root);
    p == r || p.starts_with(&format!("{}\\", r.trim_end_matches('\\')))
}
fn error(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}
fn entry(
    id: String,
    category: Category,
    root: &Path,
    path: PathBuf,
    reason: Option<String>,
) -> StorageEntry {
    StorageEntry {
        id,
        category,
        path: path.to_string_lossy().into(),
        bytes: 0,
        reason,
        root: root.to_owned(),
        identity: None,
        version: None,
    }
}
fn safe_relative(root: &Path, relative: &str) -> Option<PathBuf> {
    let p = PathBuf::from(relative.replace('\\', "/"));
    if relative.contains(':')
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
        || p.as_os_str().is_empty()
    {
        None
    } else {
        Some(root.join(p))
    }
}
fn catalog(conn: &Connection, cache: &Path, fallback: &Path) -> AppResult<Catalog> {
    let root: PathBuf = conn.query_row("SELECT COALESCE((SELECT NULLIF(TRIM(value_json),'') FROM app_settings WHERE key='download_dir'), ?1)", [fallback.to_string_lossy().as_ref()], |r| r.get::<_, String>(0))?.into();
    let mut c = Catalog {
        entries: vec![],
        protected: vec![],
        references: HashSet::new(),
        queued: HashSet::new(),
        metadata_bytes: 0,
    };
    let mut s = conn.prepare("SELECT absolute_path FROM library_roots UNION SELECT absolute_path FROM media_sources WHERE source_kind='local_imported' AND absolute_path IS NOT NULL")?;
    for p in s.query_map([], |r| r.get::<_, String>(0))? {
        c.protected.push(p?.into());
    }
    let mut s = conn.prepare("SELECT cover_ref FROM works WHERE cover_ref IS NOT NULL UNION SELECT absolute_path FROM attachments WHERE absolute_path IS NOT NULL UNION SELECT value_text FROM work_overrides WHERE field='cover_ref' AND value_text IS NOT NULL")?;
    for p in s.query_map([], |r| r.get::<_, String>(0))? {
        c.references.insert(key(Path::new(&p?)));
    }
    let mut s = conn.prepare("SELECT raw_metadata_json FROM work_sources")?;
    for raw in s.query_map([], |r| r.get::<_, String>(0))? {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw?) {
            if let Some(path) = json.get("cached_cover_path").and_then(|v| v.as_str()) {
                c.references.insert(key(Path::new(path)));
            }
        }
    }
    let mut s = conn.prepare("SELECT version_id FROM queue_items")?;
    for v in s.query_map([], |r| r.get::<_, String>(0))? {
        c.queued.insert(v?);
    }
    c.metadata_bytes = conn.query_row("SELECT COALESCE(SUM(length(CAST(raw_metadata_json AS BLOB)) + COALESCE(length(CAST(cached_tree_json AS BLOB)),0)),0) FROM work_sources", [], |r| r.get(0))?;
    let mut s =
        conn.prepare("SELECT id, temp_relative_path, status, final_renamed FROM downloads")?;
    for row in s.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })? {
        let (id, rel, status, renamed) = row?;
        if let Some(path) =
            safe_relative(&root, &rel).filter(|p| p.extension().is_some_and(|e| e == "part"))
        {
            let reason = if renamed != 0
                || !matches!(
                    status.as_str(),
                    "paused" | "failed" | "canceled" | "completed"
                ) {
                Some("下载任务正在运行或等待执行".into())
            } else {
                None
            };
            c.entries.push(entry(
                format!("part:{id}"),
                Category::Incomplete,
                &root,
                path,
                reason,
            ));
        }
    }
    let mut s = conn.prepare("SELECT id, absolute_path, relative_path, version_id FROM media_sources WHERE source_kind='local_downloaded' AND availability != 'missing'")?;
    for row in s.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (id, absolute, relative, version) = row?;
        let path = absolute
            .map(PathBuf::from)
            .or_else(|| relative.and_then(|p| safe_relative(&root, &p)));
        if let Some(path) = path {
            let reason = if !within(&path, &root) {
                Some("文件不在当前下载目录内".into())
            } else if c.queued.contains(&version) {
                Some("播放队列仍在使用此版本".into())
            } else {
                None
            };
            let mut e = entry(
                format!("offline:{id}"),
                Category::Offline,
                &root,
                path,
                reason,
            );
            e.version = Some(version);
            c.entries.push(e);
        }
    }
    // Cache candidates are enumerated separately; the catalog only contains owned downloads.
    let _ = cache;
    Ok(c)
}
impl StorageService {
    pub fn new(db: Database, downloads: DownloadService, playback: PlaybackService) -> Self {
        Self {
            db,
            downloads,
            playback,
            previews: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    fn catalog(&self) -> AppResult<Catalog> {
        self.db
            .with_conn(|c| catalog(c, &self.db.cache_dir, &self.db.data_dir.join("downloads")))
    }
    pub fn overview(&self) -> AppResult<StorageOverview> {
        let mut c = resolve_protection(&self.catalog()?);
        let mut warnings = vec![];
        let roots: HashSet<_> = c.entries.iter().map(|e| e.root.clone()).collect();
        for root in roots {
            if !root.is_dir() {
                warnings.push(format!(
                    "下载目录不可访问，缺失文件未计入用量：{}",
                    root.display()
                ));
            }
        }
        // Walk only application cache, without following any link. Directory junctions are
        // rejected again by LockedFile, including ancestors outside the cache root.
        if self.db.cache_dir.exists() {
            for item in walkdir::WalkDir::new(&self.db.cache_dir).follow_links(false) {
                match item {
                    Ok(item) if item.file_type().is_file() => {
                        let rel = item
                            .path()
                            .strip_prefix(&self.db.cache_dir)
                            .unwrap_or(item.path());
                        let first = rel
                            .components()
                            .next()
                            .map(|p| p.as_os_str().to_string_lossy().to_string())
                            .unwrap_or_default();
                        let category = if matches!(
                            first.as_str(),
                            "covers" | "thumbnails" | "waveforms" | "subtitles"
                        ) {
                            Category::Metadata
                        } else {
                            Category::Playback
                        };
                        let known = matches!(
                            first.as_str(),
                            "covers" | "thumbnails" | "waveforms" | "subtitles" | "smtc_lrc"
                        ) || (rel.components().count() == 1
                            && rel.extension().is_some_and(|e| e == "cache"));
                        c.entries.push(entry(
                            format!("cache:{}", key(item.path())),
                            category,
                            &self.db.cache_dir,
                            item.path().into(),
                            (!known).then(|| "未识别的缓存文件，保留".into()),
                        ));
                    }
                    Ok(item) if item.file_type().is_symlink() => {
                        warnings.push(format!("跳过链接：{}", item.path().display()))
                    }
                    Err(e) => warnings.push(format!("无法读取部分缓存：{e}")),
                    _ => {}
                }
            }
        }
        let (active, versions) = self
            .playback
            .with_storage_guard(|active, versions| (active, versions.clone()));
        let workers = self.downloads.with_storage_workers(|ids| ids.clone());
        let mut entries = vec![];
        let mut seen = HashSet::new();
        // Prefer explicit media ownership when cache and download roots overlap.
        for mut e in c.entries.clone() {
            if !seen.insert(key(Path::new(&e.path))) {
                continue;
            }
            match fs::symlink_metadata(&e.path) {
                Ok(m) if m.is_file() => e.bytes = m.len(),
                Ok(_) => {
                    warnings.push(format!("跳过非普通文件：{}", e.path));
                    continue;
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if e.category == Category::Offline {
                        warnings.push(format!("离线文件不可访问，未计入用量：{}", e.path));
                    }
                    continue;
                }
                Err(err) => {
                    warnings.push(format!("无法统计 {}：{err}", e.path));
                    continue;
                }
            }
            protect(&mut e, &c, active, &versions, &workers);
            match LockedFile::open(Path::new(&e.path), false).and_then(|f| f.identity()) {
                Ok(id) => e.identity = Some(id),
                Err(err) => e.reason = Some(format!("无法安全访问：{err}")),
            }
            entries.push(e);
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(StorageOverview {
            entries,
            warnings,
            database_metadata_bytes: c.metadata_bytes,
        })
    }
    pub fn preview(&self, category: Category, ids: Vec<String>) -> AppResult<CleanupPreview> {
        let overview = self.overview()?;
        let ids: HashSet<_> = ids.into_iter().collect();
        if category == Category::Offline && ids.is_empty() {
            return Err(error("请先选择要删除的离线文件"));
        }
        let entries: Vec<_> = overview
            .entries
            .into_iter()
            .filter(|e| e.category == category && (ids.is_empty() || ids.contains(&e.id)))
            .collect();
        let reclaimable_bytes = entries
            .iter()
            .filter(|e| e.reason.is_none())
            .map(|e| e.bytes)
            .sum();
        let token = uuid::Uuid::new_v4().to_string();
        let mut previews = self.previews.lock();
        previews.retain(|_, p| p.created.elapsed() < Duration::from_secs(300));
        if previews.len() >= 8 {
            previews.clear();
        }
        previews.insert(
            token.clone(),
            StoredPreview {
                created: Instant::now(),
                entries: entries.clone(),
            },
        );
        Ok(CleanupPreview {
            token,
            entries,
            reclaimable_bytes,
        })
    }
    pub fn execute(&self, token: &str) -> AppResult<CleanupResult> {
        let preview = self
            .previews
            .lock()
            .remove(token)
            .ok_or_else(|| error("预览已失效，请重新预览"))?;
        if preview.created.elapsed() >= Duration::from_secs(300) {
            return Err(error("预览已过期，请重新预览"));
        }
        let mut result = CleanupResult::default();
        for e in preview.entries {
            if let Some(reason) = &e.reason {
                result.skipped.push(format!("{}：{reason}", e.path));
                continue;
            }
            let deleted = self.delete_entry(&e);
            match deleted {
                Ok(()) => {
                    result.deleted_count += 1;
                    result.freed_bytes += e.bytes;
                }
                Err(err) => result.skipped.push(format!("{}：{}", e.path, err.message)),
            }
        }
        self.downloads.emit_tasks_updated();
        Ok(result)
    }
    fn delete_entry(&self, expected: &StorageEntry) -> AppResult<()> {
        // Disk access happens before service locks. Exclusive handles prevent playback or
        // a writer from opening this file while final ownership checks run.
        let before = self.catalog()?;
        let resolved = resolve_protection(&before);
        // Reject short-name/trailing-dot aliases; comparisons below use the same
        // normalized physical path as imported media. Resolve outside DB/service locks.
        let normalized = Path::new(&expected.path).canonicalize()?;
        if key(&normalized) != key(Path::new(&expected.path)) {
            return Err(error("文件路径包含别名，保留以便核对"));
        }
        let file = LockedFile::open(Path::new(&expected.path), true)?;
        if Some(file.identity()?) != expected.identity {
            return Err(error("预览后文件已变化，请重新预览"));
        }
        self.playback.with_storage_guard(|active, versions| self.downloads.with_storage_workers(|workers| self.db.with_conn(|conn| {
            let mut c = catalog(conn, &self.db.cache_dir, &self.db.data_dir.join("downloads"))?;
            if c.protected != before.protected || c.references != before.references { return Err(error("媒体引用发生变化，请重新预览")); }
            c.protected = resolved.protected.clone(); c.references = resolved.references.clone();
            let mut current = if expected.id.starts_with("cache:") {
                // A directory setting may have changed so a formerly cached file now belongs to a download.
                if c.entries.iter().any(|e| key(Path::new(&e.path)) == key(Path::new(&expected.path))) { return Err(error("文件用途已变化")); }
                expected.clone()
            } else {
                c.entries.iter().find(|e| e.id == expected.id && e.path == expected.path && key(&e.root) == key(&expected.root)).cloned().ok_or_else(|| error("文件归属或下载目录已变化"))?
            };
            protect(&mut current, &c, active, versions, workers);
            if let Some(reason) = current.reason { return Err(error(&reason)); }
            let tx = conn.transaction()?;
            if let Some(id) = current.id.strip_prefix("part:") {
                tx.execute("UPDATE downloads SET downloaded_bytes=0, etag=NULL, last_modified=NULL, content_sha256=NULL, final_renamed=0, updated_at=?2 WHERE id=?1", params![id, chrono::Utc::now().to_rfc3339()])?;
            } else if let Some(id) = current.id.strip_prefix("offline:") {
                tx.execute("UPDATE media_sources SET availability='missing', updated_at=?2 WHERE id=?1 AND source_kind='local_downloaded'", params![id, chrono::Utc::now().to_rfc3339()])?;
                let relative = key(Path::new(&current.path)).strip_prefix(&format!("{}\\", key(&current.root).trim_end_matches('\\'))).unwrap_or("").replace('\\', "/");
                tx.execute("UPDATE downloads SET status='canceled', downloaded_bytes=0, final_renamed=0, updated_at=?2 WHERE version_id=(SELECT version_id FROM media_sources WHERE id=?1) AND status='completed' AND lower(replace(target_relative_path, char(92), '/'))=?3", params![id, chrono::Utc::now().to_rfc3339(), relative])?;
            }
            file.delete()?;
            if let Err(err) = tx.commit() {
                // The exclusive handle is still open, so cancel delete-on-close if
                // database persistence fails instead of losing the file and its index.
                file.retain()?;
                return Err(err.into());
            }
            Ok(())
        })))
    }
}
fn protect(
    e: &mut StorageEntry,
    c: &Catalog,
    active: bool,
    versions: &HashSet<String>,
    workers: &HashSet<String>,
) {
    if c.protected.iter().any(|p| within(Path::new(&e.path), p)) {
        e.reason = Some("保留导入目录和本地原始文件".into());
    } else if c.references.contains(&key(Path::new(&e.path))) {
        e.reason = Some("音频库仍在引用此封面或附件".into());
    } else if (active && e.category == Category::Playback)
        || e.version.as_ref().is_some_and(|v| versions.contains(v))
    {
        e.reason = Some("当前播放会话或预加载正在使用此文件".into());
    } else if !workers.is_empty() && matches!(e.category, Category::Offline | Category::Incomplete)
    {
        e.reason = Some("下载工作线程仍在运行，请暂停并稍后重新预览".into());
    } else if !within(Path::new(&e.path), &e.root) {
        e.reason = Some("文件不在受管理目录内".into());
    } else if c
        .entries
        .iter()
        .filter(|other| key(Path::new(&other.path)) == key(Path::new(&e.path)))
        .count()
        > 1
    {
        e.reason = Some("多个记录指向同一文件，保留以便核对".into());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::providers::AsmrOneProvider;
    use tempfile::{tempdir, TempDir};
    struct Fixture {
        _dir: TempDir,
        storage: StorageService,
        root: PathBuf,
    }
    fn fixture() -> Fixture {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let playback = PlaybackService::new(db.clone(), provider.clone());
        let downloads = DownloadService::new(db.clone(), provider);
        let root = dir.path().join("downloads");
        downloads.set_download_root(root.to_str().unwrap()).unwrap();
        db.with_conn(|c| {
            c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','测试作品','now','now');
                INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','音轨','1','now');
                INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');
                INSERT INTO media_sources(id,version_id,source_kind,format,created_at,updated_at) VALUES('remote','v','online_provider','mp3','now','now');")?;
            Ok(())
        }).unwrap();
        Fixture {
            _dir: dir,
            storage: StorageService::new(db, downloads, playback),
            root,
        }
    }
    fn cache(f: &Fixture, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = f.storage.db.cache_dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn offline(f: &Fixture) -> PathBuf {
        let path = f.root.join("offline.mp3");
        fs::write(&path, b"downloaded audio").unwrap();
        f.storage.db.with_conn(|c| {
            c.execute("INSERT INTO media_sources(id,version_id,source_kind,relative_path,absolute_path,format,created_at,updated_at) VALUES('local','v','local_downloaded','offline.mp3',?1,'mp3','now','now')", [path.to_string_lossy().as_ref()])?;
            Ok(())
        }).unwrap();
        path
    }
    fn part(f: &Fixture, status: &str) -> PathBuf {
        let path = f.root.join("offline.mp3.part");
        fs::write(&path, b"partial").unwrap();
        f.storage.db.with_conn(|c| {
            c.execute("INSERT INTO downloads(id,work_id,version_id,source_id,status,temp_relative_path,target_relative_path,downloaded_bytes,etag,created_at,updated_at) VALUES('d','w','v','remote',?1,'offline.mp3.part','offline.mp3',7,'old','now','now')", [status])?;
            Ok(())
        }).unwrap();
        path
    }
    #[test]
    fn inventory_counts_four_categories_and_preview_does_not_mutate() {
        let f = fixture();
        let p = cache(&f, "stream.cache", b"1234");
        cache(&f, "thumbnails/thumb_test.jpg", b"12345");
        offline(&f);
        part(&f, "paused");
        let overview = f.storage.overview().unwrap();
        assert_eq!(overview.entries.len(), 4);
        assert!(
            overview.entries.iter().all(|e| e.reason.is_none()),
            "{:?}",
            overview
                .entries
                .iter()
                .map(|e| &e.reason)
                .collect::<Vec<_>>()
        );
        let preview = f.storage.preview(Category::Playback, vec![]).unwrap();
        assert_eq!(preview.reclaimable_bytes, 4);
        assert!(p.exists());
        assert!(f.storage.preview(Category::Offline, vec![]).is_err());
    }
    #[test]
    fn cleanup_is_single_use_and_retains_imports_referenced_assets_and_offline() {
        let f = fixture();
        let stream = cache(&f, "stream.cache", b"1234");
        let audio = offline(&f);
        let cover = cache(&f, "covers/embedded_test.jpg", b"cover");
        let original = cache(&f, "import.cache", b"original");
        f.storage.db.with_conn(|c| {
            c.execute("UPDATE works SET cover_ref=?1",[cover.to_string_lossy().as_ref()])?;
            c.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES('import','v','local_imported',?1,'wav','now','now')",[original.to_string_lossy().as_ref()])?;
            Ok(())
        }).unwrap();
        let preview = f.storage.preview(Category::Playback, vec![]).unwrap();
        let result = f.storage.execute(&preview.token).unwrap();
        assert_eq!(result.deleted_count, 1, "{:?}", result.skipped);
        assert_eq!(result.freed_bytes, 4);
        assert!(!stream.exists());
        assert!(audio.exists() && original.exists() && cover.exists());
        assert!(f.storage.execute(&preview.token).is_err());
        assert_eq!(
            f.storage
                .preview(Category::Metadata, vec![])
                .unwrap()
                .reclaimable_bytes,
            0
        );
    }
    #[test]
    fn modified_locked_and_new_files_are_not_deleted() {
        let f = fixture();
        let changed = cache(&f, "changed.cache", b"old");
        let locked = cache(&f, "locked.cache", b"locked");
        let preview = f.storage.preview(Category::Playback, vec![]).unwrap();
        fs::write(&changed, b"new longer content").unwrap();
        let _open = fs::File::open(&locked).unwrap();
        let new = cache(&f, "new.cache", b"new");
        let result = f.storage.execute(&preview.token).unwrap();
        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.skipped.len(), 2);
        assert!(changed.exists() && locked.exists() && new.exists());
    }
    #[test]
    fn incomplete_cleanup_resets_resume_state_but_keeps_task_and_original() {
        let f = fixture();
        let path = part(&f, "paused");
        let original = f.root.join("original.wav");
        fs::write(&original, b"user").unwrap();
        let preview = f.storage.preview(Category::Incomplete, vec![]).unwrap();
        let result = f.storage.execute(&preview.token).unwrap();
        assert_eq!(result.deleted_count, 1, "{:?}", result.skipped);
        assert!(!path.exists());
        assert!(original.exists());
        f.storage
            .db
            .with_conn(|c| {
                let row: (String, i64, Option<String>) = c.query_row(
                    "SELECT status,downloaded_bytes,etag FROM downloads WHERE id='d'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                assert_eq!(row, ("paused".into(), 0, None));
                Ok(())
            })
            .unwrap();
    }
    #[test]
    fn resumed_download_and_imported_root_after_preview_are_protected() {
        let f = fixture();
        let path = part(&f, "paused");
        let preview = f.storage.preview(Category::Incomplete, vec![]).unwrap();
        f.storage
            .db
            .with_conn(|c| {
                c.execute("UPDATE downloads SET status='queued'", [])?;
                Ok(())
            })
            .unwrap();
        assert_eq!(f.storage.execute(&preview.token).unwrap().deleted_count, 0);
        assert!(path.exists());
        let path = cache(&f, "stream.cache", b"cache");
        let preview = f.storage.preview(Category::Playback, vec![]).unwrap();
        f.storage.db.with_conn(|c| { c.execute("INSERT INTO library_roots(id,absolute_path,display_name,created_at) VALUES('r',?1,'导入目录','now')",[f.storage.db.cache_dir.to_string_lossy().as_ref()])?; Ok(()) }).unwrap();
        assert_eq!(f.storage.execute(&preview.token).unwrap().deleted_count, 0);
        assert!(path.exists());
    }
    #[test]
    fn offline_cleanup_preserves_version_progress_and_marks_source_missing() {
        let f = fixture();
        let path = offline(&f);
        f.storage
            .db
            .save_playback_position("v", 1000, false, true)
            .unwrap();
        let bookmark = f.storage.db.add_bookmark("v", 1000, "keep").unwrap();
        let preview = f
            .storage
            .preview(Category::Offline, vec!["offline:local".into()])
            .unwrap();
        let result = f.storage.execute(&preview.token).unwrap();
        assert_eq!(result.deleted_count, 1, "{:?}", result.skipped);
        assert!(!path.exists());
        f.storage
            .db
            .with_conn(|c| {
                assert_eq!(
                    c.query_row(
                        "SELECT availability FROM media_sources WHERE id='local'",
                        [],
                        |r| r.get::<_, String>(0)
                    )?,
                    "missing"
                );
                assert_eq!(
                    c.query_row(
                        "SELECT position_ms FROM playback_positions WHERE version_id='v'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )?,
                    1000
                );
                assert_eq!(
                    c.query_row(
                        "SELECT COUNT(*) FROM bookmarks WHERE id=?1",
                        [bookmark.id],
                        |r| r.get::<_, i64>(0)
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();
    }
    #[test]
    fn queue_and_changed_download_root_invalidate_offline_preview() {
        let f = fixture();
        let path = offline(&f);
        let preview = f
            .storage
            .preview(Category::Offline, vec!["offline:local".into()])
            .unwrap();
        f.storage.db.with_conn(|c| { c.execute("INSERT INTO queue_items(id,sort_order,version_id,created_at) VALUES('q',0,'v','now')",[])?; Ok(()) }).unwrap();
        assert_eq!(f.storage.execute(&preview.token).unwrap().deleted_count, 0);
        assert!(path.exists());
        f.storage
            .db
            .with_conn(|c| {
                c.execute("DELETE FROM queue_items", [])?;
                Ok(())
            })
            .unwrap();
        let preview = f
            .storage
            .preview(Category::Offline, vec!["offline:local".into()])
            .unwrap();
        f.storage
            .downloads
            .set_download_root(f._dir.path().join("new-downloads").to_str().unwrap())
            .unwrap();
        assert_eq!(f.storage.execute(&preview.token).unwrap().deleted_count, 0);
        assert!(path.exists());
    }
    #[test]
    fn hardlinks_and_expired_tokens_fail_closed() {
        let f = fixture();
        let original = cache(&f, "original.cache", b"user");
        let link = cache(&f, "link.cache", b"replace");
        fs::remove_file(&link).unwrap();
        fs::hard_link(&original, &link).unwrap();
        let preview = f.storage.preview(Category::Playback, vec![]).unwrap();
        assert_eq!(preview.reclaimable_bytes, 0);
        f.storage
            .previews
            .lock()
            .get_mut(&preview.token)
            .unwrap()
            .created = Instant::now() - Duration::from_secs(301);
        assert!(f.storage.execute(&preview.token).is_err());
        assert!(original.exists() && link.exists());
        assert!(safe_relative(&f.root, "../original.wav").is_none());
        assert!(safe_relative(&f.root, "C:\\original.wav").is_none());
    }
    #[test]
    fn playback_prefetch_and_worker_guards_keep_owned_files() {
        let f = fixture();
        offline(&f);
        part(&f, "paused");
        cache(&f, "stream.cache", b"cache");
        let c = f.storage.catalog().unwrap();
        let mut e = c
            .entries
            .iter()
            .find(|e| e.category == Category::Offline)
            .unwrap()
            .clone();
        protect(
            &mut e,
            &c,
            true,
            &HashSet::from(["v".into()]),
            &HashSet::new(),
        );
        assert!(e.reason.unwrap().contains("播放会话"));
        let mut e = c
            .entries
            .iter()
            .find(|e| e.category == Category::Incomplete)
            .unwrap()
            .clone();
        protect(
            &mut e,
            &c,
            false,
            &HashSet::new(),
            &HashSet::from(["d".into()]),
        );
        assert!(e.reason.unwrap().contains("工作线程"));
    }
    #[test]
    fn database_failure_leaves_download_on_disk_and_available() {
        let f = fixture();
        let path = offline(&f);
        let preview = f
            .storage
            .preview(Category::Offline, vec!["offline:local".into()])
            .unwrap();
        f.storage.db.with_conn(|c| { c.execute_batch("CREATE TRIGGER refuse_cleanup BEFORE UPDATE ON media_sources BEGIN SELECT RAISE(ABORT, 'test database failure'); END;")?; Ok(()) }).unwrap();
        let result = f.storage.execute(&preview.token).unwrap();
        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(path.exists());
        let handle = LockedFile::open(&path, true).unwrap();
        handle.delete().unwrap();
        handle.retain().unwrap();
        drop(handle);
        assert!(
            path.exists(),
            "Canceling delete-on-close must retain a file if commit fails"
        );
    }
    #[test]
    fn directory_junctions_cannot_escape_download_root() {
        let f = fixture();
        let outside = f._dir.path().join("originals");
        fs::create_dir(&outside).unwrap();
        let original = outside.join("offline.mp3");
        fs::write(&original, b"original").unwrap();
        let link = f.root.join("junction");
        let result = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let aliased = link.join("offline.mp3");
        assert!(LockedFile::open(&aliased, true).is_err());
        assert!(original.exists());
        fs::remove_dir(link).unwrap();
        assert!(original.exists());
    }
}
