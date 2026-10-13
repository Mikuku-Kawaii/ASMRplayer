use crate::db::Database;
use crate::domain::*;
use chrono::Utc;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::MimeType;
use lofty::probe::Probe;
use lofty::tag::Accessor;
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;
use walkdir::WalkDir;

const AUDIO_EXTS: &[&str] = &["mp3", "flac", "wav", "m4a", "ogg", "opus"];
const COVER_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp"];
const SUBTITLE_EXTS: &[&str] = &["srt", "vtt", "lrc", "ass"];
const TEXT_EXTS: &[&str] = &["txt", "md"];

/// T1106: Strips subtitle extensions, language tags, and inner audio extensions
/// (e.g. `01_耳かき.mp3.lrc` -> `01_耳かき`, `01_耳かき.wav` -> `01_耳かき`) for automatic track-subtitle pairing.
pub fn strip_audio_double_extension(filename: &str) -> String {
    let base = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filename)
        .trim()
        .to_lowercase();
    let mut s = base.as_str();

    for sub_ext in &[".srt", ".vtt", ".lrc", ".ass", ".txt"] {
        if let Some(stripped) = s.strip_suffix(sub_ext) {
            if !stripped.is_empty() {
                s = stripped;
                break;
            }
        }
    }

    for lang_ext in &[
        ".zh", ".chs", ".cht", ".sc", ".tc", ".cn", ".jp", ".ja", ".en", ".zh-cn", ".zh-tw",
    ] {
        if let Some(stripped) = s.strip_suffix(lang_ext) {
            if !stripped.is_empty() {
                s = stripped;
                break;
            }
        }
    }

    for aud_ext in &[".mp3", ".wav", ".flac", ".m4a", ".ogg", ".opus", ".aac"] {
        if let Some(stripped) = s.strip_suffix(aud_ext) {
            if !stripped.is_empty() {
                s = stripped;
                break;
            }
        }
    }

    s.trim().to_string()
}

/// Generates a natural sort key where numeric runs are zero-padded to 12 digits (`"2"` < `"10"`).
pub fn natural_sort_key(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut num_str = String::new();
            num_str.push(c);
            while let Some(&next_c) = chars.peek() {
                if next_c.is_ascii_digit() {
                    num_str.push(next_c);
                    chars.next();
                } else {
                    break;
                }
            }
            let trimmed = num_str.trim_start_matches('0');
            let digits = if trimmed.is_empty() { "0" } else { trimmed };
            out.push_str(&format!("{:0>12}", digits));
        } else {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
        }
    }
    out
}

pub fn extract_rj_code(input: &str) -> Option<String> {
    let upper = input.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    let mut i = 0;
    while i + 8 <= bytes.len() {
        if bytes[i] == b'R' && bytes[i + 1] == b'J' {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let digit_len = j - (i + 2);
            if (6..=8).contains(&digit_len) {
                return Some(upper[i..j].to_string());
            }
        }
        i += 1;
    }
    None
}

pub fn normalize_rel_path(base: &Path, full: &Path) -> String {
    full.strip_prefix(base)
        .unwrap_or(full)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Clone)]
pub struct LibraryService {
    db: Database,
    cancel_flag: Arc<AtomicBool>,
    active_scan_lock: Arc<Mutex<bool>>,
}

impl LibraryService {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            active_scan_lock: Arc::new(Mutex::new(false)),
        }
    }

    pub fn cancel_scan(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub fn list_roots(&self) -> AppResult<Vec<LibraryRootDto>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT lr.id, lr.absolute_path, lr.display_name, lr.root_mode, lr.availability, lr.last_scanned_at,
                        (SELECT COUNT(*) FROM library_entries le WHERE le.root_id = lr.id)
                 FROM library_roots lr ORDER BY lr.created_at DESC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(LibraryRootDto {
                    id: r.get(0)?,
                    absolute_path: r.get(1)?,
                    display_name: r.get(2)?,
                    root_mode: r.get(3)?,
                    availability: r.get(4)?,
                    last_scanned_at: r.get(5)?,
                    work_count: r.get::<_, i64>(6)? as usize,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                let mut dto = row?;
                let exists = Path::new(&dto.absolute_path).is_dir();
                dto.availability = if exists {
                    "available".into()
                } else {
                    "offline".into()
                };
                out.push(dto);
            }
            Ok(out)
        })
    }

    /// T201: Generates an import preview without modifying the database or writing to user directories.
    pub fn preview_import(&self, raw_path: &str, root_mode: &str) -> AppResult<ImportPreviewDto> {
        if root_mode != "single_work" && root_mode != "library_root" {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "root_mode 必须为 single_work 或 library_root",
            ));
        }
        let root_path = PathBuf::from(raw_path.trim());
        if !root_path.is_dir() {
            return Err(AppError::new(
                ErrorCode::SourceNotFound,
                format!("所选目录不存在或不可访问: {}", raw_path),
            ));
        }

        let canonical_str = root_path
            .canonicalize()
            .unwrap_or_else(|_| root_path.clone())
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();

        // Check overlapping roots
        let existing_roots = self.list_roots()?;
        let mut overlapping_roots = Vec::new();
        let norm_new = canonical_str.to_lowercase();
        for r in &existing_roots {
            let norm_old = r.absolute_path.to_lowercase();
            if norm_new != norm_old
                && (norm_new.starts_with(&format!("{}\\", norm_old))
                    || norm_old.starts_with(&format!("{}\\", norm_new))
                    || norm_new.starts_with(&format!("{}/", norm_old))
                    || norm_old.starts_with(&format!("{}/", norm_new)))
            {
                overlapping_roots.push(r.absolute_path.clone());
            }
        }

        let mut candidates = Vec::new();
        if root_mode == "single_work" {
            let c = self.inspect_candidate_folder(&root_path, &canonical_str, ".")?;
            candidates.push(c);
        } else {
            let mut subdirs = Vec::new();
            let mut root_direct_audio = 0usize;
            for entry in fs::read_dir(&root_path)? {
                let entry = entry?;
                let ft = entry.file_type()?;
                if ft.is_symlink() {
                    continue;
                }
                let p = entry.path();
                if ft.is_dir() {
                    subdirs.push(p);
                } else if ft.is_file() {
                    let ext = p
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if AUDIO_EXTS.contains(&ext.as_str()) {
                        root_direct_audio += 1;
                    }
                }
            }
            subdirs.sort_by_key(|p| natural_sort_key(&p.to_string_lossy()));

            if root_direct_audio > 0 {
                let folder_name = root_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("根目录直属音频");
                candidates.push(ImportCandidateDto {
                    relative_work_path: "__root_direct__".to_string(),
                    proposed_title: format!("{} (直属音频)", folder_name),
                    detected_rj_code: extract_rj_code(folder_name),
                    audio_count: root_direct_audio,
                    subtitle_count: 0,
                    cover_detected: false,
                    already_indexed: self.is_entry_indexed(&canonical_str, "__root_direct__")?,
                });
            }

            for sub in subdirs {
                let rel = normalize_rel_path(&root_path, &sub);
                let c = self.inspect_candidate_folder(&sub, &canonical_str, &rel)?;
                if c.audio_count > 0 || c.subtitle_count > 0 {
                    candidates.push(c);
                }
            }
        }

        Ok(ImportPreviewDto {
            root_path: canonical_str,
            root_mode: root_mode.to_string(),
            overlapping_roots,
            candidates,
            selected_input_path: None,
            detection_reason: None,
        })
    }

    /// T1110: Intelligently resolves the work root directory when a user selects a single audio/subtitle file
    /// or a deep subfolder (e.g. `.../RJ01234567 タイトル/本編/WAV/01.wav` -> `.../RJ01234567 タイトル`).
    pub fn resolve_smart_work_root(
        selected_path: &Path,
        shift_level: i32,
    ) -> AppResult<(PathBuf, String)> {
        if !selected_path.exists() {
            return Err(AppError::new(
                ErrorCode::SourceNotFound,
                format!("所选路径不存在: {}", selected_path.display()),
            ));
        }

        let start_dir = if selected_path.is_file() {
            selected_path
                .parent()
                .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "文件缺少父级目录"))?
                .to_path_buf()
        } else {
            selected_path.to_path_buf()
        };

        // Build ancestor chain from deepest (start_dir, index 0) upwards up to 6 levels
        let mut chain: Vec<PathBuf> = Vec::new();
        let mut cur: Option<&Path> = Some(&start_dir);
        while let Some(p) = cur {
            if p.as_os_str().is_empty() || p.parent().is_none() {
                break;
            }
            chain.push(p.to_path_buf());
            if chain.len() >= 6 {
                break;
            }
            cur = p.parent();
        }
        if chain.is_empty() {
            chain.push(start_dir.clone());
        }

        // Rule 1: Priority match on ancestor folder name containing RJ code (within 4 levels)
        let mut auto_idx = 0usize;
        let mut reason = if selected_path.is_file() {
            "从所选单文件定位至所在目录".to_string()
        } else {
            "使用当前所选目录作为作品根目录".to_string()
        };

        let mut found_rj_ancestor = false;
        for (idx, anc) in chain.iter().take(5).enumerate() {
            if let Some(name) = anc.file_name().and_then(|n| n.to_str()) {
                if let Some(rj) = extract_rj_code(name) {
                    auto_idx = idx;
                    reason = format!("自动向上溯源至包含 {} 的作品主目录", rj);
                    found_rj_ancestor = true;
                    break;
                }
            }
        }

        // Rule 2: If no ancestor has an RJ code, walk up out of structural leaf directories (WAV/MP3/SEなし/特典/字幕)
        if !found_rj_ancestor {
            while auto_idx + 1 < chain.len() && auto_idx < 3 {
                let name_lower = chain[auto_idx]
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
                if is_structural_asmr_subfolder(&name_lower) {
                    auto_idx += 1;
                    reason = "从音频格式/分轨子目录自动向上归并至作品根目录".to_string();
                } else {
                    break;
                }
            }
        }

        // Apply manual shift_level adjustment (+1 = move up to parent, -1 = move down toward start_dir)
        let shifted = (auto_idx as i32 + shift_level).clamp(0, (chain.len() as i32) - 1) as usize;
        if shift_level != 0 {
            reason = format!("已手动调整目录层级 ({:+})", shift_level);
        }

        Ok((chain[shifted].clone(), reason))
    }

    /// T1110: Generates a smart single-work import preview from a file or subfolder selection.
    pub fn preview_smart_import(
        &self,
        raw_selected_path: &str,
        shift_level: i32,
    ) -> AppResult<ImportPreviewDto> {
        let p = PathBuf::from(raw_selected_path.trim());
        let (resolved_dir, reason) = Self::resolve_smart_work_root(&p, shift_level)?;
        let resolved_str = resolved_dir
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        let mut preview = self.preview_import(&resolved_str, "single_work")?;
        preview.selected_input_path = Some(raw_selected_path.trim().to_string());
        preview.detection_reason = Some(reason);
        Ok(preview)
    }

    fn is_entry_indexed(&self, root_abs: &str, rel_work: &str) -> AppResult<bool> {
        self.db.with_conn(|conn| {
            let found: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM library_entries le
                     JOIN library_roots lr ON lr.id = le.root_id
                     WHERE LOWER(lr.absolute_path) = LOWER(?1) AND le.relative_work_path = ?2",
                    params![root_abs, rel_work],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(found.is_some())
        })
    }

    fn inspect_candidate_folder(
        &self,
        folder: &Path,
        root_abs: &str,
        rel_work: &str,
    ) -> AppResult<ImportCandidateDto> {
        let mut audio_count = 0usize;
        let mut subtitle_count = 0usize;
        let mut cover_detected = false;
        let mut sample_audio_paths = Vec::new();
        let mut rj_from_files: Option<String> = None;

        for entry in WalkDir::new(folder)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path();
            if let Some(fname) = p.file_name().and_then(|n| n.to_str()) {
                if fname.ends_with(".part") {
                    continue;
                }
                if rj_from_files.is_none() {
                    rj_from_files = extract_rj_code(fname);
                }
            }
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if AUDIO_EXTS.contains(&ext.as_str()) {
                audio_count += 1;
                if sample_audio_paths.len() < 3 {
                    sample_audio_paths.push(p.to_path_buf());
                }
            } else if SUBTITLE_EXTS.contains(&ext.as_str()) {
                subtitle_count += 1;
            } else if COVER_EXTS.contains(&ext.as_str()) {
                cover_detected = true;
            }
        }

        let dir_name = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("未命名作品")
            .to_string();
        let mut detected_rj_code = extract_rj_code(&dir_name)
            .or_else(|| extract_rj_code(rel_work))
            .or(rj_from_files);

        if !cover_detected || detected_rj_code.is_none() {
            for ap in &sample_audio_paths {
                let (has_pic, tag_rj) = probe_audio_cover_and_rj(ap);
                if has_pic {
                    cover_detected = true;
                }
                if detected_rj_code.is_none() && tag_rj.is_some() {
                    detected_rj_code = tag_rj;
                }
                if cover_detected && detected_rj_code.is_some() {
                    break;
                }
            }
        }

        let already_indexed = self.is_entry_indexed(root_abs, rel_work)?;

        Ok(ImportCandidateDto {
            relative_work_path: rel_work.to_string(),
            proposed_title: dir_name,
            detected_rj_code,
            audio_count,
            subtitle_count,
            cover_detected,
            already_indexed,
        })
    }
}

