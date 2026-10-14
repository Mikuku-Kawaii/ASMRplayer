use crate::db::Database;
use crate::domain::*;
use crate::providers::{AsmrOneProvider, MediaPurpose, ProviderMediaRef};
use chrono::Utc;
use futures_util::StreamExt;
use parking_lot::Mutex;
use reqwest::{header, StatusCode};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
pub mod scheduling;
#[cfg(test)]
mod scheduling_tests;
use scheduling::{DownloadPolicy, RateBudget};
use uuid::Uuid;

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// T502/R12: Sanitizes a single path segment against Windows illegal characters, reserved device names, and traversal.
pub fn sanitize_windows_segment(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();

    s = s.trim_matches(|c: char| c.is_whitespace() || c == '.').to_string();
    if s.is_empty() || s == "." || s == ".." {
        s = "_unnamed".to_string();
    }

    let stem = s.split('.').next().unwrap_or(&s).to_ascii_uppercase();
    if WINDOWS_RESERVED_NAMES.contains(&stem.as_str()) {
        s = format!("_{}", s);
    }

    if s.chars().count() > 64 {
        let ext = Path::new(&s)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e))
            .unwrap_or_default();
        let prefix: String = s.chars().take(54).collect();
        s = format!("{}{}", prefix, ext);
    }
    s
}

/// T502/R12: Builds a safe relative path under the download root and verifies no traversal or junction escape.
pub fn build_safe_target_relative_path(
    work_title: &str,
    rj_code: Option<&str>,
    remote_relative_path: &str,
    source_id: &str,
) -> String {
    let folder_prefix = match rj_code {
        Some(rj) if !rj.trim().is_empty() => {
            format!("{} {}", sanitize_windows_segment(rj), sanitize_windows_segment(work_title))
        }
        _ => sanitize_windows_segment(work_title),
    };
    let folder_seg = sanitize_windows_segment(&folder_prefix);

    let mut clean_parts = vec![folder_seg];
    for comp in remote_relative_path.split(&['/', '\\'][..]) {
        let trimmed = comp.trim();
        if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
            continue;
        }
        clean_parts.push(sanitize_windows_segment(trimmed));
    }

    if clean_parts.len() == 1 {
        clean_parts.push(format!("track_{}.mp3", &source_id[..source_id.len().min(8)]));
    }

    clean_parts.join("/")
}

/// T502/R12: Verifies that `download_root` and all parent directories of `rel_path` are real directories
/// (not symlinks/junctions) and reside strictly inside `download_root`.
pub fn ensure_safe_parent_dir(download_root: &Path, rel_path: &str) -> AppResult<PathBuf> {
    if !download_root.exists() {
        fs::create_dir_all(download_root).map_err(|e| {
            AppError::new(
                ErrorCode::DiskOffline,
                format!("无法创建或访问下载根目录 {}: {}", download_root.display(), e),
            )
        })?;
    }
    let root_meta = fs::symlink_metadata(download_root)?;
    if root_meta.file_type().is_symlink() {
        return Err(AppError::new(
            ErrorCode::PathSecurityViolation,
            "下载根目录不允许为符号链接或目录联接",
        ));
    }

    let mut cur = download_root.to_path_buf();
    let parts: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err(AppError::new(ErrorCode::PathSecurityViolation, "无效的目标相对路径"));
    }

    for dir_part in &parts[..parts.len() - 1] {
        if *dir_part == "." || *dir_part == ".." || dir_part.contains('\\') || dir_part.contains(':') {
            return Err(AppError::new(
                ErrorCode::PathSecurityViolation,
                format!("检测到越界路径段: {}", dir_part),
            ));
        }
        cur.push(dir_part);
        if cur.exists() {
            let meta = fs::symlink_metadata(&cur)?;
            if meta.file_type().is_symlink() {
                return Err(AppError::new(
                    ErrorCode::PathSecurityViolation,
                    format!("目标路径包含符号链接或目录联接，拒绝写入: {}", cur.display()),
                ));
            }
        } else {
            fs::create_dir(&cur)?;
        }
    }

    let file_name = parts[parts.len() - 1];
    if file_name == "." || file_name == ".." || file_name.contains('\\') || file_name.contains(':') {
        return Err(AppError::new(
            ErrorCode::PathSecurityViolation,
            "非法的文件名",
        ));
    }

    Ok(cur.join(file_name))
}

/// Parses HTTP `Content-Range: bytes start-end/total` header.
pub fn parse_content_range(header_val: &str) -> Option<(u64, u64, Option<u64>)> {
    let s = header_val.trim();
    let rest = s.strip_prefix("bytes ")?;
    let (range_part, total_part) = rest.split_once('/')?;
    let total = if total_part == "*" {
        None
    } else {
        Some(total_part.parse::<u64>().ok()?)
    };
    let (start_s, end_s) = range_part.split_once('-')?;
    let start = start_s.parse::<u64>().ok()?;
    let end = end_s.parse::<u64>().ok()?;
    if start > end {
        return None;
    }
    Some((start, end, total))
}

/// T506: Verifies that the downloaded `.part` file has the expected byte length, is not an HTML error page,
/// and computes its SHA-256 digest.
pub fn verify_downloaded_part_file(
    part_path: &Path,
    expected_bytes: Option<i64>,
    remote_sha256: Option<&str>,
) -> AppResult<(i64, String, String)> {
    let mut file = OpenOptions::new().read(true).open(part_path)?;
    let actual_len = file.metadata()?.len() as i64;
    if actual_len <= 0 {
        return Err(AppError::new(
            ErrorCode::MediaDecodeFailed,
            "下载文件长度为 0，校验失败",
        ));
    }
    if let Some(exp) = expected_bytes {
        if exp > 0 && actual_len != exp {
            return Err(AppError::new(
                ErrorCode::MediaDecodeFailed,
                format!("文件字节数不匹配：期望 {} 字节，实际 {} 字节", exp, actual_len),
            ));
        }
    }

    let mut head_buf = [0u8; 512];
    let n = file.read(&mut head_buf)?;
    let head_ascii = String::from_utf8_lossy(&head_buf[..n]).to_ascii_lowercase();
    let trimmed_head = head_ascii.trim_start();
    if trimmed_head.starts_with("<!doctype html")
        || trimmed_head.starts_with("<html")
        || trimmed_head.starts_with("<head")
        || trimmed_head.starts_with("{\"error\"")
    {
        return Err(AppError::new(
            ErrorCode::MediaDecodeFailed,
            "下载内容为 HTML/JSON 错误响应而非有效媒体文件",
        ));
    }

    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buf)?;
        if count == 0 {
            break;
        }
        hasher.update(&buf[..count]);
    }
    let calc_sha = hex::encode(hasher.finalize());

    let integrity_mode = if let Some(rem) = remote_sha256.filter(|s| !s.trim().is_empty()) {
        if !calc_sha.eq_ignore_ascii_case(rem.trim()) {
            return Err(AppError::new(
                ErrorCode::MediaDecodeFailed,
                format!("远端 SHA-256 校验不一致：期望 {}，实际 {}", rem, calc_sha),
            ));
        }
        "remote_sha256".to_string()
    } else {
        "limited_size_and_header".to_string()
    };

    Ok((actual_len, calc_sha, integrity_mode))
}

#[cfg(target_os = "windows")]
fn get_available_disk_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut u64,
            lp_total_number_of_bytes: *mut u64,
            lp_total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_avail: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_avail,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok != 0 {
        Some(free_avail)
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
fn get_available_disk_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// T1005: Pre-download available disk space guard (requires `required_bytes + 16 MiB` safety margin).
pub fn check_sufficient_disk_space(dir: &Path, required_bytes: u64) -> AppResult<()> {
    if let Some(avail) = get_available_disk_bytes(dir) {
        let needed = required_bytes.saturating_add(16 * 1024 * 1024);
        if avail < needed {
            return Err(AppError::new(
                ErrorCode::DiskFull,
                format!(
                    "目标磁盘剩余空间不足（需至少 {:.1} MB，当前可用 {:.1} MB）",
                    (needed as f64) / (1024.0 * 1024.0),
                    (avail as f64) / (1024.0 * 1024.0)
                ),
            )
            .with_hint("请清理磁盘空间或在设置中更换离线下载根目录后重试"));
        }
    }
    Ok(())
}

#[derive(Clone)]
pub struct DownloadService {
    db: Database,
    provider: AsmrOneProvider,
    operation: Arc<Mutex<()>>,
    policy: Arc<Mutex<Option<DownloadPolicy>>>,
    rate: Arc<Mutex<RateBudget>>,
    cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    speeds: Arc<Mutex<HashMap<String, i64>>>,
    app_handle: Arc<Mutex<Option<AppHandle>>>,
}

impl DownloadService {
    /// Keeps worker registration/removal serialized with storage cleanup.
    pub(crate) fn with_storage_workers<T>(&self, f: impl FnOnce(&std::collections::HashSet<String>) -> T) -> T {
        let workers = self.cancel_flags.lock();
        f(&workers.keys().cloned().collect())
    }

    pub fn new(db: Database, provider: AsmrOneProvider) -> Self {
        let policy = scheduling::load_policy(&db).ok();
        Self {
            db,
            provider,
            operation: Arc::new(Mutex::new(())),
            policy: Arc::new(Mutex::new(policy)),
            rate: Arc::new(Mutex::new(RateBudget::default())),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            speeds: Arc::new(Mutex::new(HashMap::new())),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn attach_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock() = Some(handle);
    }

    pub fn get_download_root(&self) -> PathBuf {
        if let Ok(Some(p)) = self.db.get_setting("download_dir") {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        self.db.data_dir.join("downloads")
    }

    pub fn set_download_root(&self, new_dir: &str) -> AppResult<()> {
        let _operation=self.operation.lock();
        let p = PathBuf::from(new_dir.trim());
        let current=self.get_download_root();
        if p==current || p.canonicalize().ok().zip(current.canonicalize().ok()).is_some_and(|(a,b)|a==b){return Ok(())}
        if !self.cancel_flags.lock().is_empty(){return Err(AppError::new(ErrorCode::InvalidInput,"请先暂停下载并等待任务停止，再更换下载目录"));}
        fs::create_dir_all(&p)?;
        let meta = fs::symlink_metadata(&p)?;
        if meta.file_type().is_symlink() {
            return Err(AppError::new(
                ErrorCode::PathSecurityViolation,
                "下载目录不能是符号链接或目录联接",
            ));
        }
        let clean = p
            .canonicalize()
            .unwrap_or(p)
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        self.db.set_setting("download_dir", &clean)?;
        Ok(())
    }

    pub fn emit_tasks_updated(&self) {
        if let Ok(tasks) = self.list_tasks() {
            if let Some(ref handle) = *self.app_handle.lock() {
                let _ = handle.emit("downloads:updated", tasks);
            }
        }
    }

    /// T507/T508/R13: Reconciles database download tasks and on-disk `.part` / formal files at startup.
    pub fn reconcile_on_startup(&self) -> AppResult<()> {
        let download_root = self.get_download_root();
        let now_str = Utc::now().to_rfc3339();

        let pending_rows: Vec<(String, String, String, String, String, String, Option<i64>, i64)> =
            self.db.with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, version_id, source_id, status, temp_relative_path, target_relative_path, total_bytes, final_renamed
                     FROM downloads WHERE status NOT IN ('completed', 'canceled')",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })?;

        for (task_id, version_id, source_id, status, temp_rel, target_rel, total_bytes, final_renamed) in
            pending_rows
        {
            let target_full = download_root.join(target_rel.replace('/', "\\"));
            let temp_full = download_root.join(temp_rel.replace('/', "\\"));

            // R13: If formal file already exists and matches expected size (or final_renamed == 1),
            // complete registration in DB without re-downloading!
            if target_full.is_file() {
                if let Ok(meta) = fs::metadata(&target_full) {
                    let len = meta.len() as i64;
                    let matches_expected = match total_bytes {
                        Some(exp) if exp > 0 => len == exp,
                        _ => len > 0 && final_renamed == 1,
                    };
                    if matches_expected {
                        let _ = self.complete_task_in_db(
                            &task_id,
                            &version_id,
                            &source_id,
                            &target_rel,
                            &target_full,
                            len,
                            None,
                            "recovered_after_rename",
                        );
                        if temp_full.is_file() {
                            let _ = fs::remove_file(&temp_full);
                        }
                        continue;
                    }
                }
            }

            let part_bytes = if temp_full.is_file() {
                fs::metadata(&temp_full).map(|m| m.len() as i64).unwrap_or(0)
            } else {
                0
            };

            let next_status = if status == "failed" || status == "paused" { status.as_str() }
                else if self.get_policy().map(|p|p.resume_on_startup).unwrap_or(false) { "queued" }
                else { "paused" };

            self.db.with_conn(|conn| {
                conn.execute(
                    "UPDATE downloads SET status = ?1, downloaded_bytes = ?2, updated_at = ?3 WHERE id = ?4",
                    params![next_status, part_bytes, now_str, task_id],
                )?;
                Ok(())
            })?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_task_in_db(
        &self,
        task_id: &str,
        version_id: &str,
        source_id: &str,
        target_rel: &str,
        target_full: &Path,
        actual_bytes: i64,
        sha256: Option<&str>,
        integrity_mode: &str,
    ) -> AppResult<()> {
        let now_str = Utc::now().to_rfc3339();
        let abs_clean = target_full
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        let ext = target_full
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp3")
            .to_ascii_lowercase();

        self.db.with_conn(|conn| {
            let tx = conn.transaction()?;

            // Check if local_downloaded media_source already exists for this version_id & relative_path
            let existing_local: Option<String> = tx
                .query_row(
                    "SELECT id FROM media_sources WHERE version_id = ?1 AND source_kind = 'local_downloaded'",
                    params![version_id],
                    |r| r.get(0),
                )
                .optional()?;

            if let Some(ms_id) = existing_local {
                tx.execute(
                    "UPDATE media_sources
                     SET relative_path = ?1, absolute_path = ?2, format = ?3, byte_size = ?4,
                         content_sha256 = ?5, availability = 'available', updated_at = ?6
                     WHERE id = ?7",
                    params![target_rel, abs_clean, ext, actual_bytes, sha256, now_str, ms_id],
                )?;
            } else {
                let (provider, source_work_id, remote_file_id): (
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ) = tx
                    .query_row(
                        "SELECT provider, source_work_id, remote_file_id FROM media_sources WHERE id = ?1",
                        params![source_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?
                    .unwrap_or((None, None, None));

                tx.execute(
                    "INSERT INTO media_sources (id, version_id, source_kind, provider, source_work_id, remote_file_id, relative_path, absolute_path, format, byte_size, content_sha256, availability, created_at, updated_at)
                     VALUES (?1, ?2, 'local_downloaded', ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, 'available', ?10, ?11)",
                    params![
                        Uuid::new_v4().to_string(),
                        version_id,
                        provider,
                        source_work_id,
                        target_rel,
                        abs_clean,
                        ext,
                        actual_bytes,
                        sha256,
                        now_str,
                        now_str
                    ],
                )?;
                let _ = remote_file_id;
            }

            tx.execute(
                "UPDATE downloads
                 SET status = 'completed', downloaded_bytes = ?1, total_bytes = ?1,
                     content_sha256 = COALESCE(?2, content_sha256), integrity_mode = ?3,
                     final_renamed = 1, error_code = NULL, error_message = NULL, updated_at = ?4
                 WHERE id = ?5",
                params![actual_bytes, sha256, integrity_mode, now_str, task_id],
            )?;

            tx.commit()?;
            Ok(())
        })
    }

    pub fn list_tasks(&self) -> AppResult<Vec<DownloadTaskDto>> {
        let speeds_map = self.speeds.lock().clone();
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT d.id, d.work_id, w.title, d.version_id, t.title, d.source_id,
                        d.status, d.target_relative_path, d.downloaded_bytes, d.total_bytes,
                        d.integrity_mode, d.retry_count, d.error_code, d.error_message, d.updated_at, d.priority, d.next_retry_at
                 FROM downloads d
                 JOIN works w ON w.id = d.work_id
                 JOIN track_versions tv ON tv.id = d.version_id
                 JOIN tracks t ON t.id = tv.track_id
                 ORDER BY d.priority DESC, d.created_at ASC, d.rowid ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                let id: String = r.get(0)?;
                let speed = speeds_map.get(&id).copied().unwrap_or(0);
                Ok(DownloadTaskDto {
                    id,
                    work_id: r.get(1)?,
                    work_title: r.get(2)?,
                    version_id: r.get(3)?,
                    track_title: r.get(4)?,
                    source_id: r.get(5)?,
                    status: r.get(6)?,
                    target_relative_path: r.get(7)?,
                    downloaded_bytes: r.get(8)?,
                    total_bytes: r.get(9)?,
                    speed_bps: speed,
                    integrity_mode: r.get(10)?,
                    retry_count: r.get(11)?,
                    error_code: r.get(12)?,
                    error_message: r.get(13)?,
                    updated_at: r.get(14)?,
                    priority: r.get(15)?,
                    next_retry_at: r.get(16)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    /// T501: Previews downloadable online sources for a work without starting downloads.
    pub fn preview_download(
        &self,
        work_id: &str,
        source_ids: Option<Vec<String>>,
    ) -> AppResult<DownloadPreviewDto> {
        let detail = self.db.get_work_detail(work_id)?;
        let download_root = self
            .get_download_root()
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();

        let mut items = Vec::new();
        let mut total_bytes = 0i64;
        let mut used_rel_paths_lower = std::collections::HashSet::new();

        for track in &detail.tracks {
            for ver in &track.versions {
                let already_downloaded = ver
                    .sources
                    .iter()
                    .any(|s| (s.source_kind == "local_downloaded" || s.source_kind == "local_imported") && s.availability == "available");

                for src in &ver.sources {
                    if src.source_kind != "online_provider" {
                        continue;
                    }
                    if let Some(ref filter_ids) = source_ids {
                        if !filter_ids.contains(&src.id) {
                            continue;
                        }
                    }

                    let remote_rel = src
                        .remote_relative_path
                        .as_deref()
                        .unwrap_or(&track.title);
                    let mut target_rel = build_safe_target_relative_path(
                        &detail.title,
                        detail.rj_code.as_deref(),
                        remote_rel,
                        &src.id,
                    );

                    // Case-insensitive collision resolution on Windows (T502/R12)
                    if !used_rel_paths_lower.insert(target_rel.to_lowercase()) {
                        let ext = Path::new(&target_rel)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("mp3");
                        let without_ext = target_rel
                            .strip_suffix(&format!(".{}", ext))
                            .unwrap_or(&target_rel);
                        target_rel = format!(
                            "{}_{}.{}",
                            without_ext,
                            &src.id[..src.id.len().min(6)],
                            ext
                        );
                        used_rel_paths_lower.insert(target_rel.to_lowercase());
                    }

                    let already_queued = self.db.with_conn(|conn| {
                        let q: Option<i64> = conn
                            .query_row(
                                "SELECT 1 FROM downloads WHERE source_id = ?1 AND status NOT IN ('canceled', 'failed')",
                                params![src.id],
                                |r| r.get(0),
                            )
                            .optional()?;
                        Ok(q.is_some())
                    })?;

                    if let Some(b) = src.byte_size {
                        total_bytes += b;
                    }

                    items.push(DownloadPreviewItemDto {
                        source_id: src.id.clone(),
                        version_id: ver.id.clone(),
                        track_title: track.title.clone(),
                        format: src.format.clone(),
                        byte_size: src.byte_size,
                        target_relative_path: target_rel,
                        already_downloaded,
                        already_queued,
                    });
                }
            }
        }

        Ok(DownloadPreviewDto {
            download_root,
            work_id: detail.id,
            work_title: detail.title,
            items,
            total_bytes,
        })
    }

    /// T501: Enqueues download tasks deduplicated by `source_id` and launches workers.
    pub fn enqueue_downloads(
        &self,
        work_id: &str,
        source_ids: Option<Vec<String>>,
    ) -> AppResult<Vec<DownloadTaskDto>> {
        let preview = self.preview_download(work_id, source_ids)?;
        let now_str = Utc::now().to_rfc3339();
        let mut to_spawn = Vec::new();
        let operation = self.operation.lock();
        let active_workers: std::collections::HashSet<_> = self.cancel_flags.lock().keys().cloned().collect();

        self.db.with_conn(|conn| {
            let tx = conn.transaction()?;
            for item in &preview.items {
                if item.already_downloaded {
                    continue;
                }
                let existing: Option<(String, String)> = tx
                    .query_row(
                        "SELECT id, status FROM downloads WHERE source_id = ?1 ORDER BY created_at DESC LIMIT 1",
                        params![item.source_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;

                if let Some((tid, status)) = existing {
                    if status == "canceled" && active_workers.contains(&tid) {
                        return Err(AppError::new(ErrorCode::InvalidInput,"任务正在取消，请稍后重新加入"));
                    }
                    if matches!(status.as_str(), "paused" | "failed" | "canceled") {
                        tx.execute(
                            "UPDATE downloads SET status = 'queued', next_retry_at = NULL, error_code = NULL, error_message = NULL, updated_at = ?1 WHERE id = ?2",
                            params![now_str, tid],
                        )?;
                        to_spawn.push(tid);
                    }
                    continue;
                }

                // Also check if another active task has the same case-insensitive target_relative_path
                let mut target_rel = item.target_relative_path.clone();
                let collision: Option<i64> = tx
                    .query_row(
                        "SELECT 1 FROM downloads WHERE LOWER(target_relative_path) = LOWER(?1) AND source_id != ?2",
                        params![target_rel, item.source_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if collision.is_some() {
                    let ext = Path::new(&target_rel)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("mp3")
                        .to_string();
                    let stem = target_rel
                        .strip_suffix(&format!(".{}", ext))
                        .unwrap_or(&target_rel);
                    target_rel = format!(
                        "{}_{}.{}",
                        stem,
                        &item.source_id[..item.source_id.len().min(6)],
                        ext
                    );
                }

                let temp_rel = format!("{}.part", target_rel);
                let tid = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO downloads (id, work_id, version_id, source_id, status, temp_relative_path, target_relative_path, downloaded_bytes, total_bytes, integrity_mode, retry_count, final_renamed, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, 0, ?7, 'limited_size_and_header', 0, 0, ?8, ?9)",
                    params![
                        tid,
                        work_id,
                        item.version_id,
                        item.source_id,
                        temp_rel,
                        target_rel,
                        item.byte_size,
                        now_str,
                        now_str
                    ],
                )?;
                to_spawn.push(tid);
            }
            tx.commit()?;
            Ok(())
        })?;

        drop(operation);
        for tid in to_spawn {
            self.spawn_worker(tid);
        }
        self.emit_tasks_updated();
        self.list_tasks()
    }
}

impl DownloadService {
    pub fn pause_task(&self, task_id: &str) -> AppResult<Vec<DownloadTaskDto>> {
        {
            let _operation=self.operation.lock();
            self.db.with_conn(|c|{c.execute("UPDATE downloads SET status='paused',next_retry_at=NULL,updated_at=?1 WHERE id=?2 AND status IN ('queued','resolving','downloading','verifying','failed')",params![Utc::now().to_rfc3339(),task_id])?;Ok(())})?;
            if let Some(flag)=self.cancel_flags.lock().get(task_id){flag.store(true,Ordering::SeqCst);}
        }
        self.speeds.lock().remove(task_id);self.emit_tasks_updated();self.list_tasks()
    }
    pub fn resume_task(&self, task_id: &str) -> AppResult<Vec<DownloadTaskDto>> {
        {
            let _operation=self.operation.lock();
            self.db.with_conn(|c|{c.execute("UPDATE downloads SET status='queued',next_retry_at=NULL,error_code=NULL,error_message=NULL,updated_at=?1 WHERE id=?2 AND status IN ('paused','failed')",params![Utc::now().to_rfc3339(),task_id])?;Ok(())})?;
        }
        self.dispatch()?;self.emit_tasks_updated();self.list_tasks()
    }
    /// Cancellation retains the worker registration until its file handles close.
    pub fn cancel_task(&self, task_id: &str) -> AppResult<Vec<DownloadTaskDto>> {
        {
            let _operation=self.operation.lock();
            self.db.with_conn(|c|{c.execute("UPDATE downloads SET status='canceled',next_retry_at=NULL,downloaded_bytes=0,updated_at=?1 WHERE id=?2 AND status NOT IN ('completed','canceled')",params![Utc::now().to_rfc3339(),task_id])?;Ok(())})?;
            let workers=self.cancel_flags.lock();
            if let Some(flag)=workers.get(task_id){flag.store(true,Ordering::SeqCst);}else{self.cleanup_canceled_part(task_id)?;}
        }
        self.speeds.lock().remove(task_id);self.emit_tasks_updated();self.list_tasks()
    }
    pub fn cancel_all_tasks_for_work(&self, work_id: &str) -> AppResult<()> {
        let ids:Vec<String>=self.db.with_conn(|c|{
            let mut stmt=c.prepare("SELECT id FROM downloads WHERE work_id=?1 AND status NOT IN ('completed','canceled')")?;
            let rows=stmt.query_map([work_id],|r|r.get(0))?;Ok(rows.collect::<Result<Vec<_>,_>>()?)
        })?;
        for id in ids {self.cancel_task(&id)?;}
        Ok(())
    }

    /// Safely deletes ONLY software-downloaded offline copies (`local_downloaded`) and `.part` files
    /// inside `download_root` for `work_id`, never touching any `local_imported` user files.
    pub fn delete_downloaded_files_for_work(&self, work_id: &str) -> AppResult<usize> {
        self.cancel_all_tasks_for_work(work_id)?;

        let download_root = self.get_download_root();
        let (dl_paths, ms_paths): (Vec<(String, String)>, Vec<Option<String>>) =
            self.db.with_conn(|conn| {
                let mut s1 = conn.prepare(
                    "SELECT target_relative_path, temp_relative_path FROM downloads WHERE work_id = ?1",
                )?;
                let r1 = s1
                    .query_map(params![work_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;

                let mut s2 = conn.prepare(
                    "SELECT ms.relative_path
                     FROM media_sources ms
                     JOIN track_versions tv ON tv.id = ms.version_id
                     JOIN tracks t ON t.id = tv.track_id
                     WHERE t.work_id = ?1 AND ms.source_kind = 'local_downloaded'",
                )?;
                let r2 = s2
                    .query_map(params![work_id], |r| r.get(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((r1, r2))
            })?;

        let mut rel_candidates = Vec::new();
        for (target_rel, temp_rel) in dl_paths {
            rel_candidates.push(target_rel);
            rel_candidates.push(temp_rel);
        }
        for rel in ms_paths.into_iter().flatten() {
            rel_candidates.push(rel);
        }

        let mut deleted_count = 0usize;
        let mut parent_dirs = Vec::new();

        for rel in rel_candidates {
            let clean_rel = rel.trim().trim_start_matches(&['/', '\\'][..]);
            if clean_rel.is_empty() || clean_rel.contains("..") || clean_rel.contains(':') {
                continue;
            }
            let full_path = download_root.join(clean_rel.replace('/', "\\"));
            if let Ok(meta) = fs::symlink_metadata(&full_path) {
                if meta.is_file() && !meta.file_type().is_symlink() && fs::remove_file(&full_path).is_ok() {
                    deleted_count += 1;
                    if let Some(parent) = full_path.parent() {
                        parent_dirs.push(parent.to_path_buf());
                    }
                }
            }
        }

        // Clean up empty work directories inside download_root
        for mut dir in parent_dirs {
            while dir != download_root && dir.starts_with(&download_root) {
                if fs::remove_dir(&dir).is_err() {
                    break;
                }
                if let Some(p) = dir.parent() {
                    dir = p.to_path_buf();
                } else {
                    break;
                }
            }
        }

        self.db.with_conn(|conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM downloads WHERE work_id = ?1", params![work_id])?;
            tx.execute(
                "DELETE FROM media_sources
                 WHERE source_kind = 'local_downloaded'
                   AND version_id IN (
                       SELECT tv.id FROM track_versions tv
                       JOIN tracks t ON t.id = tv.track_id
                       WHERE t.work_id = ?1
                   )",
                params![work_id],
            )?;
            tx.commit()?;
            Ok(())
        })?;

        self.emit_tasks_updated();
        Ok(deleted_count)
    }

    pub fn spawn_worker(&self, _task_id: String) { let _=self.dispatch(); }

    #[cfg(test)]
    pub async fn run_single_task(&self, task_id: &str) -> AppResult<()> {
        let flag=Arc::new(AtomicBool::new(false));
        {
            let _operation=self.operation.lock();let mut workers=self.cancel_flags.lock();
            if workers.contains_key(task_id){return Ok(())}
            workers.insert(task_id.into(),flag.clone());
        }
        self.run_registered(task_id,flag).await
    }
    async fn run_registered(&self, task_id: &str, flag: Arc<AtomicBool>) -> AppResult<()> {
        let res=tokio::select! {
            biased;
            _=Self::cancelled(flag.clone()) => Ok(()),
            result=self.execute_task_inner(task_id,flag.clone()) => result,
        };
        {
            let _operation=self.operation.lock();
            if let Err(ref err)=res {
                if !flag.load(Ordering::SeqCst) {
                    let network=matches!(err.code,ErrorCode::NetworkUnavailable|ErrorCode::RateLimited);
                    let persisted=self.db.with_conn(|c|{
                        let retries:i64=c.query_row("SELECT retry_count FROM downloads WHERE id=?1",[task_id],|r|r.get(0)).optional()?.unwrap_or(0);
                        let delay=30_i64.saturating_mul(1_i64<<retries.clamp(0,4)).min(300);
                        let due=network.then(||(Utc::now()+chrono::Duration::seconds(delay)).to_rfc3339());
                        c.execute("UPDATE downloads SET status='failed',retry_count=retry_count+1,error_code=?1,error_message=?2,next_retry_at=?3,updated_at=?4 WHERE id=?5 AND status NOT IN ('paused','canceled','completed')",params![format!("{:?}",err.code),err.message,due,Utc::now().to_rfc3339(),task_id])?;Ok(())
                    });
                    if let Err(e)=persisted {self.db.append_log("WARN",&format!("下载失败状态保存失败：{}",e.message));}
                }
            }
            // The transfer future has dropped its writer, so disk length is authoritative.
            let download_root=self.get_download_root();
            let _=self.db.with_conn(|c|{
                let rel:Option<String>=c.query_row("SELECT temp_relative_path FROM downloads WHERE id=?1 AND status NOT IN ('completed','canceled')",[task_id],|r|r.get(0)).optional()?;
                if let Some(rel)=rel {
                    if let Ok(path)=ensure_safe_parent_dir(&download_root,&rel){
                        let bytes=fs::metadata(path).map(|m|m.len() as i64).unwrap_or(0);
                        c.execute("UPDATE downloads SET downloaded_bytes=?1 WHERE id=?2",params![bytes,task_id])?;
                    }
                }
                Ok(())
            });
            let cleanup=self.cleanup_canceled_part(task_id);
            self.cancel_flags.lock().remove(task_id);
            self.speeds.lock().remove(task_id);
            if let Err(e)=cleanup {self.db.append_log("WARN",&format!("取消下载后的临时文件清理失败：{}",e.message));}
        }
        self.emit_tasks_updated();res
    }

    async fn execute_task_inner(
        &self,
        task_id: &str,
        cancel_flag: Arc<AtomicBool>,
    ) -> AppResult<()> {
        let row: Option<(
            String,
            String,
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
            Option<String>,
        )> = self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT version_id, source_id, status, temp_relative_path, target_relative_path,
                        total_bytes, etag, last_modified
                 FROM downloads WHERE id = ?1",
                params![task_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .optional()
            .map_err(AppError::from)
        })?;

        let (
            version_id,
            source_id,
            status,
            temp_rel,
            target_rel,
            saved_total_bytes,
            saved_etag,
            saved_last_mod,
        ) = match row {
            Some(r) => r,
            None => return Ok(()),
        };

        if status != "queued" {
            return Ok(());
        }

        {
            let _operation=self.operation.lock();
            if cancel_flag.load(Ordering::SeqCst) { return Ok(()) }
            let claimed=self.db.with_conn(|conn| Ok(conn.execute(
                "UPDATE downloads SET status='resolving',updated_at=?1 WHERE id=?2 AND status='queued'",
                params![Utc::now().to_rfc3339(),task_id])?))?;
            if claimed==0 {return Ok(())}
        }
        self.emit_tasks_updated();

        let (provider_name, source_work_id, remote_file_id, remote_rel, expected_sha256): (
            String,
            String,
            String,
            String,
            Option<String>,
        ) = self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT COALESCE(provider, 'asmr_one'), COALESCE(source_work_id, ''),
                        COALESCE(remote_file_id, ''), COALESCE(remote_relative_path, ''), content_sha256
                 FROM media_sources WHERE id = ?1",
                params![source_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到关联的媒体源"))
        })?;

        let resolved = if remote_file_id.starts_with("http://") || remote_file_id.starts_with("https://")
        {
            crate::providers::ResolvedMedia {
                url: remote_file_id.clone(),
                headers: Vec::new(),
                byte_size: saved_total_bytes,
                expires_at: None,
            }
        } else {
            let media_ref = ProviderMediaRef {
                provider: provider_name,
                source_work_id,
                remote_file_id,
                remote_relative_path: remote_rel,
            };
            self.provider
                .resolve_media(&media_ref, MediaPurpose::Download, false)
                .await?
        };

        if cancel_flag.load(Ordering::SeqCst) {
            return Ok(());
        }

        let download_root = self.get_download_root();
        let target_full = ensure_safe_parent_dir(&download_root, &target_rel)?;
        let temp_full = ensure_safe_parent_dir(&download_root, &temp_rel)?;

        let existing_part_len = if temp_full.is_file() {
            fs::metadata(&temp_full).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        // T505/R11: Only attempt Range resume if we have a validator (ETag/Last-Modified) or known total_bytes
        let has_validator = saved_etag.is_some() || saved_last_mod.is_some() || saved_total_bytes.is_some();
        let mut request_offset = if existing_part_len > 0 && has_validator {
            existing_part_len
        } else {
            0
        };

        let client = self.provider.build_download_client()?;

        let mut skip_to_verify = false;
        let mut final_expected_bytes = saved_total_bytes.or(resolved.byte_size);

        // Loop at most twice (if Range returns invalid 206 or 416 mismatch, fallback to full download from 0)
        for pass in 0..2 {
            let mut req = client.get(&resolved.url);
            for (k, v) in &resolved.headers {
                req = req.header(k, v);
            }
            if request_offset > 0 {
                req = req.header(header::RANGE, format!("bytes={}-", request_offset));
                // Send If-Range only when ETag is present (per T006 finding that CDN rejects HTTP-date If-Range)
                if let Some(ref et) = saved_etag {
                    if !et.trim().is_empty() {
                        req = req.header(header::IF_RANGE, et.trim());
                    }
                }
            }

            let resp = tokio::time::timeout(std::time::Duration::from_secs(30),req.send()).await
                .map_err(|_|AppError::new(ErrorCode::NetworkUnavailable,"下载响应超时，请检查网络连接"))?.map_err(|e| {
                AppError::new(
                    ErrorCode::NetworkUnavailable,
                    format!("下载连接失败: {}", e.without_url()),
                )
            })?;

            let status = resp.status();
            let resp_etag = resp
                .headers()
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let resp_last_mod = resp
                .headers()
                .get(header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                return Err(AppError::new(
                    ErrorCode::AuthRequired,
                    format!("下载链接需要认证或已过期 (HTTP {})", status.as_u16()),
                ));
            }
            if status == StatusCode::RANGE_NOT_SATISFIABLE {
                // T505/R11: 416 only enters completion verification if length and remote validator are confirmed
                let validator_confirmed = saved_etag.is_some() || saved_last_mod.is_some();
                let length_confirmed = saved_total_bytes
                    .map(|tot| tot > 0 && tot as u64 == request_offset)
                    .unwrap_or(false);
                if validator_confirmed && length_confirmed {
                    skip_to_verify = true;
                    break;
                } else if pass == 0 {
                    request_offset = 0;
                    continue;
                } else {
                    return Err(AppError::new(
                        ErrorCode::NetworkUnavailable,
                        "服务器返回 416 Range Not Satisfiable 且无法验证文件完整性",
                    ));
                }
            }

            if !status.is_success() {
                return Err(AppError::new(
                    if status.as_u16()==429 {ErrorCode::RateLimited} else if status.is_server_error() || status.as_u16()==408 {ErrorCode::NetworkUnavailable} else {ErrorCode::SourceNotFound},
                    format!("下载请求返回错误 HTTP {}", status.as_u16()),
                ));
            }

            let append_mode;
            if status == StatusCode::PARTIAL_CONTENT {
                // T505/R11: Verify Content-Range start == request_offset and validator consistency
                let cr_header = resp
                    .headers()
                    .get(header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let parsed_cr = parse_content_range(cr_header);

                let etag_mismatch = match (&saved_etag, &resp_etag) {
                    (Some(old), Some(new)) => old != new,
                    (Some(_), None) => true,
                    _ => false,
                };
                let last_mod_mismatch = match (&saved_last_mod, &resp_last_mod) {
                    (Some(old), Some(new)) => old != new,
                    (Some(_), None) => true,
                    _ => false,
                };

                if let Some((start, _end, total_opt)) = parsed_cr {
                    let total_mismatch = match (saved_total_bytes, total_opt) {
                        (Some(old_t), Some(new_t)) if old_t > 0 => (old_t as u64) != new_t,
                        _ => false,
                    };
                    if start == request_offset && (request_offset == 0 || (!etag_mismatch && !last_mod_mismatch && !total_mismatch))
                    {
                        // A fresh 206 starting at zero must replace a stale .part file.
                        append_mode = request_offset > 0;
                        if let Some(tot) = total_opt {
                            final_expected_bytes = Some(tot as i64);
                        }
                    } else if pass == 0 {
                        request_offset = 0;
                        continue;
                    } else {
                        return Err(AppError::new(
                            ErrorCode::SourceChanged,
                            "206 Content-Range 起点或远端校验器不匹配",
                        ));
                    }
                } else if pass == 0 {
                    request_offset = 0;
                    continue;
                } else {
                    return Err(AppError::new(
                        ErrorCode::ProviderSchemaChanged,
                        "服务器返回 206 但缺少有效的 Content-Range 响应头",
                    ));
                }
            } else {
                // Status 200 OK: MUST overwrite from 0, NEVER append (T505/R11)!
                append_mode = false;
                request_offset = 0;
                if let Some(cl) = resp.content_length() {
                    if cl > 0 {
                        final_expected_bytes = Some(cl as i64);
                    }
                }
            }

            // T1005: Pre-download available disk space check
            if let Some(expected_total) = final_expected_bytes {
                let remaining_needed = (expected_total - request_offset as i64).max(0) as u64;
                if remaining_needed > 0 {
                    if let Some(parent_dir) = temp_full.parent() {
                        check_sufficient_disk_space(parent_dir, remaining_needed)?;
                    }
                }
            }

            let operation=self.operation.lock();
            if cancel_flag.load(Ordering::SeqCst){return Ok(())}
            let file = if append_mode {
                OpenOptions::new().create(true).append(true).open(&temp_full)?
            } else {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&temp_full)?
            };
            // T1005: 256 KB buffered writer to coalesce small network chunks and prevent I/O amplification
            let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file);

            let mut current_bytes = request_offset as i64;
            let now_str = Utc::now().to_rfc3339();
            self.db.with_conn(|conn| {
                conn.execute(
                    "UPDATE downloads
                     SET status = 'downloading', downloaded_bytes = ?1, total_bytes = ?2,
                         etag = COALESCE(?3, etag), last_modified = COALESCE(?4, last_modified), updated_at = ?5
                     WHERE id = ?6",
                    params![
                        current_bytes,
                        final_expected_bytes,
                        resp_etag,
                        resp_last_mod,
                        now_str,
                        task_id
                    ],
                )?;
                Ok(())
            })?;
            drop(operation);
            self.emit_tasks_updated();

            let mut stream = resp.bytes_stream();
            let mut last_tick = Instant::now();
            let mut bytes_since_tick = 0i64;

            while let Some(chunk_res) = tokio::time::timeout(std::time::Duration::from_secs(30),stream.next()).await
                .map_err(|_|AppError::new(ErrorCode::NetworkUnavailable,"下载数据等待超时，断点已保留"))? {
                if cancel_flag.load(Ordering::SeqCst) {
                    let _ = writer.flush();
                    return Ok(());
                }
                let chunk = chunk_res.map_err(|e| {
                    AppError::new(ErrorCode::NetworkUnavailable, format!("下载流中断: {}", e.without_url()))
                })?;
                for piece in chunk.chunks(16 * 1024) {
                    self.wait_budget(piece.len(),&cancel_flag).await?;
                    if cancel_flag.load(Ordering::SeqCst){let _=writer.flush();return Ok(())}
                    writer.write_all(piece).map_err(|e| AppError::new(ErrorCode::DiskFull,format!("写入临时文件失败: {}",e)))?;
                    current_bytes += piece.len() as i64;
                    bytes_since_tick += piece.len() as i64;
                }

                let elapsed = last_tick.elapsed();
                if elapsed.as_millis() >= 250 {
                    let bps = ((bytes_since_tick as f64) / elapsed.as_secs_f64()).round() as i64;
                    self.speeds.lock().insert(task_id.to_string(), bps);
                    let _operation=self.operation.lock();
                    if cancel_flag.load(Ordering::SeqCst){return Ok(())}
                    let ts = Utc::now().to_rfc3339();
                    let _ = self.db.with_conn(|conn| {
                        conn.execute(
                            "UPDATE downloads SET downloaded_bytes = ?1, updated_at = ?2 WHERE id = ?3",
                            params![current_bytes, ts, task_id],
                        )?;
                        Ok(())
                    });
                    self.emit_tasks_updated();
                    last_tick = Instant::now();
                    bytes_since_tick = 0;
                }
            }
            writer
                .flush()
                .map_err(|e| AppError::new(ErrorCode::DiskFull, format!("刷新下载缓冲区失败: {}", e)))?;
            if let Ok(inner_file) = writer.into_inner() {
                let _ = inner_file.sync_all();
            }
            break;
        }

        if cancel_flag.load(Ordering::SeqCst) {
            return Ok(());
        }

        let _ = skip_to_verify;
        let operation=self.operation.lock();
        if cancel_flag.load(Ordering::SeqCst){return Ok(())}
        let now_str = Utc::now().to_rfc3339();
        self.db.with_conn(|conn| {
            conn.execute(
                "UPDATE downloads SET status = 'verifying', updated_at = ?1 WHERE id = ?2",
                params![now_str, task_id],
            )?;
            Ok(())
        })?;
        self.emit_tasks_updated();

        drop(operation);
        // T506: Verify length, non-HTML header, and SHA-256
        let (actual_len, calc_sha, integrity_mode) = verify_downloaded_part_file(
            &temp_full,
            final_expected_bytes,
            expected_sha256.as_deref(),
        )?;

        // Pause/cancel and final rename+registration are serialized.
        let _operation=self.operation.lock();
        if cancel_flag.load(Ordering::SeqCst){return Ok(())}
        // T507: Atomic rename on same filesystem, then DB transaction
        fs::rename(&temp_full, &target_full).map_err(|e| {
            AppError::new(
                ErrorCode::FileLocked,
                format!("原子重命名正式文件失败: {}", e),
            )
        })?;

        let _ = self.db.with_conn(|conn| {
            conn.execute(
                "UPDATE downloads SET final_renamed = 1 WHERE id = ?1",
                params![task_id],
            )?;
            Ok(())
        });

        self.complete_task_in_db(
            task_id,
            &version_id,
            &source_id,
            &target_rel,
            &target_full,
            actual_len,
            Some(&calc_sha),
            &integrity_mode,
        )?;
        self.emit_tasks_updated();
        Ok(())
    }
}

#[cfg(test)]
mod download_tests {
    use super::*;
    use tempfile::tempdir;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_safe_target_path_sanitizes_traversal_and_reserved_names_r12() {
        let rel = build_safe_target_relative_path(
            "RJ01705068: 聖女<テスト>",
            Some("RJ01705068"),
            "../../CON/NUL.mp3",
            "src123456",
        );
        assert!(!rel.contains(".."));
        assert!(!rel.contains('<'));
        assert!(!rel.contains(':'));
        let parts: Vec<&str> = rel.split('/').collect();
        assert_eq!(parts[1], "_CON");
        assert_eq!(parts[2], "_NUL.mp3");
    }

    #[tokio::test]
    async fn test_range_206_resume_and_200_full_overwrite_r11() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let dl_svc = DownloadService::new(db.clone(), provider);
        dl_svc
            .set_download_root(&dir.path().join("downloads").to_string_lossy())
            .unwrap();

        let payload = b"ID3_SYNTHETIC_AUDIO_PAYLOAD_0123456789_ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let expected_sha = hex::encode(hasher.finalize());

        // Endpoint 1: Supports Range 206 from offset 16
        let url_206 = format!("{}/audio206.mp3", server.uri());
        Mock::given(method("GET"))
            .and(path("/audio206.mp3"))
            .and(header_exists("Range"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header(
                        "Content-Range",
                        format!("bytes 16-{}/{}", payload.len() - 1, payload.len()).as_str(),
                    )
                    .insert_header("Last-Modified", "Sun, 11 Oct 2026 01:53:38 GMT")
                    .set_body_bytes(&payload[16..]),
            )
            .mount(&server)
            .await;

        // Endpoint 2: Ignores Range and returns 200 OK full body (must NOT append to existing .part!)
        let url_200 = format!("{}/audio200.mp3", server.uri());
        Mock::given(method("GET"))
            .and(path("/audio200.mp3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Last-Modified", "Sun, 11 Oct 2026 01:53:38 GMT")
                    .set_body_bytes(payload.as_slice()),
            )
            .mount(&server)
            .await;

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w1', 'DL Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t1', 'w1', 'Track 206', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t2', 'w1', 'Track 200', '002', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, created_at) VALUES ('v1', 't1', 'MP3', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, created_at) VALUES ('v2', 't2', 'MP3', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, provider, source_work_id, remote_file_id, remote_relative_path, format, byte_size, availability, created_at, updated_at)
                 VALUES ('ms206', 'v1', 'online_provider', 'asmr_one', '1', ?1, 't206.mp3', 'mp3', ?2, 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![url_206, payload.len() as i64],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, provider, source_work_id, remote_file_id, remote_relative_path, format, byte_size, availability, created_at, updated_at)
                 VALUES ('ms200', 'v2', 'online_provider', 'asmr_one', '1', ?1, 't200.mp3', 'mp3', ?2, 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![url_200, payload.len() as i64],
            )?;
            Ok(())
        })
        .unwrap();

        // Pre-populate 16-byte .part files for both tasks
        let dl_root = dl_svc.get_download_root();
        let part1 = ensure_safe_parent_dir(&dl_root, "DL Work/t206.mp3.part").unwrap();
        fs::write(&part1, &payload[..16]).unwrap();
        let part2 = ensure_safe_parent_dir(&dl_root, "DL Work/t200.mp3.part").unwrap();
        fs::write(&part2, &payload[..16]).unwrap();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO downloads (id, work_id, version_id, source_id, status, temp_relative_path, target_relative_path, downloaded_bytes, total_bytes, last_modified, created_at, updated_at)
                 VALUES ('task206', 'w1', 'v1', 'ms206', 'queued', 'DL Work/t206.mp3.part', 'DL Work/t206.mp3', 16, ?1, 'Sun, 11 Oct 2026 01:53:38 GMT', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![payload.len() as i64],
            )?;
            conn.execute(
                "INSERT INTO downloads (id, work_id, version_id, source_id, status, temp_relative_path, target_relative_path, downloaded_bytes, total_bytes, last_modified, created_at, updated_at)
                 VALUES ('task200', 'w1', 'v2', 'ms200', 'queued', 'DL Work/t200.mp3.part', 'DL Work/t200.mp3', 16, ?1, 'Sun, 11 Oct 2026 01:53:38 GMT', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![payload.len() as i64],
            )?;
            Ok(())
        })
        .unwrap();

        dl_svc.run_single_task("task206").await.unwrap();
        dl_svc.run_single_task("task200").await.unwrap();

        let tasks = dl_svc.list_tasks().unwrap();
        for t in &tasks {
            assert_eq!(t.status, "completed");
            assert_eq!(t.downloaded_bytes, payload.len() as i64);
        }

        let detail = db.get_work_detail("w1").unwrap();
        for tr in &detail.tracks {
            let local_src = tr.versions[0]
                .sources
                .iter()
                .find(|s| s.source_kind == "local_downloaded")
                .expect("Completed download must create local_downloaded source under same version_id");
            assert_eq!(local_src.content_sha256.as_deref(), Some(expected_sha.as_str()));
        }

        // R11: A discarded resume offset followed by a full 206 must truncate old bytes.
        server.reset().await;
        Mock::given(method("GET")).and(path("/audio200.mp3"))
            .respond_with(ResponseTemplate::new(206)
                .insert_header("Content-Range", format!("bytes 0-{}/{}", payload.len()-1, payload.len()).as_str())
                .set_body_bytes(payload.as_slice())).mount(&server).await;
        fs::write(&part2, b"stale partial data").unwrap();
        db.with_conn(|conn| {
            conn.execute("UPDATE downloads SET status='queued', total_bytes=NULL, last_modified=NULL, etag=NULL, final_renamed=0 WHERE id='task200'", [])?;
            Ok(())
        }).unwrap();
        fs::remove_file(dl_root.join("DL Work/t200.mp3")).unwrap();
        dl_svc.run_single_task("task200").await.unwrap();
        assert_eq!(fs::read(dl_root.join("DL Work/t200.mp3")).unwrap(), payload);
    }

    #[test]
    fn test_crash_after_rename_recovery_r13() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let dl_svc = DownloadService::new(db.clone(), provider);
        dl_svc
            .set_download_root(&dir.path().join("downloads").to_string_lossy())
            .unwrap();

        let content = b"ID3_FORMAL_FILE_RENAMED_BEFORE_CRASH";
        let target_full =
            ensure_safe_parent_dir(&dl_svc.get_download_root(), "CrashWork/track1.mp3").unwrap();
        fs::write(&target_full, content).unwrap();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w_crash', 'CrashWork', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t_crash', 'w_crash', 'Track 1', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, created_at) VALUES ('v_crash', 't_crash', 'MP3', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, provider, source_work_id, remote_file_id, format, byte_size, availability, created_at, updated_at)
                 VALUES ('ms_crash', 'v_crash', 'online_provider', 'asmr_one', '1', '1/1', 'mp3', ?1, 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![content.len() as i64],
            )?;
            conn.execute(
                "INSERT INTO downloads (id, work_id, version_id, source_id, status, temp_relative_path, target_relative_path, downloaded_bytes, total_bytes, final_renamed, created_at, updated_at)
                 VALUES ('task_crash', 'w_crash', 'v_crash', 'ms_crash', 'verifying', 'CrashWork/track1.mp3.part', 'CrashWork/track1.mp3', ?1, ?1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![content.len() as i64],
            )?;
            Ok(())
        })
        .unwrap();

        // Startup reconciliation must recover the renamed file and bind local_downloaded source to v_crash!
        dl_svc.reconcile_on_startup().unwrap();
        let tasks = dl_svc.list_tasks().unwrap();
        assert_eq!(tasks[0].status, "completed");

        let detail = db.get_work_detail("w_crash").unwrap();
        assert!(detail.tracks[0].versions[0]
            .sources
            .iter()
            .any(|s| s.source_kind == "local_downloaded" && s.availability == "available"));
    }

    #[test]
    fn test_delete_downloaded_files_preserves_local_imported_files() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let dl_svc = DownloadService::new(db.clone(), provider);
        dl_svc
            .set_download_root(&dir.path().join("downloads").to_string_lossy())
            .unwrap();

        let user_dir = dir.path().join("user_library");
        fs::create_dir_all(&user_dir).unwrap();
        let user_audio = user_dir.join("original_user_track.wav");
        fs::write(&user_audio, b"RIFF_USER_ORIGINAL_AUDIO").unwrap();

        let downloaded_audio =
            ensure_safe_parent_dir(&dl_svc.get_download_root(), "RJ01009999/track1.mp3").unwrap();
        fs::write(&downloaded_audio, b"ID3_DOWNLOADED_COPY").unwrap();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, rj_code, created_at, updated_at) VALUES ('w_del', 'DeleteTestWork', 'RJ01009999', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t_del', 'w_del', 'Track 1', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, created_at) VALUES ('v_del', 't_del', 'Default', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, relative_path, format, byte_size, availability, created_at, updated_at)
                 VALUES ('ms_user', 'v_del', 'local_imported', 'original_user_track.wav', 'wav', 24, 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, relative_path, format, byte_size, availability, created_at, updated_at)
                 VALUES ('ms_dl', 'v_del', 'local_downloaded', 'RJ01009999/track1.mp3', 'mp3', 19, 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let deleted_count = dl_svc.delete_downloaded_files_for_work("w_del").unwrap();
        assert_eq!(deleted_count, 1);
        assert!(
            !downloaded_audio.exists(),
            "Software-downloaded file in download_root should be removed"
        );
        assert!(
            user_audio.exists(),
            "User's local_imported file must NEVER be deleted"
        );

        db.delete_work("w_del").unwrap();
        assert!(
            user_audio.exists(),
            "Removing work from DB must NEVER delete user's local_imported file"
        );
    }
}