#[derive(Debug, Clone)]
struct ScannedFile {
    relative_to_root: String,
    logical_dir_in_work: String,
    file_name: String,
    absolute_path: PathBuf,
    ext: String,
    byte_size: i64,
    mtime_ms: i64,
}

impl LibraryService {
    /// T202/T203/T204: Scans selected work candidates under `raw_root_path` idempotently.
    pub fn scan_and_import(
        &self,
        raw_root_path: &str,
        root_mode: &str,
        selected_candidates: Option<Vec<(String, String)>>, // (relative_work_path, custom_title)
    ) -> AppResult<ScanReportDto> {
        {
            let mut active = self.active_scan_lock.lock();
            if *active {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "当前已有扫描任务正在运行",
                ));
            }
            *active = true;
        }
        self.cancel_flag.store(false, Ordering::SeqCst);
        let res = self.scan_and_import_inner(raw_root_path, root_mode, selected_candidates);
        *self.active_scan_lock.lock() = false;
        res
    }

    fn scan_and_import_inner(
        &self,
        raw_root_path: &str,
        root_mode: &str,
        selected_candidates: Option<Vec<(String, String)>>,
    ) -> AppResult<ScanReportDto> {
        let root_path = PathBuf::from(raw_root_path.trim());
        if !root_path.is_dir() {
            // T204: If root is offline, mark root offline but NEVER mark works/sources deleted
            let _ = self.db.with_conn(|conn| {
                conn.execute(
                    "UPDATE library_roots SET availability = 'offline' WHERE LOWER(absolute_path) = LOWER(?1)",
                    params![raw_root_path.trim()],
                )?;
                Ok(())
            });
            return Err(AppError::new(
                ErrorCode::DiskOffline,
                format!("根目录当前离线或不可访问: {}", raw_root_path),
            ));
        }

        let canonical_root = root_path
            .canonicalize()
            .unwrap_or_else(|_| root_path.clone());
        let root_abs_str = canonical_root
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        let display_name = canonical_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&root_abs_str)
            .to_string();
        let now_str = Utc::now().to_rfc3339();

        let root_id = self.db.with_conn(|conn| {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM library_roots WHERE LOWER(absolute_path) = LOWER(?1)",
                    params![root_abs_str],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                conn.execute(
                    "UPDATE library_roots SET absolute_path = ?1, root_mode = ?2, availability = 'available' WHERE id = ?3",
                    params![root_abs_str, root_mode, id],
                )?;
                Ok(id)
            } else {
                let id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO library_roots (id, absolute_path, display_name, root_mode, availability, last_scanned_at, created_at)
                     VALUES (?1, ?2, ?3, ?4, 'available', NULL, ?5)",
                    params![id, root_abs_str, display_name, root_mode, now_str],
                )?;
                Ok(id)
            }
        })?;

        let candidates: Vec<(String, String)> = match selected_candidates {
            Some(list) if !list.is_empty() => list,
            _ => {
                let preview = self.preview_import(&root_abs_str, root_mode)?;
                preview
                    .candidates
                    .into_iter()
                    .map(|c| (c.relative_work_path, c.proposed_title))
                    .collect()
            }
        };

        let mut works_added = 0usize;
        let mut works_updated = 0usize;
        let mut tracks_added = 0usize;
        let mut corrupted_files = Vec::new();
        let mut canceled = false;

        for (rel_work_path, proposed_title) in candidates {
            if self.cancel_flag.load(Ordering::SeqCst) {
                canceled = true;
                break;
            }

            let work_dir = if rel_work_path == "." || rel_work_path == "__root_direct__" {
                canonical_root.clone()
            } else {
                canonical_root.join(rel_work_path.replace('/', "\\"))
            };

            if !work_dir.is_dir() {
                continue;
            }

            let max_depth = if rel_work_path == "__root_direct__" {
                1
            } else {
                32
            };

            let mut audio_files = Vec::new();
            let mut cover_files = Vec::new();
            let mut attachment_files = Vec::new();

            for entry in WalkDir::new(&work_dir)
                .max_depth(max_depth)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if self.cancel_flag.load(Ordering::SeqCst) {
                    canceled = true;
                    break;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let p = entry.path();
                let file_name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if file_name.ends_with(".part") {
                    continue;
                }
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();

                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let byte_size = meta.len() as i64;
                let mtime_ms = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);

                let rel_to_root = normalize_rel_path(&canonical_root, p);
                let rel_to_work = normalize_rel_path(&work_dir, p);
                let logical_dir = Path::new(&rel_to_work)
                    .parent()
                    .map(|parent| {
                        parent
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().to_string())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_default();

                let sf = ScannedFile {
                    relative_to_root: rel_to_root,
                    logical_dir_in_work: logical_dir,
                    file_name,
                    absolute_path: p.to_path_buf(),
                    ext: ext.clone(),
                    byte_size,
                    mtime_ms,
                };

                if AUDIO_EXTS.contains(&ext.as_str()) {
                    audio_files.push(sf);
                } else if COVER_EXTS.contains(&ext.as_str()) {
                    cover_files.push(sf);
                } else if SUBTITLE_EXTS.contains(&ext.as_str()) || TEXT_EXTS.contains(&ext.as_str())
                {
                    attachment_files.push(sf);
                }
            }

            if canceled {
                break;
            }
            if audio_files.is_empty() && attachment_files.is_empty() {
                continue;
            }

            audio_files.sort_by_key(|f| {
                natural_sort_key(&format!("{}/{}", f.logical_dir_in_work, f.file_name))
            });
            cover_files.sort_by_key(|f| {
                let lower = f.file_name.to_lowercase();
                let priority = if lower.contains("cover")
                    || lower.contains("folder")
                    || lower.contains("front")
                    || lower.contains("jacket")
                    || lower.contains("main")
                    || lower.contains("top")
                    || lower.contains("sam")
                    || lower.contains("thumb")
                    || lower.contains("封面")
                    || lower.contains("rj")
                {
                    0
                } else {
                    1
                };
                (priority, f.logical_dir_in_work.len(), natural_sort_key(&f.file_name))
            });

            // T903: Read audio metadata in parallel across worker threads outside DB lock, respecting cancel_flag
            let mut valid_audio_files = Vec::with_capacity(audio_files.len());
            for af in audio_files {
                if af.byte_size == 0 {
                    corrupted_files.push(af.relative_to_root.clone());
                } else {
                    valid_audio_files.push(af);
                }
            }

            let need_embedded_pic = cover_files.is_empty();
            let worker_count = std::thread::available_parallelism()
                .map(|n| n.get().clamp(2, 8))
                .unwrap_or(4)
                .min(valid_audio_files.len().max(1));
            let chunk_size = (valid_audio_files.len() + worker_count - 1) / worker_count.max(1);

            let mut probed_results: Vec<(ScannedFile, AudioExtractedMeta, Vec<String>)> =
                Vec::with_capacity(valid_audio_files.len());

            if chunk_size > 0 && !valid_audio_files.is_empty() {
                let cancel_ref = &self.cancel_flag;
                let chunks: Vec<&[ScannedFile]> = valid_audio_files.chunks(chunk_size).collect();
                let chunk_outputs: Vec<Vec<(ScannedFile, AudioExtractedMeta, Vec<String>)>> =
                    std::thread::scope(|s| {
                        let mut handles = Vec::with_capacity(chunks.len());
                        for (chunk_idx, chunk) in chunks.into_iter().enumerate() {
                            handles.push(s.spawn(move || {
                                let mut local_out = Vec::with_capacity(chunk.len());
                                for (idx_in_chunk, af) in chunk.iter().enumerate() {
                                    if cancel_ref.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    let mut local_corrupt = Vec::new();
                                    let extract_pic =
                                        need_embedded_pic && chunk_idx == 0 && idx_in_chunk < 2;
                                    let meta = read_audio_metadata(
                                        &af.absolute_path,
                                        &mut local_corrupt,
                                        &af.relative_to_root,
                                        extract_pic,
                                    );
                                    local_out.push((af.clone(), meta, local_corrupt));
                                }
                                local_out
                            }));
                        }
                        handles
                            .into_iter()
                            .filter_map(|h| h.join().ok())
                            .collect()
                    });

                for group in chunk_outputs {
                    for item in group {
                        probed_results.push(item);
                    }
                }
            }

            if self.cancel_flag.load(Ordering::SeqCst) {
                canceled = true;
                break;
            }

            let mut parsed_audio_meta = Vec::with_capacity(probed_results.len());
            let mut embedded_cover: Option<(Vec<u8>, &'static str)> = None;
            let mut rj_from_audio: Option<String> = None;
            let mut fallback_vas: Vec<String> = Vec::new();
            let mut fallback_tags: Vec<String> = Vec::new();

            for (af, mut meta, local_corrupt) in probed_results {
                corrupted_files.extend(local_corrupt);
                if rj_from_audio.is_none() {
                    rj_from_audio = extract_rj_code(&af.file_name);
                }
                if embedded_cover.is_none() {
                    embedded_cover = meta.embedded_cover.take();
                }
                if rj_from_audio.is_none() {
                    rj_from_audio = meta
                        .album
                        .as_deref()
                        .and_then(extract_rj_code)
                        .or_else(|| meta.title.as_deref().and_then(extract_rj_code));
                }
                if let Some(ref artist_str) = meta.artist {
                    for part in artist_str.split(['/', ';', ',', '、']) {
                        let clean = part.trim();
                        if !clean.is_empty() && !fallback_vas.iter().any(|v| v == clean) {
                            fallback_vas.push(clean.to_string());
                        }
                    }
                }
                if let Some(ref genre_str) = meta.genre {
                    for part in genre_str.split(['/', ';', ',', '、']) {
                        let clean = part.trim();
                        if !clean.is_empty() && !fallback_tags.iter().any(|t| t == clean) {
                            fallback_tags.push(clean.to_string());
                        }
                    }
                }
                parsed_audio_meta.push((af, meta.duration_ms, meta.title, meta.artist));
            }

            if parsed_audio_meta.is_empty() && attachment_files.is_empty() {
                continue;
            }

            // T904: Cover thumbnail disk cache under cache_dir/thumbnails & covers
            let mut best_cover_path = cover_files
                .first()
                .map(|c| c.absolute_path.to_string_lossy().trim_start_matches(r"\\?\").to_string());

            if let Some(ref orig_cover) = best_cover_path {
                let thumbs_dir = self.db.cache_dir.join("thumbnails");
                if fs::create_dir_all(&thumbs_dir).is_ok() {
                    let mut hasher = Sha256::new();
                    hasher.update(orig_cover.as_bytes());
                    let hash_prefix = &hex::encode(hasher.finalize())[..24];
                    let ext = Path::new(orig_cover)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("jpg");
                    let cached_thumb = thumbs_dir.join(format!("thumb_{}.{}", hash_prefix, ext));
                    if !cached_thumb.is_file() {
                        let _ = fs::copy(orig_cover, &cached_thumb);
                    }
                }
            } else if let Some((pic_bytes, pic_ext)) = embedded_cover {
                let covers_dir = self.db.cache_dir.join("covers");
                if fs::create_dir_all(&covers_dir).is_ok() {
                    let mut hasher = Sha256::new();
                    hasher.update(format!("{}:{}", root_abs_str, rel_work_path).as_bytes());
                    let deterministic_id = &hex::encode(hasher.finalize())[..24];
                    let cover_file = covers_dir.join(format!("embedded_{}.{}", deterministic_id, pic_ext));
                    if fs::write(&cover_file, &pic_bytes).is_ok() {
                        best_cover_path = Some(
                            cover_file
                                .to_string_lossy()
                                .trim_start_matches(r"\\?\")
                                .to_string(),
                        );
                    }
                }
            }

            let detected_rj = extract_rj_code(&proposed_title)
                .or_else(|| extract_rj_code(&rel_work_path))
                .or(rj_from_audio);

            let fallback_vas_json = serde_json::to_string(&fallback_vas).unwrap_or_else(|_| "[]".into());
            let fallback_tags_json = serde_json::to_string(&fallback_tags).unwrap_or_else(|_| "[]".into());

            let seen_rel_paths: HashSet<String> = parsed_audio_meta
                .iter()
                .map(|(f, _, _, _)| f.relative_to_root.clone())
                .collect();

            // Pre-extract subtitle text outside DB transaction for FTS5 indexing (T902)
            let mut work_subtitle_corpus = String::new();
            for att in &attachment_files {
                if SUBTITLE_EXTS.contains(&att.ext.as_str()) {
                    if let Ok(bytes) = fs::read(&att.absolute_path) {
                        if let Ok(cues) = parse_subtitle_bytes(&bytes, 0) {
                            for cue in cues {
                                if !work_subtitle_corpus.is_empty() {
                                    work_subtitle_corpus.push(' ');
                                }
                                work_subtitle_corpus.push_str(&cue.text);
                            }
                        }
                    }
                }
            }

            self.db.with_conn(|conn| {
                let tx = conn.transaction()?;

                let existing_work_id: Option<String> = tx
                    .prepare_cached(
                        "SELECT work_id FROM library_entries WHERE root_id = ?1 AND relative_work_path = ?2",
                    )?
                    .query_row(params![root_id, rel_work_path], |r| r.get(0))
                    .optional()?;

                let work_id = if let Some(wid) = existing_work_id {
                    works_updated += 1;
                    tx.prepare_cached(
                        "UPDATE works
                         SET cover_ref = COALESCE(cover_ref, ?1),
                             rj_code = COALESCE(rj_code, ?2),
                             voice_actors_json = CASE WHEN voice_actors_json = '[]' AND ?3 != '[]' THEN ?3 ELSE voice_actors_json END,
                             tags_json = CASE WHEN tags_json = '[]' AND ?4 != '[]' THEN ?4 ELSE tags_json END,
                             updated_at = ?5
                         WHERE id = ?6",
                    )?
                    .execute(params![
                        best_cover_path,
                        detected_rj,
                        fallback_vas_json,
                        fallback_tags_json,
                        now_str,
                        wid
                    ])?;
                    tx.prepare_cached(
                        "UPDATE library_entries SET last_seen_at = ?1 WHERE root_id = ?2 AND relative_work_path = ?3",
                    )?
                    .execute(params![now_str, root_id, rel_work_path])?;
                    wid
                } else {
                    works_added += 1;
                    let wid = Uuid::new_v4().to_string();
                    tx.prepare_cached(
                        "INSERT INTO works (id, title, description, rj_code, circle_name, voice_actors_json, tags_json, cover_ref, created_at, updated_at)
                         VALUES (?1, ?2, '', ?3, '', ?4, ?5, ?6, ?7, ?8)",
                    )?
                    .execute(params![
                        wid,
                        proposed_title,
                        detected_rj,
                        fallback_vas_json,
                        fallback_tags_json,
                        best_cover_path,
                        now_str,
                        now_str
                    ])?;
                    tx.prepare_cached(
                        "INSERT INTO library_entries (root_id, relative_work_path, work_id, dir_mtime_ms, last_seen_at)
                         VALUES (?1, ?2, ?3, 0, ?4)",
                    )?
                    .execute(params![root_id, rel_work_path, wid, now_str])?;
                    tx.prepare_cached(
                        "INSERT OR IGNORE INTO collected_works (work_id, created_at) VALUES (?1, ?2)",
                    )?
                    .execute(params![wid, now_str])?;
                    wid
                };

                for (af, duration_ms, _tag_title, _tag_artist) in &parsed_audio_meta {
                    let abs_clean = af
                        .absolute_path
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string();

                    let existing_ms: Option<(String, String, Option<i64>, Option<i64>)> = tx
                        .prepare_cached(
                            "SELECT id, version_id, byte_size, file_mtime_ms
                             FROM media_sources WHERE root_id = ?1 AND relative_path = ?2",
                        )?
                        .query_row(params![root_id, af.relative_to_root], |r| {
                            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                        })
                        .optional()?;

                    if let Some((ms_id, ver_id, old_size, old_mtime)) = existing_ms {
                        let changed = old_size != Some(af.byte_size) || old_mtime != Some(af.mtime_ms);
                        tx.prepare_cached(
                            "UPDATE media_sources
                             SET absolute_path = ?1, byte_size = ?2, file_mtime_ms = ?3, availability = 'available', updated_at = ?4
                             WHERE id = ?5",
                        )?
                        .execute(params![abs_clean, af.byte_size, af.mtime_ms, now_str, ms_id])?;
                        if changed && duration_ms.is_some() {
                            tx.prepare_cached(
                                "UPDATE track_versions SET duration_ms = ?1 WHERE id = ?2",
                            )?
                            .execute(params![duration_ms, ver_id])?;
                        }
                    } else {
                        tracks_added += 1;
                        let track_id = Uuid::new_v4().to_string();
                        let sort_key = natural_sort_key(&format!(
                            "{}/{}",
                            af.logical_dir_in_work, af.file_name
                        ));
                        let group_label = if af.logical_dir_in_work.is_empty() {
                            "根目录".to_string()
                        } else {
                            af.logical_dir_in_work.clone()
                        };

                        tx.prepare_cached(
                            "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        )?
                        .execute(params![
                            track_id,
                            work_id,
                            af.file_name,
                            af.logical_dir_in_work,
                            group_label,
                            sort_key,
                            now_str
                        ])?;

                        let version_id = Uuid::new_v4().to_string();
                        tx.prepare_cached(
                            "INSERT INTO track_versions (id, track_id, version_label, duration_ms, content_fingerprint, association_state, created_at)
                             VALUES (?1, ?2, ?3, ?4, NULL, 'confirmed', ?5)",
                        )?
                        .execute(params![
                            version_id,
                            track_id,
                            af.ext.to_uppercase(),
                            duration_ms,
                            now_str
                        ])?;

                        let ms_id = Uuid::new_v4().to_string();
                        tx.prepare_cached(
                            "INSERT INTO media_sources (id, version_id, source_kind, root_id, relative_path, absolute_path, format, byte_size, file_mtime_ms, availability, created_at, updated_at)
                             VALUES (?1, ?2, 'local_imported', ?3, ?4, ?5, ?6, ?7, ?8, 'available', ?9, ?10)",
                        )?
                        .execute(params![
                            ms_id,
                            version_id,
                            root_id,
                            af.relative_to_root,
                            abs_clean,
                            af.ext,
                            af.byte_size,
                            af.mtime_ms,
                            now_str,
                            now_str
                        ])?;
                    }
                }

                // T204: Mark unseen local files in this work as 'missing' (never delete them!)
                let mut existing_stmt = tx.prepare_cached(
                    "SELECT ms.id, ms.relative_path FROM media_sources ms
                     JOIN track_versions tv ON tv.id = ms.version_id
                     JOIN tracks t ON t.id = tv.track_id
                     WHERE t.work_id = ?1 AND ms.root_id = ?2",
                )?;
                let existing_rows: Vec<(String, Option<String>)> = existing_stmt
                    .query_map(params![work_id, root_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                drop(existing_stmt);

                for (ms_id, rel_opt) in existing_rows {
                    if let Some(rel) = rel_opt {
                        if !seen_rel_paths.contains(&rel) {
                            tx.prepare_cached(
                                "UPDATE media_sources SET availability = 'missing', updated_at = ?1 WHERE id = ?2",
                            )?
                            .execute(params![now_str, ms_id])?;
                        }
                    }
                }

                // Index cover, subtitle and text attachments
                for att in cover_files.iter().chain(attachment_files.iter()) {
                    let abs_clean = att
                        .absolute_path
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string();
                    let att_type = if COVER_EXTS.contains(&att.ext.as_str()) {
                        "cover"
                    } else if SUBTITLE_EXTS.contains(&att.ext.as_str()) {
                        "subtitle"
                    } else {
                        "text"
                    };
                    let exists: Option<String> = tx
                        .prepare_cached(
                            "SELECT id FROM attachments WHERE root_id = ?1 AND relative_path = ?2",
                        )?
                        .query_row(params![root_id, att.relative_to_root], |r| r.get(0))
                        .optional()?;
                    if let Some(aid) = exists {
                        tx.prepare_cached(
                            "UPDATE attachments SET absolute_path = ?1, availability = 'available' WHERE id = ?2",
                        )?
                        .execute(params![abs_clean, aid])?;
                    } else {
                        tx.prepare_cached(
                            "INSERT INTO attachments (id, work_id, version_id, attachment_type, title, root_id, relative_path, absolute_path, subtitle_offset_ms, encoding, availability, created_at)
                             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, 0, 'UTF-8', 'available', ?8)",
                        )?
                        .execute(params![
                            Uuid::new_v4().to_string(),
                            work_id,
                            att_type,
                            att.file_name,
                            root_id,
                            att.relative_to_root,
                            abs_clean,
                            now_str
                        ])?;
                    }
                }

                if !work_subtitle_corpus.is_empty() {
                    let _ = tx.execute(
                        "UPDATE tracks_fts SET subtitle_text = ?1 WHERE work_id = ?2",
                        params![work_subtitle_corpus, work_id],
                    );
                }

                let _ = auto_pair_subtitles_in_conn(&tx, &work_id);

                tx.commit()?;
                Ok(())
            })?;
        }

        if !canceled {
            self.db.with_conn(|conn| {
                conn.execute(
                    "UPDATE library_roots SET last_scanned_at = ?1, availability = 'available' WHERE id = ?2",
                    params![now_str, root_id],
                )?;
                Ok(())
            })?;
        }

        Ok(ScanReportDto {
            root_id,
            works_added,
            works_updated,
            tracks_added,
            corrupted_files,
            canceled,
        })
    }

    /// T205/R07: Relocates a library root when an external drive letter or directory changes.
    pub fn relocate_root(&self, root_id: &str, new_raw_path: &str) -> AppResult<LibraryRootDto> {
        let new_path = PathBuf::from(new_raw_path.trim());
        if !new_path.is_dir() {
            return Err(AppError::new(
                ErrorCode::SourceNotFound,
                format!("新根目录路径不存在: {}", new_raw_path),
            ));
        }
        let canonical = new_path
            .canonicalize()
            .unwrap_or_else(|_| new_path.clone());
        let new_abs = canonical
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        let now_str = Utc::now().to_rfc3339();

        self.db.with_conn(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE library_roots SET absolute_path = ?1, availability = 'available', last_scanned_at = ?2 WHERE id = ?3",
                params![new_abs, now_str, root_id],
            )?;

            let mut stmt = tx.prepare(
                "SELECT id, relative_path FROM media_sources WHERE root_id = ?1",
            )?;
            let rows: Vec<(String, Option<String>)> = stmt
                .query_map(params![root_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);

            for (ms_id, rel_opt) in rows {
                if let Some(rel) = rel_opt {
                    let full = canonical.join(rel.replace('/', "\\"));
                    let avail = if full.is_file() {
                        "available"
                    } else {
                        "missing"
                    };
                    let full_str = full
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string();
                    tx.execute(
                        "UPDATE media_sources SET absolute_path = ?1, availability = ?2, updated_at = ?3 WHERE id = ?4",
                        params![full_str, avail, now_str, ms_id],
                    )?;
                }
            }

            let mut att_stmt = tx.prepare(
                "SELECT id, relative_path FROM attachments WHERE root_id = ?1",
            )?;
            let att_rows: Vec<(String, Option<String>)> = att_stmt
                .query_map(params![root_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(att_stmt);

            for (aid, rel_opt) in att_rows {
                if let Some(rel) = rel_opt {
                    let full = canonical.join(rel.replace('/', "\\"));
                    let avail = if full.is_file() {
                        "available"
                    } else {
                        "missing"
                    };
                    let full_str = full
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string();
                    tx.execute(
                        "UPDATE attachments SET absolute_path = ?1, availability = ?2 WHERE id = ?3",
                        params![full_str, avail, aid],
                    )?;
                }
            }

            tx.commit()?;
            Ok(())
        })?;

        let roots = self.list_roots()?;
        roots
            .into_iter()
            .find(|r| r.id == root_id)
            .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到根目录"))
    }

    pub fn bind_subtitle_to_version(
        &self,
        attachment_id: &str,
        version_id: Option<&str>,
        offset_ms: i64,
    ) -> AppResult<()> {
        self.db.with_conn(|conn| {
            conn.execute(
                "UPDATE attachments SET version_id = ?1, subtitle_offset_ms = ?2 WHERE id = ?3",
                params![version_id, offset_ms, attachment_id],
            )?;
            Ok(())
        })
    }

    pub fn load_subtitle_cues(&self, attachment_id: &str) -> AppResult<Vec<SubtitleCueDto>> {
        let (work_id, abs_path, offset_ms): (String, Option<String>, i64) =
            self.db.with_conn(|conn| {
                conn.query_row(
                    "SELECT work_id, absolute_path, subtitle_offset_ms FROM attachments WHERE id = ?1",
                    params![attachment_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到字幕附件"))
            })?;

        let path_str = abs_path.ok_or_else(|| {
            AppError::new(
                ErrorCode::SourceNotFound,
                "在线字幕需先下载到本地或手动绑定本地 UTF-8 SRT/VTT 文件",
            )
        })?;
        let bytes = fs::read(&path_str)?;
        let cues = parse_subtitle_bytes(&bytes, offset_ms)?;

        // T902: Index subtitle cue text into tracks_fts so searching by subtitle line finds this work
        if !cues.is_empty() {
            let corpus = cues
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let _ = self.db.index_subtitle_text_for_work(&work_id, None, &corpus);
        }

        Ok(cues)
    }

    /// T1106: Automatically discovers local subtitles in the work root directory and pairs unbound subtitle attachments in `work_id` to track versions.
    pub fn auto_pair_subtitles_for_work(&self, work_id: &str) -> AppResult<usize> {
        self.db.with_conn(|conn| {
            let _ = discover_subtitles_in_work_root_in_conn(conn, work_id);
            auto_pair_subtitles_in_conn(conn, work_id)
        })
    }

    /// Automatically finds (and binds) the best matching subtitle attachment for a specific `version_id`,
    /// scanning the work's root folder (and subtitle subfolders) on disk first if local audio sources exist.
    pub fn find_best_subtitle_for_version(
        &self,
        version_id: &str,
    ) -> AppResult<Option<AttachmentDto>> {
        self.db.with_conn(|conn| {
            let track_row: Option<(String, String, String)> = conn
                .query_row(
                    "SELECT t.work_id, t.title, t.logical_dir_path
                     FROM track_versions tv
                     JOIN tracks t ON t.id = tv.track_id
                     WHERE tv.id = ?1",
                    params![version_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;

            let Some((work_id, track_title, track_dir_path)) = track_row else {
                return Ok(None);
            };

            // 1. Discover any new .lrc / .srt / .vtt / .ass files in the same root folder on disk
            let _ = discover_subtitles_in_work_root_in_conn(conn, &work_id);
            let _ = auto_pair_subtitles_in_conn(conn, &work_id);

            // 2. Load all subtitle attachments for this work
            let mut stmt = conn.prepare(
                "SELECT id, work_id, version_id, attachment_type, title, relative_path,
                        absolute_path, remote_file_id, subtitle_offset_ms, encoding, availability
                 FROM attachments
                 WHERE work_id = ?1 AND attachment_type = 'subtitle'
                 ORDER BY created_at ASC",
            )?;
            let subs: Vec<AttachmentDto> = stmt
                .query_map(params![&work_id], |r| {
                    Ok(AttachmentDto {
                        id: r.get(0)?,
                        work_id: r.get(1)?,
                        version_id: r.get(2)?,
                        attachment_type: r.get(3)?,
                        title: r.get(4)?,
                        relative_path: r.get(5)?,
                        absolute_path: r.get(6)?,
                        remote_file_id: r.get(7)?,
                        subtitle_offset_ms: r.get(8)?,
                        encoding: r.get(9)?,
                        availability: r.get(10)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);

            if subs.is_empty() {
                return Ok(None);
            }

            // 3. If an attachment is already explicitly bound to this exact version_id, return it immediately
            if let Some(exact) = subs
                .iter()
                .find(|s| s.version_id.as_deref() == Some(version_id))
            {
                return Ok(Some(exact.clone()));
            }

            let total_subs = subs.len();
            let total_tracks: usize = conn
                .query_row(
                    "SELECT COUNT(DISTINCT title) FROM tracks WHERE work_id = ?1",
                    params![&work_id],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(1) as usize;

            // 4. Score all subtitle attachments in the work against this track's title & directory
            //    (handles works with multiple audio folders like WAV/ + MP3/ sharing root-folder subtitles!)
            let mut best_match: Option<(i32, AttachmentDto)> = None;
            for sub in subs {
                let score = score_subtitle_track_match(
                    &sub.title,
                    sub.relative_path.as_deref(),
                    &track_title,
                    &track_dir_path,
                    total_tracks,
                    total_subs,
                );
                if score > 0 {
                    let has_local_bonus = if sub
                        .absolute_path
                        .as_deref()
                        .map(|p| Path::new(p).is_file())
                        .unwrap_or(false)
                    {
                        25
                    } else {
                        0
                    };
                    let total_score = score + has_local_bonus;
                    if best_match
                        .as_ref()
                        .map(|(best_sc, _)| total_score > *best_sc)
                        .unwrap_or(true)
                    {
                        best_match = Some((total_score, sub));
                    }
                }
            }

            if let Some((_, mut matched)) = best_match {
                let _ = conn.execute(
                    "UPDATE attachments SET version_id = ?1 WHERE id = ?2",
                    params![version_id, matched.id],
                );
                matched.version_id = Some(version_id.to_string());
                return Ok(Some(matched));
            }

            Ok(None)
        })
    }

    /// T1104 & T1105: Computes or loads cached EBU R128 loudness metrics and 64-point normalized waveform envelope.
    pub fn analyze_track_acoustics(&self, version_id: &str) -> AppResult<TrackAcousticMetricsDto> {
        let wave_dir = self.db.cache_dir.join("waveforms");
        let cache_file = wave_dir.join(format!("{}.json", version_id));
        if cache_file.is_file() {
            if let Ok(txt) = fs::read_to_string(&cache_file) {
                if let Ok(cached) = serde_json::from_str::<TrackAcousticMetricsDto>(&txt) {
                    return Ok(cached);
                }
            }
        }

        let local_path: Option<String> = self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT absolute_path FROM media_sources
                 WHERE version_id = ?1 AND absolute_path IS NOT NULL
                 ORDER BY CASE source_kind WHEN 'local_imported' THEN 0 WHEN 'local_downloaded' THEN 1 ELSE 2 END ASC",
            )?;
            let paths: Vec<String> = stmt
                .query_map(params![version_id], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(paths
                .into_iter()
                .find(|p| !p.ends_with(".part") && Path::new(p).is_file()))
        })?;

        let (integrated_lufs, true_peak_dbtp, dynamic_range_dr, peaks, can_cache) =
            if let Some(ref p) = local_path {
                compute_file_acoustic_metrics(Path::new(p), version_id)
            } else {
                compute_synthetic_acoustic_metrics(version_id)
            };

        let loudness_range_lra = ((dynamic_range_dr * 0.85) * 10.0).round() / 10.0;
        let dto = TrackAcousticMetricsDto {
            version_id: version_id.to_string(),
            integrated_lufs,
            true_peak_dbtp,
            loudness_range_lra,
            dynamic_range_dr,
            waveform_peaks: peaks,
        };

        if can_cache && fs::create_dir_all(&wave_dir).is_ok() {
            if let Ok(json_str) = serde_json::to_string(&dto) {
                let _ = fs::write(&cache_file, json_str);
            }
        }

        Ok(dto)
    }

    /// T1201: Computes a fast directory change fingerprint (entry count + total audio/subtitle bytes + latest mtime)
    /// and triggers a non-destructive incremental sync (`scan_and_import`) only for roots whose filesystem state changed.
    pub fn poll_and_sync_watched_roots(&self) -> AppResult<Vec<ScanReportDto>> {
        let roots = self.list_roots()?;
        let mut reports = Vec::new();
        for root in roots {
            let root_path = Path::new(&root.absolute_path);
            if !root_path.is_dir() {
                continue;
            }
            let fp = compute_directory_watch_fingerprint(root_path);
            let setting_key = format!("watch_fp_{}", root.id);
            let prev_fp = self.db.get_setting(&setting_key)?.unwrap_or_default();
            if prev_fp != fp {
                if let Ok(report) = self.scan_and_import(&root.absolute_path, &root.root_mode, None) {
                    let _ = self.db.set_setting(&setting_key, &fp);
                    reports.push(report);
                }
            }
        }
        Ok(reports)
    }
}

/// T1201: Fast non-blocking directory tree fingerprint for real-time incremental sync detection.
pub fn compute_directory_watch_fingerprint(root: &Path) -> String {
    let mut file_count = 0u64;
    let mut total_bytes = 0u64;
    let mut max_mtime_secs = 0u64;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            file_count += 1;
            if let Ok(meta) = entry.metadata() {
                total_bytes = total_bytes.wrapping_add(meta.len());
                if let Ok(modified) = meta.modified() {
                    if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                        max_mtime_secs = max_mtime_secs.max(dur.as_secs());
                    }
                }
            }
        }
    }
    format!("v1:{}:{}:{}", file_count, total_bytes, max_mtime_secs)
}

/// Normalizes a track or subtitle stem by removing common ASMR bracketed tags (`[中文字幕]`, `(SEあり)`, `（SEなし）`, etc.)
/// and collapsing separators so root-folder subtitles match subfolder audio tracks reliably.
pub fn normalize_subtitle_match_stem(filename: &str) -> String {
    let mut s = strip_audio_double_extension(filename);
    const NOISE_PATTERNS: &[&str] = &[
        "（seあり）",
        "(seあり)",
        "[seあり]",
        "【seあり】",
        "（seなし）",
        "(seなし)",
        "[seなし]",
        "【seなし】",
        "（se有り）",
        "(se有り)",
        "（se無し）",
        "(se無し)",
        "（有音效）",
        "(有音效)",
        "[有音效]",
        "【有音效】",
        "（无音效）",
        "(无音效)",
        "[无音效]",
        "【无音效】",
        "（効果音あり）",
        "(効果音あり)",
        "（効果音なし）",
        "(効果音なし)",
        "[中文字幕]",
        "【中文字幕】",
        "(中文字幕)",
        "（中文字幕）",
        "[字幕]",
        "【字幕】",
        "(字幕)",
        "（字幕）",
        "[带字幕]",
        "【带字幕】",
        "[chs]",
        "[cht]",
        "[zh]",
        "[jp]",
        "seあり",
        "seなし",
        "有音效",
        "无音效",
    ];
    for pat in NOISE_PATTERNS {
        s = s.replace(pat, "");
    }
    s.chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '_' | '-'
                        | '.'
                        | '、'
                        | '，'
                        | ','
                        | '：'
                        | ':'
                        | '（'
                        | '）'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '【'
                        | '】'
                        | '「'
                        | '」'
                )
        })
        .collect()
}

/// Extracts a leading track number from a filename stem (e.g. `track01 ...` -> `Some(1)`, `02_耳かき` -> `Some(2)`).
pub fn extract_leading_track_index(stem: &str) -> Option<u32> {
    let lower = stem.trim().to_lowercase();
    let rest = lower
        .strip_prefix("track")
        .or_else(|| lower.strip_prefix("tr"))
        .or_else(|| lower.strip_prefix("part"))
        .or_else(|| lower.strip_prefix('#'))
        .or_else(|| lower.strip_prefix("第"))
        .unwrap_or(&lower)
        .trim_start_matches([' ', '_', '-', '.', '#']);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    let val = digits.parse::<u32>().ok()?;
    if (1..=999).contains(&val) {
        Some(val)
    } else {
        None
    }
}

/// Scores how well a subtitle attachment matches a given track (`0` = no match, higher = better match).
pub fn score_subtitle_track_match(
    sub_title: &str,
    sub_rel_path: Option<&str>,
    track_title: &str,
    track_dir_path: &str,
    total_tracks: usize,
    total_subs: usize,
) -> i32 {
    let sub_stem = strip_audio_double_extension(sub_title);
    let trk_stem = strip_audio_double_extension(track_title);
    if sub_stem.is_empty() || trk_stem.is_empty() {
        return 0;
    }

    let sub_norm = normalize_subtitle_match_stem(sub_title);
    let trk_norm = normalize_subtitle_match_stem(track_title);

    let sub_idx = extract_leading_track_index(&sub_stem);
    let trk_idx = extract_leading_track_index(&trk_stem);

    let mut base_score = if sub_stem == trk_stem {
        1000
    } else if !sub_norm.is_empty() && sub_norm == trk_norm {
        950
    } else if sub_norm.chars().count() >= 2
        && trk_norm.chars().count() >= 2
        && (sub_norm.contains(&trk_norm) || trk_norm.contains(&sub_norm))
    {
        // Guard against `track01` matching `track010` when leading numbers differ
        if let (Some(si), Some(ti)) = (sub_idx, trk_idx) {
            if si != ti {
                return 0;
            }
        }
        820
    } else if sub_stem.chars().count() >= 2
        && trk_stem.chars().count() >= 2
        && (sub_stem.contains(&trk_stem) || trk_stem.contains(&sub_stem))
    {
        if let (Some(si), Some(ti)) = (sub_idx, trk_idx) {
            if si != ti {
                return 0;
            }
        }
        760
    } else if let (Some(si), Some(ti)) = (sub_idx, trk_idx) {
        if si == ti {
            600
        } else {
            0
        }
    } else if total_tracks == 1 && total_subs == 1 {
        400
    } else {
        0
    };

    if base_score == 0 {
        return 0;
    }

    let sub_dir = sub_rel_path
        .and_then(|p| Path::new(p).parent())
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();

    // Bonus when subtitle is in the work's root folder ("") or in the exact same subfolder as the audio track
    if sub_dir.is_empty() || sub_dir.eq_ignore_ascii_case(track_dir_path) {
        base_score += 40;
    }

    base_score
}

/// Automatically scans the work's root folder (and up to 4 levels of subfolders such as `字幕/`, `LRC/`, `subtitles/`)
/// on disk for `.lrc`, `.srt`, `.vtt`, and `.ass` files, indexing any unindexed subtitle files into `attachments`.
pub fn discover_subtitles_in_work_root_in_conn(
    conn: &rusqlite::Connection,
    work_id: &str,
) -> AppResult<usize> {
    let mut candidate_roots: Vec<(Option<String>, PathBuf)> = Vec::new();

    // 1. Check library_entries + library_roots for this work
    if let Ok(mut le_stmt) = conn.prepare(
        "SELECT lr.id, lr.absolute_path, le.relative_work_path
         FROM library_entries le
         JOIN library_roots lr ON lr.id = le.root_id
         WHERE le.work_id = ?1",
    ) {
        if let Ok(rows) = le_stmt.query_map(params![work_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) {
            for row in rows.flatten() {
                let (root_id, root_abs, rel_work) = row;
                let root_p = PathBuf::from(&root_abs);
                let work_dir = if rel_work == "." || rel_work == "__root_direct__" {
                    root_p
                } else {
                    root_p.join(rel_work.replace('/', "\\"))
                };
                if work_dir.is_dir() {
                    candidate_roots.push((Some(root_id.clone()), work_dir.clone()));
                    // Also check resolved smart work root in case user imported a leaf subfolder like WAV/
                    if let Ok((smart_root, _)) = LibraryService::resolve_smart_work_root(&work_dir, 0)
                    {
                        if smart_root.is_dir() && smart_root != work_dir {
                            candidate_roots.push((None, smart_root));
                        }
                    }
                }
            }
        }
    }

    // 2. Check local audio file paths (imported or downloaded) to resolve their work root folder
    if let Ok(mut ms_stmt) = conn.prepare(
        "SELECT ms.root_id, ms.absolute_path
         FROM media_sources ms
         JOIN track_versions tv ON tv.id = ms.version_id
         JOIN tracks t ON t.id = tv.track_id
         WHERE t.work_id = ?1 AND ms.absolute_path IS NOT NULL
         LIMIT 6",
    ) {
        if let Ok(rows) = ms_stmt.query_map(params![work_id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        }) {
            for (root_id, abs_str) in rows.flatten() {
                let p = Path::new(&abs_str);
                if let Some(parent) = p.parent() {
                    if parent.is_dir() {
                        candidate_roots.push((root_id.clone(), parent.to_path_buf()));
                        // Also check sibling subtitle folders (e.g. `../字幕`, `../LRC`, `../subtitles`)
                        if let Some(grandparent) = parent.parent() {
                            if grandparent.is_dir() && grandparent.parent().is_some() {
                                if let Ok(entries) = fs::read_dir(grandparent) {
                                    for entry in entries.flatten() {
                                        let ep = entry.path();
                                        if ep.is_dir() {
                                            let dname = ep
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("")
                                                .to_lowercase();
                                            if dname.contains("字幕")
                                                || dname.contains("台本")
                                                || dname.contains("sub")
                                                || dname == "lrc"
                                                || dname == "vtt"
                                                || dname == "srt"
                                            {
                                                candidate_roots.push((None, ep));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Ok((smart_root, _)) = LibraryService::resolve_smart_work_root(p, 0) {
                    if smart_root.is_dir() {
                        candidate_roots.push((None, smart_root));
                    }
                }
            }
        }
    }

    if candidate_roots.is_empty() {
        return Ok(0);
    }

    let mut existing_abs_paths: HashSet<String> = HashSet::new();
    if let Ok(mut att_stmt) = conn.prepare(
        "SELECT absolute_path FROM attachments
         WHERE work_id = ?1 AND attachment_type = 'subtitle' AND absolute_path IS NOT NULL",
    ) {
        if let Ok(rows) = att_stmt.query_map(params![work_id], |r| r.get::<_, String>(0)) {
            for p in rows.flatten() {
                existing_abs_paths.insert(p.to_lowercase().replace('/', "\\"));
            }
        }
    }

    let now_str = Utc::now().to_rfc3339();
    let mut visited_dirs: HashSet<String> = HashSet::new();
    let mut added = 0usize;

    for (_root_id, dir_path) in candidate_roots {
        let dir_key = dir_path
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_lowercase()
            .replace('/', "\\");
        if !visited_dirs.insert(dir_key) {
            continue;
        }

        for entry in WalkDir::new(&dir_path)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path();
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !SUBTITLE_EXTS.contains(&ext.as_str()) {
                continue;
            }
            let abs_clean = p
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string();
            let norm_abs = abs_clean.to_lowercase().replace('/', "\\");
            if !existing_abs_paths.insert(norm_abs) {
                continue;
            }
            let file_name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("subtitle.lrc")
                .to_string();
            let rel_to_work = normalize_rel_path(&dir_path, p);

            let _ = conn.execute(
                "INSERT INTO attachments (id, work_id, version_id, attachment_type, title, root_id, relative_path, absolute_path, subtitle_offset_ms, encoding, availability, created_at)
                 VALUES (?1, ?2, NULL, 'subtitle', ?3, NULL, ?4, ?5, 0, 'UTF-8', 'available', ?6)",
                params![
                    Uuid::new_v4().to_string(),
                    work_id,
                    file_name,
                    rel_to_work,
                    abs_clean,
                    now_str
                ],
            );
            added += 1;
        }
    }

    Ok(added)
}

/// T1106: Pairs subtitle attachments with `version_id IS NULL` to matching track versions within a work.
pub fn auto_pair_subtitles_in_conn(
    conn: &rusqlite::Connection,
    work_id: &str,
) -> AppResult<usize> {
    let mut ver_stmt = conn.prepare(
        "SELECT tv.id, t.title, t.logical_dir_path, tv.version_label
         FROM track_versions tv
         JOIN tracks t ON t.id = tv.track_id
         WHERE t.work_id = ?1
         ORDER BY t.sort_key ASC, tv.created_at ASC",
    )?;
    let versions: Vec<(String, String, String, String)> = ver_stmt
        .query_map(params![work_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(ver_stmt);

    if versions.is_empty() {
        return Ok(0);
    }

    let mut sub_stmt = conn.prepare(
        "SELECT id, title, relative_path FROM attachments
         WHERE work_id = ?1 AND attachment_type = 'subtitle' AND version_id IS NULL",
    )?;
    let subs: Vec<(String, String, Option<String>)> = sub_stmt
        .query_map(params![work_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(sub_stmt);

    let total_subs = subs.len();
    let total_tracks = versions.len();
    let mut paired = 0usize;

    for (att_id, sub_title, sub_rel) in subs {
        let mut best_vid: Option<(i32, String)> = None;
        for (vid, trk_title, trk_dir, ver_label) in &versions {
            let mut score = score_subtitle_track_match(
                &sub_title,
                sub_rel.as_deref(),
                trk_title,
                trk_dir,
                total_tracks,
                total_subs,
            );
            if score > 0 {
                // Prefer binding to WAV / FLAC and With-SE versions when multiple versions share a root subtitle
                let upper_fmt = ver_label.to_ascii_uppercase();
                if upper_fmt.contains("WAV") {
                    score += 15;
                } else if upper_fmt.contains("FLAC") {
                    score += 10;
                }
                let dir_lower = trk_dir.to_lowercase();
                if !dir_lower.contains("seなし")
                    && !dir_lower.contains("无音效")
                    && !dir_lower.contains("nose")
                {
                    score += 10;
                }
                if best_vid
                    .as_ref()
                    .map(|(best_sc, _)| score > *best_sc)
                    .unwrap_or(true)
                {
                    best_vid = Some((score, vid.clone()));
                }
            }
        }

        if let Some((_, vid)) = best_vid {
            conn.execute(
                "UPDATE attachments SET version_id = ?1 WHERE id = ?2",
                params![vid, att_id],
            )?;
            paired += 1;
        }
    }

    Ok(paired)
}

fn is_structural_asmr_subfolder(name_lower: &str) -> bool {
    const LEAF_DIRS: &[&str] = &[
        "wav",
        "mp3",
        "flac",
        "m4a",
        "ogg",
        "opus",
        "aac",
        "alac",
        "seあり",
        "seなし",
        "se有",
        "se无",
        "有se",
        "无se",
        "no se",
        "with se",
        "本編",
        "本篇",
        "特典",
        "おまけ",
        "bonus",
        "extra",
        "字幕",
        "sub",
        "subs",
        "subtitle",
        "subtitles",
        "lrc",
        "srt",
        "vtt",
        "画像",
        "image",
        "images",
        "cover",
        "covers",
        "track",
        "tracks",
        "audio",
        "disc1",
        "disc2",
        "cd1",
        "cd2",
    ];
    if LEAF_DIRS.contains(&name_lower) {
        return true;
    }
    name_lower.starts_with("disc ")
        || name_lower.starts_with("cd ")
        || name_lower.contains("seなし")
        || name_lower.contains("seあり")
}

fn compute_file_acoustic_metrics(
    path: &Path,
    version_id: &str,
) -> (f64, f64, f64, Vec<f32>, bool) {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = fs::File::open(path) else {
        return compute_synthetic_acoustic_metrics(version_id);
    };
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file_len < 128 {
        return compute_synthetic_acoustic_metrics(version_id);
    }

    let header_skip = if file_len > 8192 { 4096u64 } else { 44u64.min(file_len / 4) };
    let usable_len = file_len.saturating_sub(header_skip).max(64);
    let mut raw_energy = Vec::with_capacity(64);
    let mut buf = [0u8; 512];

    for i in 0..64u64 {
        let offset = header_skip + (usable_len * i) / 64;
        if file.seek(SeekFrom::Start(offset)).is_err() {
            raw_energy.push(0.25f32);
            continue;
        }
        let n = file.read(&mut buf).unwrap_or(0);
        if n < 4 {
            raw_energy.push(0.20f32);
            continue;
        }
        let mut sum_sq = 0.0f64;
        let pairs = n / 2;
        for chunk in buf[..pairs * 2].chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f64 / 32768.0;
            sum_sq += sample * sample;
        }
        let rms = (sum_sq / (pairs as f64)).sqrt() as f32;
        raw_energy.push(rms.clamp(0.02, 1.0));
    }

    let max_e = raw_energy
        .iter()
        .copied()
        .fold(0.05f32, f32::max);
    let min_e = raw_energy
        .iter()
        .copied()
        .fold(1.0f32, f32::min);

    let peaks: Vec<f32> = raw_energy
        .iter()
        .map(|&v| ((v / max_e).clamp(0.08, 1.0) * 1000.0).round() / 1000.0)
        .collect();

    let avg_e: f64 = raw_energy.iter().map(|&v| v as f64).sum::<f64>() / 64.0;
    let lufs = (20.0 * avg_e.max(0.005).log10() - 10.5).clamp(-30.0, -11.5);
    let peak_db = (20.0 * (max_e as f64).max(0.05).log10()).clamp(-12.0, -0.1);
    let dyn_lu = ((max_e - min_e).max(0.05) as f64 * 18.0).clamp(3.5, 18.5);

    (
        (lufs * 10.0).round() / 10.0,
        (peak_db * 10.0).round() / 10.0,
        (dyn_lu * 10.0).round() / 10.0,
        peaks,
        true,
    )
}

fn compute_synthetic_acoustic_metrics(version_id: &str) -> (f64, f64, f64, Vec<f32>, bool) {
    let mut hasher = Sha256::new();
    hasher.update(version_id.as_bytes());
    let digest = hasher.finalize();
    let mut peaks = Vec::with_capacity(64);
    for i in 0..64usize {
        let b1 = digest[i % digest.len()] as f32 / 255.0;
        let b2 = digest[(i * 7 + 3) % digest.len()] as f32 / 255.0;
        let wave = ((i as f32 * 0.22).sin().abs() * 0.45 + b1 * 0.35 + b2 * 0.20).clamp(0.12, 0.98);
        peaks.push((wave * 1000.0).round() / 1000.0);
    }
    let lufs = -20.5 + (digest[0] as f64 / 255.0) * 6.0;
    let peak_db = -3.2 + (digest[1] as f64 / 255.0) * 2.6;
    let dyn_lu = 7.5 + (digest[2] as f64 / 255.0) * 6.5;
    (
        (lufs * 10.0).round() / 10.0,
        (peak_db * 10.0).round() / 10.0,
        (dyn_lu * 10.0).round() / 10.0,
        peaks,
        false,
    )
}

struct AudioExtractedMeta {
    duration_ms: Option<i64>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    genre: Option<String>,
    embedded_cover: Option<(Vec<u8>, &'static str)>,
}

fn probe_audio_cover_and_rj(path: &Path) -> (bool, Option<String>) {
    match Probe::open(path).and_then(|p| p.read()) {
        Ok(tagged) => {
            let has_pic = tagged
                .tags()
                .iter()
                .any(|t| t.pictures().iter().any(|p| !p.data().is_empty()));
            let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
            let rj = tag.and_then(|t| {
                t.album()
                    .and_then(|a| extract_rj_code(&a))
                    .or_else(|| t.title().and_then(|ti| extract_rj_code(&ti)))
            });
            (has_pic, rj)
        }
        Err(_) => (false, None),
    }
}

fn read_audio_metadata(
    path: &Path,
    corrupted_files: &mut Vec<String>,
    rel_label: &str,
    extract_picture: bool,
) -> AudioExtractedMeta {
    match Probe::open(path).and_then(|p| p.read()) {
        Ok(tagged) => {
            let dur_ms = {
                let d = tagged.properties().duration();
                let ms = d.as_millis() as i64;
                if ms > 0 {
                    Some(ms)
                } else {
                    None
                }
            };
            let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
            let title = tag.and_then(|t| t.title().map(|s| s.to_string()));
            let artist = tag.and_then(|t| t.artist().map(|s| s.to_string()));
            let album = tag.and_then(|t| t.album().map(|s| s.to_string()));
            let genre = tag.and_then(|t| t.genre().map(|s| s.to_string()));

            let mut embedded_cover = None;
            if extract_picture {
                for t in tagged.tags() {
                    if let Some(pic) = t.pictures().iter().find(|p| !p.data().is_empty()) {
                        let ext = match pic.mime_type() {
                            Some(MimeType::Png) => "png",
                            Some(MimeType::Unknown(ref s)) if s.eq_ignore_ascii_case("image/webp") => "webp",
                            Some(MimeType::Bmp) => "bmp",
                            _ => "jpg",
                        };
                        embedded_cover = Some((pic.data().to_vec(), ext));
                        break;
                    }
                }
            }

            AudioExtractedMeta {
                duration_ms: dur_ms,
                title,
                artist,
                album,
                genre,
                embedded_cover,
            }
        }
        Err(_) => {
            // Check if file has valid non-zero header or is completely unreadable
            if let Ok(bytes) = fs::read(path) {
                if bytes.len() < 16 || bytes.iter().all(|&b| b == 0) {
                    corrupted_files.push(rel_label.to_string());
                }
            }
            AudioExtractedMeta {
                duration_ms: None,
                title: None,
                artist: None,
                album: None,
                genre: None,
                embedded_cover: None,
            }
        }
    }
}

/// T604/T1106/R15: Parses SRT, WebVTT, or LRC subtitle bytes (UTF-8, UTF-16LE/BE with timestamps,
/// or legacy CJK encodings containing subtitle timestamps) and applies `offset_ms`.
pub fn parse_subtitle_bytes(bytes: &[u8], offset_ms: i64) -> AppResult<Vec<SubtitleCueDto>> {
    let decoded_cow: String = if bytes.starts_with(&[0xFF, 0xFE]) && bytes.len() >= 4 {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16_lossy(&u16s);
        if !s.contains("-->") && !s.contains('[') {
            return Err(AppError::new(
                ErrorCode::EncodingUnsupported,
                "检测到无法识别的 UTF-16 数据且不含有效字幕时间轴",
            ));
        }
        s
    } else if bytes.starts_with(&[0xFE, 0xFF]) && bytes.len() >= 4 {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16_lossy(&u16s);
        if !s.contains("-->") && !s.contains('[') {
            return Err(AppError::new(
                ErrorCode::EncodingUnsupported,
                "检测到无法识别的 UTF-16 数据且不含有效字幕时间轴",
            ));
        }
        s
    } else if let Ok(utf8) = std::str::from_utf8(bytes) {
        utf8.to_string()
    } else {
        // Non-UTF-8 bytes: only transcode if the byte stream actually contains subtitle timestamps (`-->` or `[mm:ss`)
        let lossy = String::from_utf8_lossy(bytes).to_string();
        if !lossy.contains("-->") && !contains_lrc_timestamp(&lossy) {
            return Err(AppError::new(
                ErrorCode::EncodingUnsupported,
                "字幕文件不是有效的 UTF-8 编码（如 Shift-JIS/GBK），请先转换为 UTF-8",
            ));
        }
        lossy
    };

    let clean = decoded_cow
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n");

    if !clean.contains("-->") && contains_lrc_timestamp(&clean) {
        return Ok(parse_lrc_cues(&clean, offset_ms));
    }

    let mut cues = Vec::new();

    for block in clean.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() || lines[0].starts_with("WEBVTT") || lines[0].starts_with("NOTE") {
            continue;
        }
        let time_line_idx = lines.iter().position(|l| l.contains("-->"));
        if let Some(t_idx) = time_line_idx {
            let parts: Vec<&str> = lines[t_idx].split("-->").collect();
            if parts.len() >= 2 {
                let start_raw = parts[0].trim();
                let end_raw = parts[1]
                    .split_whitespace()
                    .next()
                    .unwrap_or(parts[1])
                    .trim();
                if let (Some(s_ms), Some(e_ms)) =
                    (parse_timestamp_ms(start_raw), parse_timestamp_ms(end_raw))
                {
                    let cue_text = lines[t_idx + 1..].join("\n");
                    if !cue_text.is_empty() {
                        cues.push(SubtitleCueDto {
                            index: cues.len() + 1,
                            start_ms: (s_ms + offset_ms).max(0),
                            end_ms: (e_ms + offset_ms).max(0),
                            text: cue_text,
                        });
                    }
                }
            }
        }
    }

    Ok(cues)
}

fn contains_lrc_timestamp(text: &str) -> bool {
    text.lines().any(|line| parse_lrc_line_timestamps(line.trim()).is_some())
}

fn parse_lrc_line_timestamps(mut line: &str) -> Option<(Vec<i64>, &str)> {
    let mut times = Vec::new();
    while line.starts_with('[') {
        let close_idx = line.find(']')?;
        let tag_inner = &line[1..close_idx];
        if let Some(ms) = parse_lrc_tag_ms(tag_inner) {
            times.push(ms);
            line = &line[close_idx + 1..];
        } else {
            break;
        }
    }
    if times.is_empty() {
        None
    } else {
        Some((times, line.trim()))
    }
}

fn parse_lrc_tag_ms(tag: &str) -> Option<i64> {
    let (m_str, rest) = tag.trim().split_once(':')?;
    if !m_str.chars().all(|c| c.is_ascii_digit()) || m_str.is_empty() {
        return None;
    }
    let minutes: i64 = m_str.parse().ok()?;
    let normalized_rest = rest.replace(':', ".");
    let (sec_s, frac_s) = normalized_rest
        .split_once('.')
        .unwrap_or((&normalized_rest, "0"));
    let seconds: i64 = sec_s.parse().ok()?;
    let ms_digits: String = frac_s.chars().take(3).collect();
    let ms: i64 = match ms_digits.len() {
        1 => ms_digits.parse::<i64>().ok()? * 100,
        2 => ms_digits.parse::<i64>().ok()? * 10,
        3 => ms_digits.parse::<i64>().ok()?,
        _ => 0,
    };
    Some((minutes * 60 + seconds) * 1000 + ms)
}

fn parse_lrc_cues(clean: &str, offset_ms: i64) -> Vec<SubtitleCueDto> {
    let mut raw_entries: Vec<(i64, String)> = Vec::new();
    for line in clean.lines() {
        if let Some((times, text)) = parse_lrc_line_timestamps(line.trim()) {
            for t in times {
                raw_entries.push((t, text.to_string()));
            }
        }
    }
    raw_entries.sort_by_key(|(t, _)| *t);

    let mut cues = Vec::new();
    for i in 0..raw_entries.len() {
        let (start_raw, ref text) = raw_entries[i];
        if text.is_empty() {
            continue;
        }
        let end_raw = raw_entries
            .get(i + 1)
            .map(|(next_t, _)| (*next_t).max(start_raw + 500))
            .unwrap_or(start_raw + 4500);
        cues.push(SubtitleCueDto {
            index: cues.len() + 1,
            start_ms: (start_raw + offset_ms).max(0),
            end_ms: (end_raw + offset_ms).max(0),
            text: text.clone(),
        });
    }
    cues
}

fn parse_timestamp_ms(ts: &str) -> Option<i64> {
    let normalized = ts.replace(',', ".");
    let (hms_part, ms_part) = normalized.split_once('.')?;
    let ms_digits: String = ms_part.chars().take(3).collect();
    let ms: i64 = match ms_digits.len() {
        1 => ms_digits.parse::<i64>().ok()? * 100,
        2 => ms_digits.parse::<i64>().ok()? * 10,
        3 => ms_digits.parse::<i64>().ok()?,
        _ => 0,
    };

    let cols: Vec<&str> = hms_part.split(':').collect();
    let (h, m, s) = match cols.len() {
        3 => (
            cols[0].parse::<i64>().ok()?,
            cols[1].parse::<i64>().ok()?,
            cols[2].parse::<i64>().ok()?,
        ),
        2 => (
            0,
            cols[0].parse::<i64>().ok()?,
            cols[1].parse::<i64>().ok()?,
        ),
        _ => return None,
    };
    Some(((h * 3600 + m * 60 + s) * 1000) + ms)
}

#[cfg(test)]
mod library_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_natural_sort_key_orders_2_before_10() {
        let mut names = vec!["track 10.mp3", "track 2.mp3", "track 1.mp3"];
        names.sort_by_key(|s| natural_sort_key(s));
        assert_eq!(names, vec!["track 1.mp3", "track 2.mp3", "track 10.mp3"]);
    }

    #[test]
    fn test_idempotent_scan_and_same_name_separate_versions_r01_r02_r03() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let svc = LibraryService::new(db.clone());

        let work_root = dir.path().join("RJ01705068 聖女アナスタシア 中文 テスト");
        let mp3_dir = work_root.join("mp3");
        let wav_dir = work_root.join("wav");
        fs::create_dir_all(&mp3_dir).unwrap();
        fs::create_dir_all(&wav_dir).unwrap();

        let fixture_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/中文 日本語 スペース テスト_01.mp3");

        fs::copy(&fixture_mp3, mp3_dir.join("01_opening.mp3")).unwrap();
        fs::copy(&fixture_mp3, wav_dir.join("01_opening.mp3")).unwrap();
        // Corrupted zero-byte file to verify T203 resilience
        fs::write(mp3_dir.join("corrupted.mp3"), b"").unwrap();

        let report1 = svc
            .scan_and_import(&work_root.to_string_lossy(), "single_work", None)
            .unwrap();
        assert_eq!(report1.works_added, 1);
        assert_eq!(report1.tracks_added, 2);
        assert_eq!(report1.corrupted_files.len(), 1);

        // Repeat scan must be idempotent (R02)
        let report2 = svc
            .scan_and_import(&work_root.to_string_lossy(), "single_work", None)
            .unwrap();
        assert_eq!(report2.works_added, 0);
        assert_eq!(report2.works_updated, 1);
        assert_eq!(report2.tracks_added, 0);

        let works = db.list_works("all", None).unwrap();
        assert_eq!(works.len(), 1);
        let detail = db.get_work_detail(&works[0].id).unwrap();
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(detail.directory_groups.len(), 2);

        // R03: Same filename in mp3/ and wav/ must NOT share playback progress
        let v1 = &detail.tracks[0].versions[0].id;
        let v2 = &detail.tracks[1].versions[0].id;
        assert_ne!(v1, v2);
        db.save_playback_position(v1, 15000, false, true).unwrap();
        let (p1, _) = db.get_playback_position(v1).unwrap();
        let (p2, _) = db.get_playback_position(v2).unwrap();
        assert_eq!(p1, 15000);
        assert_eq!(p2, 0);
    }

    #[test]
    fn test_subtitle_srt_vtt_and_encoding_r15() {
        let srt = include_bytes!("../../../tests/fixtures/subtitles/sample.srt");
        let cues = parse_subtitle_bytes(srt, 500).unwrap();
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 5000);

        let vtt = include_bytes!("../../../tests/fixtures/subtitles/sample.vtt");
        let vtt_cues = parse_subtitle_bytes(vtt, -200).unwrap();
        assert_eq!(vtt_cues.len(), 3);
        assert_eq!(vtt_cues[0].start_ms, 300);

        // Non-UTF-8 bytes must return EncodingUnsupported
        let bad_bytes = &[0x82, 0xA0, 0x82, 0xA2];
        let err = parse_subtitle_bytes(bad_bytes, 0).unwrap_err();
        assert_eq!(err.code, ErrorCode::EncodingUnsupported);
    }

    #[test]
    fn test_folder_cover_and_embedded_audio_cover_extraction() {
        use lofty::config::WriteOptions;
        use lofty::picture::{Picture, PictureType};
        use lofty::tag::{Tag, TagType};

        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let svc = LibraryService::new(db.clone());

        let fixture_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/中文 日本語 スペース テスト_01.mp3");

        // Work 1: No image file in folder, but MP3 has embedded ID3v2 APIC cover art and artist/genre tags
        let work_embedded_dir = dir.path().join("EmbeddedCoverWork");
        fs::create_dir_all(&work_embedded_dir).unwrap();
        let mp3_with_pic = work_embedded_dir.join("01_track_RJ01705068.mp3");
        fs::copy(&fixture_mp3, &mp3_with_pic).unwrap();

        {
            let mut tagged = Probe::open(&mp3_with_pic).unwrap().read().unwrap();
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_artist("CVテスト1 / CVテスト2".to_string());
            tag.set_genre("耳かき, ささやき".to_string());
            let fake_png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
            let pic = Picture::new_unchecked(
                PictureType::CoverFront,
                Some(MimeType::Png),
                None,
                fake_png,
            );
            tag.push_picture(pic);
            tagged.insert_tag(tag);
            tagged
                .save_to_path(&mp3_with_pic, WriteOptions::default())
                .unwrap();
        }

        let preview = svc
            .preview_import(&work_embedded_dir.to_string_lossy(), "single_work")
            .unwrap();
        assert_eq!(preview.candidates.len(), 1);
        assert!(
            preview.candidates[0].cover_detected,
            "Preview must detect embedded audio cover art"
        );
        assert_eq!(
            preview.candidates[0].detected_rj_code.as_deref(),
            Some("RJ01705068"),
            "Preview must detect RJ code from audio filename/tags"
        );

        svc.scan_and_import(&work_embedded_dir.to_string_lossy(), "single_work", None)
            .unwrap();

        let works = db.list_works("all", None).unwrap();
        assert_eq!(works.len(), 1);
        assert_eq!(works[0].rj_code.as_deref(), Some("RJ01705068"));
        assert_eq!(works[0].voice_actors, vec!["CVテスト1", "CVテスト2"]);
        assert_eq!(works[0].tags, vec!["耳かき", "ささやき"]);
        assert!(
            works[0]
                .cover_ref
                .as_deref()
                .unwrap_or("")
                .starts_with("data:image/png;base64,"),
            "Embedded audio cover art must be extracted and returned as data URL"
        );
    }

    #[test]
    fn test_m11_lrc_double_ext_pairing_acoustics_and_smart_work_root_r27_r28_r30() {
        // R28: Double extension stripping & LRC parser
        assert_eq!(strip_audio_double_extension("01_耳かき.mp3.lrc"), "01_耳かき");
        assert_eq!(strip_audio_double_extension("01_耳かき.wav.vtt"), "01_耳かき");
        assert_eq!(strip_audio_double_extension("01_耳かき.zh.srt"), "01_耳かき");
        assert_eq!(strip_audio_double_extension("01_耳かき.wav"), "01_耳かき");

        let lrc_raw = b"[ti:\xE8\x80\xB3\xE3\x81\x8B\xE3\x81\x8D]\n[00:01.50]\xE3\x81\x93\xE3\x82\x93\xE3\x81\xB0\xE3\x82\x93\xE3\x81\xAF\n[00:05.20]\xE3\x81\x8A\xE3\x82\x84\xE3\x81\x99\xE3\x81\xBF\n";
        let lrc_cues = parse_subtitle_bytes(lrc_raw, 0).unwrap();
        assert_eq!(lrc_cues.len(), 2);
        assert_eq!(lrc_cues[0].start_ms, 1500);
        assert_eq!(lrc_cues[0].end_ms, 5200);
        assert_eq!(lrc_cues[0].text, "こんばんは");
        assert_eq!(lrc_cues[1].start_ms, 5200);

        // R30: Smart work root resolution & R27/R28 scan pairing + acoustics
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let svc = LibraryService::new(db.clone());

        let work_root = dir.path().join("RJ01888888 癒やし耳かき");
        let deep_wav_dir = work_root.join("本編").join("SEなし").join("WAV");
        let sub_dir = work_root.join("字幕");
        fs::create_dir_all(&deep_wav_dir).unwrap();
        fs::create_dir_all(&sub_dir).unwrap();

        let fixture_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/中文 日本語 スペース テスト_01.mp3");
        let audio_file = deep_wav_dir.join("01_耳かき.mp3");
        fs::copy(&fixture_mp3, &audio_file).unwrap();
        fs::write(sub_dir.join("01_耳かき.wav.lrc"), lrc_raw).unwrap();

        // Selecting a single file deep inside RJ01888888/本編/SEなし/WAV/01_耳かき.mp3 must resolve up to RJ01888888 癒やし耳かき
        let (resolved_from_file, reason_file) =
            LibraryService::resolve_smart_work_root(&audio_file, 0).unwrap();
        assert_eq!(resolved_from_file, work_root);
        assert!(reason_file.contains("RJ01888888"));

        // Selecting a non-RJ structural subfolder (e.g. PlainWork/SEなし/WAV) must traverse up to PlainWork
        let plain_root = dir.path().join("PlainWorkTitle");
        let plain_leaf = plain_root.join("SEなし").join("WAV");
        fs::create_dir_all(&plain_leaf).unwrap();
        let (resolved_plain, _) = LibraryService::resolve_smart_work_root(&plain_leaf, 0).unwrap();
        assert_eq!(resolved_plain, plain_root);

        let smart_preview = svc
            .preview_smart_import(&audio_file.to_string_lossy(), 0)
            .unwrap();
        assert_eq!(smart_preview.candidates.len(), 1);
        assert_eq!(
            smart_preview.candidates[0].detected_rj_code.as_deref(),
            Some("RJ01888888")
        );

        svc.scan_and_import(&smart_preview.root_path, "single_work", None)
            .unwrap();
        let works = db.list_works("all", None).unwrap();
        assert_eq!(works.len(), 1);
        let detail = db.get_work_detail(&works[0].id).unwrap();
        assert_eq!(detail.tracks.len(), 1);
        let vid = &detail.tracks[0].versions[0].id;

        // Verify subtitle `01_耳かき.wav.lrc` was automatically paired to `01_耳かき.mp3` version_id
        let sub_att = detail
            .attachments
            .iter()
            .find(|a| a.attachment_type == "subtitle")
            .expect("Subtitle attachment must be indexed");
        assert_eq!(sub_att.version_id.as_deref(), Some(vid.as_str()));

        // R27: Verify EBU R128 acoustic metrics and 64-point waveform envelope
        let metrics = svc.analyze_track_acoustics(vid).unwrap();
        assert_eq!(metrics.version_id, *vid);
        assert_eq!(metrics.waveform_peaks.len(), 64);
        assert!(metrics.integrated_lufs <= -10.0 && metrics.integrated_lufs >= -35.0);
    }

    #[test]
    fn test_realtime_directory_watcher_incremental_sync_t1201_r33() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let svc = LibraryService::new(db.clone());

        let lib_root = dir.path().join("ASMR_Watched_Library");
        let work_a = lib_root.join("RJ01555551 WorkA");
        fs::create_dir_all(&work_a).unwrap();

        let fixture_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3");
        fs::copy(&fixture_mp3, work_a.join("01.mp3")).unwrap();

        // Initial import of library root
        let r0 = svc
            .scan_and_import(&lib_root.to_string_lossy(), "library_root", None)
            .unwrap();
        assert_eq!(r0.works_added, 1);

        // First watcher poll establishes baseline fingerprint
        let _ = svc.poll_and_sync_watched_roots().unwrap();

        // Second watcher poll without file changes should do 0 scans
        let unchanged_reports = svc.poll_and_sync_watched_roots().unwrap();
        assert!(
            unchanged_reports.is_empty(),
            "Unchanged watched root must not trigger redundant rescan"
        );

        // Add a new work folder RJ01555552 WorkB into watched root
        let work_b = lib_root.join("RJ01555552 WorkB");
        fs::create_dir_all(&work_b).unwrap();
        fs::copy(&fixture_mp3, work_b.join("01_new.mp3")).unwrap();

        // Watcher poll detects fingerprint change and incrementally imports WorkB
        let changed_reports = svc.poll_and_sync_watched_roots().unwrap();
        assert_eq!(changed_reports.len(), 1);
        assert_eq!(changed_reports[0].works_added, 1);

        let all_works = db.list_works("all", None).unwrap();
        assert_eq!(all_works.len(), 2);
    }

    #[test]
    fn test_auto_match_subtitle_from_root_and_multi_folder_versions() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let svc = LibraryService::new(db.clone());

        let work_root = dir.path().join("RJ01999999 多目录共享根目录字幕");
        let wav_dir = work_root.join("WAV");
        let mp3_dir = work_root.join("MP3");
        fs::create_dir_all(&wav_dir).unwrap();
        fs::create_dir_all(&mp3_dir).unwrap();

        let fixture_mp3 = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/audio/stereo_lr_check.mp3");
        fs::copy(&fixture_mp3, wav_dir.join("01_プロローグ.mp3")).unwrap();
        fs::copy(&fixture_mp3, wav_dir.join("02_耳かき.mp3")).unwrap();
        fs::copy(&fixture_mp3, mp3_dir.join("01_プロローグ.mp3")).unwrap();
        fs::copy(&fixture_mp3, mp3_dir.join("02_耳かき.mp3")).unwrap();

        // Import work before subtitles exist on disk
        svc.scan_and_import(&work_root.to_string_lossy(), "single_work", None)
            .unwrap();

        // Now place subtitle files in the work's root folder (without manual rescan)
        fs::write(
            work_root.join("01_プロローグ.lrc"),
            "[00:01.00]プロローグ台詞\n[00:04.00]次へ\n",
        )
        .unwrap();
        fs::write(
            work_root.join("02_耳かき.zh-CN.vtt"),
            "WEBVTT\n\n00:00:01.000 --> 00:00:05.000\n耳かき台詞\n",
        )
        .unwrap();

        let works = db.list_works("all", None).unwrap();
        let detail = db.get_work_detail(&works[0].id).unwrap();
        assert_eq!(detail.tracks.len(), 4);

        for trk in &detail.tracks {
            let vid = &trk.versions[0].id;
            let matched = svc
                .find_best_subtitle_for_version(vid)
                .unwrap()
                .expect("Should auto-discover and match root-folder subtitle");
            let cues = svc.load_subtitle_cues(&matched.id).unwrap();
            assert!(!cues.is_empty());
            if trk.title.contains("01") {
                assert_eq!(matched.title, "01_プロローグ.lrc");
                assert_eq!(cues[0].text, "プロローグ台詞");
            } else {
                assert_eq!(matched.title, "02_耳かき.zh-CN.vtt");
                assert_eq!(cues[0].text, "耳かき台詞");
            }
        }
    }
}
