use crate::db::Database;
use crate::domain::*;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use uuid::Uuid;

struct LruCoverCache {
    map: HashMap<String, Option<String>>,
    order: VecDeque<String>,
    capacity: usize,
}

impl LruCoverCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn get(&mut self, key: &str) -> Option<Option<String>> {
        let val = self.map.get(key).cloned()?;
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.to_string());
        Some(val)
    }

    fn insert(&mut self, key: String, val: Option<String>) {
        if self.map.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
        } else if self.map.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, val);
    }
}

static COVER_DATA_URL_CACHE: LazyLock<Mutex<LruCoverCache>> =
    LazyLock::new(|| Mutex::new(LruCoverCache::new(512)));

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Converts a local cover file path into a `data:image/...;base64,...` URL so WebView2 renders it
/// without cross-scheme blocking, while keeping `http(s)://` and `data:` URLs intact.
/// Uses an in-memory bounded LRU cache (T904) to avoid repeated disk stats/reads during `list_works`.
pub fn resolve_cover_for_ui(raw_cover: Option<String>) -> Option<String> {
    let raw = raw_cover?.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("data:") {
        return Some(raw);
    }

    {
        let mut cache = COVER_DATA_URL_CACHE.lock();
        if let Some(cached) = cache.get(&raw) {
            return cached;
        }
    }

    let path = Path::new(&raw);
    let meta = match fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() > 0 && m.len() <= 15 * 1024 * 1024 => m,
        _ => {
            let mut cache = COVER_DATA_URL_CACHE.lock();
            cache.insert(raw.clone(), Some(raw.clone()));
            return Some(raw);
        }
    };
    let _len = meta.len();

    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => {
            let mut cache = COVER_DATA_URL_CACHE.lock();
            cache.insert(raw.clone(), Some(raw.clone()));
            return Some(raw);
        }
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    };
    let data_url = format!("data:{};base64,{}", mime, encode_base64(&bytes));

    {
        let mut cache = COVER_DATA_URL_CACHE.lock();
        cache.insert(raw, Some(data_url.clone()));
    }

    Some(data_url)
}

impl Database {
    pub fn get_setting(&self, key: &str) -> AppResult<Option<String>> {
        self.with_conn(|conn| {
            let val: Option<String> = conn
                .query_row(
                    "SELECT value_json FROM app_settings WHERE key = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(val)
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_settings (key, value_json, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                params![key, value, now],
            )?;
            Ok(())
        })
    }

    pub fn resolve_effective_work_fields_from_data(
        ov_map: Option<&BTreeMap<String, (String, Option<String>)>>,
        online_meta: Option<&Value>,
        base_title: &str,
        base_desc: &str,
        base_circle: &str,
        base_cover: &Option<String>,
        base_vas_json: &str,
        base_tags_json: &str,
    ) -> (
        String,
        String,
        String,
        Option<String>,
        Vec<String>,
        Vec<String>,
    ) {
        let resolve_str = |field: &str, online_val: Option<&str>, local_val: &str| -> String {
            if let Some(map) = ov_map {
                if let Some((mode, val)) = map.get(field) {
                    if mode == "cleared" {
                        return String::new();
                    }
                    if let Some(v) = val {
                        return v.clone();
                    }
                }
            }
            if let Some(ov) = online_val {
                if !ov.trim().is_empty() {
                    return ov.to_string();
                }
            }
            local_val.to_string()
        };

        let online_title = online_meta
            .and_then(|m| m.get("title"))
            .and_then(|v| v.as_str());
        let online_desc = online_meta
            .and_then(|m| m.get("description"))
            .and_then(|v| v.as_str());
        let online_circle = online_meta
            .and_then(|m| {
                m.get("circle_name")
                    .or_else(|| m.get("name"))
                    .or_else(|| m.get("circle").and_then(|c| c.get("name")))
            })
            .and_then(|v| v.as_str());
        let online_cached_cover = online_meta
            .and_then(|m| m.get("cached_cover_path"))
            .and_then(|v| v.as_str())
            .filter(|p| Path::new(p).is_file());
        let online_cover = online_cached_cover.or_else(|| {
            online_meta
                .and_then(|m| {
                    m.get("mainCoverUrl")
                        .or_else(|| m.get("cover_url"))
                        .or_else(|| m.get("thumbnailCoverUrl"))
                        .or_else(|| m.get("samCoverUrl"))
                })
                .and_then(|v| v.as_str())
        });

        let title = {
            let t = resolve_str("title", online_title, base_title);
            if t.is_empty() {
                base_title.to_string()
            } else {
                t
            }
        };
        let description = resolve_str("description", online_desc, base_desc);
        let circle_name = resolve_str("circle_name", online_circle, base_circle);

        let raw_cover_ref = if let Some((mode, val)) = ov_map.and_then(|m| m.get("cover_ref")) {
            if mode == "cleared" {
                None
            } else {
                val.clone()
            }
        } else if let Some(ref bc) = base_cover {
            let is_url = bc.starts_with("http://")
                || bc.starts_with("https://")
                || bc.starts_with("data:");
            if online_cover.is_none() || (!is_url && Path::new(bc).is_file()) {
                Some(bc.clone())
            } else if let Some(oc) = online_cover {
                Some(oc.to_string())
            } else {
                Some(bc.clone())
            }
        } else {
            online_cover.map(|s| s.to_string())
        };
        let cover_ref = resolve_cover_for_ui(raw_cover_ref);

        let voice_actors: Vec<String> =
            if let Some((mode, val)) = ov_map.and_then(|m| m.get("voice_actors")) {
                if mode == "cleared" {
                    Vec::new()
                } else {
                    val.as_deref()
                        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                        .unwrap_or_default()
                }
            } else if let Some(arr) = online_meta
                .and_then(|m| m.get("voice_actors_list"))
                .and_then(|v| v.as_array())
                .filter(|a| !a.is_empty())
            {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else if let Some(arr) = online_meta
                .and_then(|m| m.get("vas"))
                .and_then(|v| v.as_array())
                .filter(|a| !a.is_empty())
            {
                arr.iter()
                    .filter_map(|item| {
                        item.get("name")
                            .and_then(|n| n.as_str())
                            .or_else(|| item.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect()
            } else {
                serde_json::from_str::<Vec<String>>(base_vas_json).unwrap_or_default()
            };

        let tags: Vec<String> = if let Some((mode, val)) = ov_map.and_then(|m| m.get("tags")) {
            if mode == "cleared" {
                Vec::new()
            } else {
                val.as_deref()
                    .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                    .unwrap_or_default()
            }
        } else if let Some(arr) = online_meta
            .and_then(|m| m.get("tags_list"))
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
        {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        } else if let Some(arr) = online_meta
            .and_then(|m| m.get("tags"))
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
        {
            arr.iter()
                .filter_map(|item| {
                    item.get("i18n")
                        .and_then(|i| i.get("zh-cn"))
                        .and_then(|z| z.get("name"))
                        .and_then(|n| n.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| item.get("name").and_then(|n| n.as_str()))
                        .or_else(|| item.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        } else {
            serde_json::from_str::<Vec<String>>(base_tags_json).unwrap_or_default()
        };

        (
            title,
            description,
            circle_name,
            cover_ref,
            voice_actors,
            tags,
        )
    }

    pub fn get_effective_work_fields(
        conn: &rusqlite::Connection,
        work_id: &str,
        base_title: &str,
        base_desc: &str,
        base_circle: &str,
        base_cover: &Option<String>,
        base_vas_json: &str,
        base_tags_json: &str,
    ) -> AppResult<(
        String,
        String,
        String,
        Option<String>,
        Vec<String>,
        Vec<String>,
        Vec<WorkOverrideDto>,
    )> {
        let mut overrides = Vec::new();
        let mut ov_map: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
        {
            let mut stmt = conn.prepare_cached(
                "SELECT field, override_mode, value_text FROM work_overrides WHERE work_id = ?1",
            )?;
            let rows = stmt.query_map(params![work_id], |r| {
                Ok(WorkOverrideDto {
                    field: r.get(0)?,
                    override_mode: r.get(1)?,
                    value_text: r.get(2)?,
                })
            })?;
            for row in rows {
                let dto = row?;
                ov_map.insert(
                    dto.field.clone(),
                    (dto.override_mode.clone(), dto.value_text.clone()),
                );
                overrides.push(dto);
            }
        }

        let online_meta: Option<Value> = conn
            .query_row(
                "SELECT raw_metadata_json FROM work_sources WHERE work_id = ?1 ORDER BY refreshed_at DESC LIMIT 1",
                params![work_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .and_then(|s| serde_json::from_str(&s).ok());

        let (title, description, circle_name, cover_ref, voice_actors, tags) =
            Self::resolve_effective_work_fields_from_data(
                Some(&ov_map),
                online_meta.as_ref(),
                base_title,
                base_desc,
                base_circle,
                base_cover,
                base_vas_json,
                base_tags_json,
            );

        Ok((
            title,
            description,
            circle_name,
            cover_ref,
            voice_actors,
            tags,
            overrides,
        ))
    }

    pub fn set_work_override(
        &self,
        work_id: &str,
        field: &str,
        override_mode: &str,
        value_text: Option<&str>,
    ) -> AppResult<()> {
        if override_mode != "set" && override_mode != "cleared" {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "override_mode 必须为 set 或 cleared",
            ));
        }
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO work_overrides (work_id, field, override_mode, value_text, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(work_id, field) DO UPDATE SET
                   override_mode = excluded.override_mode,
                   value_text = excluded.value_text,
                   updated_at = excluded.updated_at",
                params![work_id, field, override_mode, value_text, now],
            )?;
            conn.execute(
                "UPDATE works SET updated_at = ?1 WHERE id = ?2",
                params![now, work_id],
            )?;
            Ok(())
        })
    }

    pub fn reset_work_override(&self, work_id: &str, field: &str) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM work_overrides WHERE work_id = ?1 AND field = ?2",
                params![work_id, field],
            )?;
            conn.execute(
                "UPDATE works SET updated_at = ?1 WHERE id = ?2",
                params![now, work_id],
            )?;
            Ok(())
        })
    }

    pub fn toggle_favorite(&self, work_id: &str) -> AppResult<bool> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            let exists: Option<String> = conn
                .query_row(
                    "SELECT work_id FROM favorites WHERE work_id = ?1",
                    params![work_id],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_some() {
                conn.execute("DELETE FROM favorites WHERE work_id = ?1", params![work_id])?;
                Ok(false)
            } else {
                conn.execute(
                    "INSERT INTO favorites (work_id, created_at) VALUES (?1, ?2)",
                    params![work_id, now],
                )?;
                Ok(true)
            }
        })
    }

    pub fn toggle_collected(&self, work_id: &str) -> AppResult<bool> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            let exists: Option<String> = conn
                .query_row(
                    "SELECT work_id FROM collected_works WHERE work_id = ?1",
                    params![work_id],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_some() {
                conn.execute(
                    "DELETE FROM collected_works WHERE work_id = ?1",
                    params![work_id],
                )?;
                Ok(false)
            } else {
                conn.execute(
                    "INSERT INTO collected_works (work_id, created_at) VALUES (?1, ?2)",
                    params![work_id, now],
                )?;
                Ok(true)
            }
        })
    }

    /// Indexes parsed subtitle text into `tracks_fts` so full-text search can find works by subtitle cues (T902).
    pub fn index_subtitle_text_for_work(
        &self,
        work_id: &str,
        track_id: Option<&str>,
        subtitle_text: &str,
    ) -> AppResult<()> {
        let cleaned = subtitle_text.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        self.with_conn(|conn| {
            if let Some(tid) = track_id {
                conn.execute(
                    "UPDATE tracks_fts SET subtitle_text = ?1 WHERE track_id = ?2 AND work_id = ?3",
                    params![cleaned, tid, work_id],
                )?;
            } else {
                // Attach subtitle text to the first track of the work in tracks_fts, or insert a virtual work-level row
                let first_tid: Option<String> = conn
                    .query_row(
                        "SELECT id FROM tracks WHERE work_id = ?1 ORDER BY sort_key ASC LIMIT 1",
                        params![work_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(tid) = first_tid {
                    conn.execute(
                        "UPDATE tracks_fts SET subtitle_text = ?1 WHERE track_id = ?2",
                        params![cleaned, tid],
                    )?;
                } else {
                    conn.execute(
                        "INSERT INTO tracks_fts (track_id, work_id, title, logical_dir_path, subtitle_text)
                         VALUES (?1, ?2, '', '', ?3)",
                        params![format!("sub_{}", work_id), work_id, cleaned],
                    )?;
                }
            }
            Ok(())
        })
    }

    pub fn list_works(&self, filter: &str, search: Option<&str>) -> AppResult<Vec<WorkSummary>> {
        self.with_conn(|conn| {
            // 1. Preload all user overrides in a single query (T901: eliminates N queries)
            let mut all_overrides: HashMap<String, BTreeMap<String, (String, Option<String>)>> =
                HashMap::new();
            {
                let mut ov_stmt = conn.prepare_cached(
                    "SELECT work_id, field, override_mode, value_text FROM work_overrides",
                )?;
                let ov_rows = ov_stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                })?;
                for row in ov_rows {
                    let (w_id, field, mode, val) = row?;
                    all_overrides
                        .entry(w_id)
                        .or_default()
                        .insert(field, (mode, val));
                }
            }

            // 2. If search query is provided, query FTS5 virtual tables works_fts & tracks_fts (T902)
            let q_trimmed = search.map(|s| s.trim()).unwrap_or_default();
            let q_lower = q_trimmed.to_lowercase();
            let mut fts_matched_work_ids: HashSet<String> = HashSet::new();

            if !q_lower.is_empty() {
                let fts_tokens: Vec<String> = q_trimmed
                    .split_whitespace()
                    .map(|tok| {
                        tok.chars()
                            .filter(|c| *c != '"' && *c != '\'' && !c.is_control())
                            .collect::<String>()
                    })
                    .filter(|tok| !tok.is_empty())
                    .map(|tok| format!("\"{}\"*", tok))
                    .collect();

                if !fts_tokens.is_empty() {
                    let fts_query = fts_tokens.join(" AND ");
                    if let Ok(mut fts_stmt) = conn.prepare_cached(
                        "SELECT work_id FROM works_fts WHERE works_fts MATCH ?1
                         UNION
                         SELECT work_id FROM tracks_fts WHERE tracks_fts MATCH ?1",
                    ) {
                        if let Ok(rows) =
                            fts_stmt.query_map(params![&fts_query], |r| r.get::<_, String>(0))
                        {
                            for wid in rows.flatten() {
                                fts_matched_work_ids.insert(wid);
                            }
                        }
                    }
                }

                // Fallback substring match on tracks_fts for mid-token CJK queries in track titles or subtitles
                if fts_matched_work_ids.is_empty() {
                    let like_pat = format!("%{}%", q_trimmed);
                    if let Ok(mut tr_like_stmt) = conn.prepare_cached(
                        "SELECT DISTINCT work_id FROM tracks_fts
                         WHERE title LIKE ?1 OR logical_dir_path LIKE ?1 OR subtitle_text LIKE ?1",
                    ) {
                        if let Ok(rows) =
                            tr_like_stmt.query_map(params![&like_pat], |r| r.get::<_, String>(0))
                        {
                            for wid in rows.flatten() {
                                fts_matched_work_ids.insert(wid);
                            }
                        }
                    }
                }
            }

            // 3. Single CTE query aggregating media_sources & latest work_sources (T901: eliminates 2N queries)
            let mut stmt = conn.prepare_cached(
                "WITH src_agg AS (
                    SELECT t.work_id,
                           MAX(CASE WHEN ms.source_kind IN ('local_imported', 'local_downloaded') THEN 1 ELSE 0 END) AS has_local,
                           MAX(CASE WHEN ms.source_kind = 'local_downloaded' THEN 1 ELSE 0 END) AS has_downloaded,
                           MAX(CASE WHEN ms.source_kind = 'online_provider' THEN 1 ELSE 0 END) AS has_online_src,
                           MAX(CASE WHEN ms.availability = 'available' THEN 1 ELSE 0 END) AS any_available,
                           MAX(CASE WHEN ms.availability = 'offline_disk' THEN 1 ELSE 0 END) AS any_offline_disk
                    FROM tracks t
                    JOIN track_versions tv ON tv.track_id = t.id
                    JOIN media_sources ms ON ms.version_id = tv.id
                    GROUP BY t.work_id
                 ),
                 latest_ws AS (
                    SELECT ws.work_id, ws.source_work_id, ws.raw_metadata_json
                    FROM work_sources ws
                    WHERE ws.id = (
                        SELECT ws2.id FROM work_sources ws2
                        WHERE ws2.work_id = ws.work_id
                        ORDER BY ws2.refreshed_at DESC
                        LIMIT 1
                    )
                 )
                 SELECT w.id, w.title, w.description, w.rj_code, w.circle_name,
                        w.voice_actors_json, w.tags_json, w.cover_ref, w.updated_at,
                        CASE WHEN f.work_id IS NOT NULL THEN 1 ELSE 0 END AS is_fav,
                        wps.last_version_id, wps.last_played_at,
                        pp.position_ms,
                        t_last.title AS last_track_title,
                        lws.source_work_id,
                        lws.raw_metadata_json,
                        COALESCE(sa.has_local, 0),
                        COALESCE(sa.has_downloaded, 0),
                        COALESCE(sa.has_online_src, 0),
                        COALESCE(sa.any_available, 0),
                        COALESCE(sa.any_offline_disk, 0),
                        CASE WHEN c.work_id IS NOT NULL THEN 1 ELSE 0 END AS is_col
                 FROM works w
                 LEFT JOIN favorites f ON f.work_id = w.id
                 LEFT JOIN collected_works c ON c.work_id = w.id
                 LEFT JOIN work_playback_state wps ON wps.work_id = w.id
                 LEFT JOIN playback_positions pp ON pp.version_id = wps.last_version_id
                 LEFT JOIN track_versions tv_last ON tv_last.id = wps.last_version_id
                 LEFT JOIN tracks t_last ON t_last.id = tv_last.track_id
                 LEFT JOIN latest_ws lws ON lws.work_id = w.id
                 LEFT JOIN src_agg sa ON sa.work_id = w.id
                 ORDER BY CASE WHEN ?1 = 'recent' THEN wps.last_played_at ELSE w.updated_at END DESC",
            )?;

            let rows = stmt.query_map(params![filter], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)? == 1,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, Option<String>>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                    r.get::<_, Option<String>>(13)?,
                    r.get::<_, Option<String>>(14)?,
                    r.get::<_, Option<String>>(15)?,
                    r.get::<_, i64>(16)? == 1,
                    r.get::<_, i64>(17)? == 1,
                    r.get::<_, i64>(18)? == 1,
                    r.get::<_, i64>(19)? == 1,
                    r.get::<_, i64>(20)? == 1,
                    r.get::<_, i64>(21)? == 1,
                ))
            })?;

            let mut results = Vec::new();

            for row in rows {
                let (
                    id,
                    base_title,
                    base_desc,
                    rj_code,
                    base_circle,
                    vas_json,
                    tags_json,
                    base_cover,
                    updated_at,
                    is_favorite,
                    last_version_id,
                    last_played_at,
                    last_position_ms,
                    last_track_title,
                    source_work_id,
                    raw_metadata_json,
                    has_local,
                    has_downloaded,
                    has_online_src,
                    any_available,
                    any_offline_disk,
                    is_collected,
                ) = row?;

                if filter == "favorites" && !is_favorite {
                    continue;
                }
                if filter == "recent" && last_played_at.is_none() {
                    continue;
                }
                if (filter == "all" || filter == "local" || filter == "online") && !is_collected {
                    continue;
                }

                let has_online = source_work_id.is_some() || has_online_src;
                if filter == "local" && !has_local {
                    continue;
                }
                if filter == "online" && !has_online {
                    continue;
                }

                let ov_map = all_overrides.get(&id);
                let online_meta: Option<Value> = raw_metadata_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());

                let (title, _desc, circle_name, cover_ref, voice_actors, tags) =
                    Self::resolve_effective_work_fields_from_data(
                        ov_map,
                        online_meta.as_ref(),
                        &base_title,
                        &base_desc,
                        &base_circle,
                        &base_cover,
                        &vas_json,
                        &tags_json,
                    );

                // T1119: When nsfw_enabled is false (default OFF), filter out works flagged NSFW or tagged R18
                let nsfw_enabled: bool = conn
                    .query_row(
                        "SELECT value_json FROM app_settings WHERE key = 'nsfw_enabled'",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false);

                if !nsfw_enabled {
                    let is_nsfw_meta = online_meta
                        .as_ref()
                        .and_then(|m| m.get("nsfw"))
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    let is_nsfw_tag = tags.iter().any(|t| {
                        let u = t.trim().to_ascii_uppercase();
                        u == "R18" || u == "R-18" || u == "NSFW" || u == "成人向"
                    });
                    if is_nsfw_meta || is_nsfw_tag {
                        continue;
                    }
                }

                if !q_lower.is_empty() {
                    let matched = fts_matched_work_ids.contains(&id)
                        || title.to_lowercase().contains(&q_lower)
                        || rj_code
                            .as_deref()
                            .map(|c| c.to_lowercase().contains(&q_lower))
                            .unwrap_or(false)
                        || circle_name.to_lowercase().contains(&q_lower)
                        || voice_actors
                            .iter()
                            .any(|v| v.to_lowercase().contains(&q_lower))
                        || tags.iter().any(|t| t.to_lowercase().contains(&q_lower));
                    if !matched {
                        continue;
                    }
                }

                let availability = if any_available {
                    "available".to_string()
                } else if any_offline_disk {
                    "offline_disk".to_string()
                } else {
                    "missing".to_string()
                };

                results.push(WorkSummary {
                    id,
                    title,
                    rj_code,
                    circle_name,
                    voice_actors,
                    tags,
                    cover_ref,
                    has_local,
                    has_online,
                    has_downloaded,
                    availability,
                    is_collected,
                    is_favorite,
                    last_version_id,
                    last_track_title,
                    last_position_ms,
                    last_played_at,
                    source_work_id,
                    updated_at,
                });
            }

            Ok(results)
        })
    }

    /// Resolves work metadata and the raw cover file path (or remote URL if not cached on disk yet)
    /// specifically for Windows SMTC (`SystemMediaTransportControlsDisplayUpdater`), never converting
    /// local file paths into `data:image/...;base64,...` strings.
    pub fn get_work_smtc_metadata(
        &self,
        work_id: &str,
    ) -> AppResult<(
        String,
        Option<String>,
        String,
        Vec<String>,
        Option<String>,
        Option<String>,
    )> {
        let cache_covers_dir = self.cache_dir.join("covers");
        self.with_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT title, description, rj_code, circle_name, voice_actors_json, tags_json, cover_ref
                     FROM works WHERE id = ?1",
                    params![work_id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, Option<String>>(6)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| {
                    AppError::new(ErrorCode::SourceNotFound, format!("未找到作品: {}", work_id))
                })?;

            let (base_title, base_desc, rj_code, base_circle, vas_json, tags_json, base_cover) =
                row;

            let (title, _desc, circle_name, _ui_cover, voice_actors, _tags, overrides) =
                Self::get_effective_work_fields(
                    conn,
                    work_id,
                    &base_title,
                    &base_desc,
                    &base_circle,
                    &base_cover,
                    &vas_json,
                    &tags_json,
                )?;

            let ws_row: Option<(String, String)> = conn
                .query_row(
                    "SELECT source_work_id, raw_metadata_json
                     FROM work_sources
                     WHERE work_id = ?1
                     ORDER BY refreshed_at DESC LIMIT 1",
                    params![work_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;

            let source_work_id = ws_row.as_ref().map(|(sid, _)| sid.clone());
            let online_meta: Option<Value> = ws_row
                .as_ref()
                .and_then(|(_, raw)| serde_json::from_str(raw).ok());

            // 1. Check user override on cover_ref first
            if let Some(ov) = overrides.iter().find(|o| o.field == "cover_ref") {
                if ov.override_mode == "cleared" {
                    return Ok((title, rj_code, circle_name, voice_actors, None, source_work_id));
                }
                if let Some(ref val) = ov.value_text {
                    let trimmed = val.trim();
                    if !trimmed.is_empty() {
                        return Ok((
                            title,
                            rj_code,
                            circle_name,
                            voice_actors,
                            Some(trimmed.to_string()),
                            source_work_id,
                        ));
                    }
                }
            }

            // 2. Prefer an existing local file on disk:
            //    (a) base_cover if it is a valid local file path
            //    (b) cached_cover_path in work_sources.raw_metadata_json
            //    (c) cache_dir/covers/asmr_{source_work_id}.jpg
            //    (d) attachments WHERE attachment_type = 'cover' AND absolute_path IS NOT NULL
            let mut candidate_local_paths: Vec<String> = Vec::new();
            if let Some(ref bc) = base_cover {
                let t = bc.trim();
                if !t.is_empty()
                    && !t.starts_with("http://")
                    && !t.starts_with("https://")
                    && !t.starts_with("data:")
                {
                    candidate_local_paths.push(t.to_string());
                }
            }
            if let Some(cached_p) = online_meta
                .as_ref()
                .and_then(|m| m.get("cached_cover_path"))
                .and_then(|v| v.as_str())
            {
                candidate_local_paths.push(cached_p.trim().to_string());
            }
            if let Some(ref sid) = source_work_id {
                let p = cache_covers_dir.join(format!("asmr_{}.jpg", sid));
                candidate_local_paths.push(
                    p.to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string(),
                );
            }
            if let Ok(Some(att_cover)) = conn
                .query_row(
                    "SELECT absolute_path FROM attachments
                     WHERE work_id = ?1 AND attachment_type = 'cover' AND absolute_path IS NOT NULL
                     ORDER BY created_at ASC LIMIT 1",
                    params![work_id],
                    |r| r.get::<_, String>(0),
                )
                .optional()
            {
                candidate_local_paths.push(att_cover);
            }

            for p in candidate_local_paths {
                let path_obj = Path::new(&p);
                if let Ok(meta) = fs::metadata(path_obj) {
                    if meta.is_file() && meta.len() > 32 {
                        return Ok((
                            title,
                            rj_code,
                            circle_name,
                            voice_actors,
                            Some(p),
                            source_work_id,
                        ));
                    }
                }
            }

            // 3. Fallback to remote HTTP(S) cover URL if local file isn't cached yet
            let remote_url = online_meta
                .as_ref()
                .and_then(|m| {
                    m.get("mainCoverUrl")
                        .or_else(|| m.get("cover_url"))
                        .or_else(|| m.get("thumbnailCoverUrl"))
                        .or_else(|| m.get("samCoverUrl"))
                })
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .or_else(|| {
                    base_cover
                        .as_ref()
                        .map(|s| s.trim().to_string())
                        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                });

            Ok((
                title,
                rj_code,
                circle_name,
                voice_actors,
                remote_url,
                source_work_id,
            ))
        })
    }

    pub fn get_work_detail(&self, work_id: &str) -> AppResult<WorkDetailDto> {
        self.with_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT id, title, description, rj_code, circle_name, voice_actors_json, tags_json, cover_ref,
                            (SELECT 1 FROM favorites f WHERE f.work_id = works.id) AS is_fav,
                            (SELECT 1 FROM collected_works c WHERE c.work_id = works.id) AS is_col
                     FROM works WHERE id = ?1",
                    params![work_id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, Option<String>>(7)?,
                            r.get::<_, Option<i64>>(8)?.unwrap_or(0) == 1,
                            r.get::<_, Option<i64>>(9)?.unwrap_or(0) == 1,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| {
                    AppError::new(ErrorCode::SourceNotFound, format!("未找到作品: {}", work_id))
                })?;

            let (
                id,
                base_title,
                base_desc,
                rj_code,
                base_circle,
                vas_json,
                tags_json,
                base_cover,
                is_favorite,
                is_collected,
            ) = row;

            let (title, description, circle_name, cover_ref, voice_actors, tags, overrides) =
                Self::get_effective_work_fields(
                    conn,
                    &id,
                    &base_title,
                    &base_desc,
                    &base_circle,
                    &base_cover,
                    &vas_json,
                    &tags_json,
                )?;

            let mut online_sources = Vec::new();
            {
                let mut ws_stmt = conn.prepare(
                    "SELECT id, provider, source_work_id, sync_status, refreshed_at, expires_at
                     FROM work_sources WHERE work_id = ?1",
                )?;
                let ws_rows = ws_stmt.query_map(params![work_id], |r| {
                    let expires_at: Option<String> = r.get(5)?;
                    let is_expired = expires_at
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc) <= Utc::now())
                        .unwrap_or(false);
                    Ok(WorkSourceInfoDto {
                        id: r.get(0)?,
                        provider: r.get(1)?,
                        source_work_id: r.get(2)?,
                        sync_status: r.get(3)?,
                        refreshed_at: r.get(4)?,
                        expires_at,
                        is_expired,
                    })
                })?;
                for ws in ws_rows {
                    online_sources.push(ws?);
                }
            }

            let mut tracks = Vec::new();
            let mut dir_groups_map: BTreeMap<String, (usize, std::collections::BTreeSet<String>)> =
                BTreeMap::new();

            {
                let mut t_stmt = conn.prepare(
                    "SELECT id, work_id, title, logical_dir_path, group_label, sort_key
                     FROM tracks WHERE work_id = ?1 ORDER BY sort_key ASC",
                )?;
                let t_rows = t_stmt.query_map(params![work_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                })?;

                for tr in t_rows {
                    let (t_id, w_id, t_title, logical_dir_path, group_label, sort_key) = tr?;

                    let mut v_stmt = conn.prepare(
                        "SELECT tv.id, tv.track_id, tv.version_label, tv.duration_ms, tv.content_fingerprint, tv.association_state,
                                COALESCE(pp.position_ms, 0), COALESCE(pp.completed, 0)
                         FROM track_versions tv
                         LEFT JOIN playback_positions pp ON pp.version_id = tv.id
                         WHERE tv.track_id = ?1
                         ORDER BY tv.created_at ASC",
                    )?;
                    let v_rows = v_stmt.query_map(params![&t_id], |vr| {
                        Ok((
                            vr.get::<_, String>(0)?,
                            vr.get::<_, String>(1)?,
                            vr.get::<_, String>(2)?,
                            vr.get::<_, Option<i64>>(3)?,
                            vr.get::<_, Option<String>>(4)?,
                            vr.get::<_, String>(5)?,
                            vr.get::<_, i64>(6)?,
                            vr.get::<_, i64>(7)? == 1,
                        ))
                    })?;

                    let mut versions = Vec::new();
                    for vr in v_rows {
                        let (
                            v_id,
                            tr_id,
                            version_label,
                            duration_ms,
                            content_fingerprint,
                            association_state,
                            position_ms,
                            completed,
                        ) = vr?;

                        let mut ms_stmt = conn.prepare(
                            "SELECT id, version_id, source_kind, provider, source_work_id, remote_file_id,
                                    remote_relative_path, root_id, relative_path, absolute_path, format,
                                    byte_size, content_sha256, availability
                             FROM media_sources WHERE version_id = ?1
                             ORDER BY CASE source_kind
                               WHEN 'local_imported' THEN 1
                               WHEN 'local_downloaded' THEN 2
                               ELSE 3 END ASC",
                        )?;
                        let ms_rows = ms_stmt.query_map(params![&v_id], |mr| {
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

                        let mut sources = Vec::new();
                        for ms in ms_rows {
                            let s = ms?;
                            let entry = dir_groups_map
                                .entry(logical_dir_path.clone())
                                .or_insert_with(|| (0, std::collections::BTreeSet::new()));
                            entry.1.insert(s.format.to_uppercase());
                            sources.push(s);
                        }

                        versions.push(TrackVersionDto {
                            id: v_id,
                            track_id: tr_id,
                            version_label,
                            duration_ms,
                            content_fingerprint,
                            association_state,
                            position_ms,
                            completed,
                            sources,
                        });
                    }

                    let entry = dir_groups_map
                        .entry(logical_dir_path.clone())
                        .or_insert_with(|| (0, std::collections::BTreeSet::new()));
                    entry.0 += 1;

                    tracks.push(TrackItemDto {
                        id: t_id,
                        work_id: w_id,
                        title: t_title,
                        logical_dir_path,
                        group_label,
                        sort_key,
                        versions,
                    });
                }
            }

            let directory_groups = dir_groups_map
                .into_iter()
                .map(|(group_path, (track_count, formats_set))| {
                    let label = if group_path.is_empty() {
                        "根目录".to_string()
                    } else {
                        group_path.clone()
                    };
                    DirectoryGroupOption {
                        group_path,
                        label,
                        track_count,
                        formats: formats_set.into_iter().collect(),
                    }
                })
                .collect();

            let mut attachments = Vec::new();
            {
                let mut a_stmt = conn.prepare(
                    "SELECT id, work_id, version_id, attachment_type, title, relative_path,
                            absolute_path, remote_file_id, subtitle_offset_ms, encoding, availability
                     FROM attachments WHERE work_id = ?1 ORDER BY title ASC",
                )?;
                let a_rows = a_stmt.query_map(params![work_id], |r| {
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
                })?;
                for a in a_rows {
                    attachments.push(a?);
                }
            }

            let last_state: Option<(String, Option<i64>)> = conn
                .query_row(
                    "SELECT wps.last_version_id, pp.position_ms
                     FROM work_playback_state wps
                     LEFT JOIN playback_positions pp ON pp.version_id = wps.last_version_id
                     WHERE wps.work_id = ?1",
                    params![work_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;

            Ok(WorkDetailDto {
                id,
                title,
                description,
                rj_code,
                circle_name,
                voice_actors,
                tags,
                cover_ref,
                is_collected,
                is_favorite,
                overrides,
                online_sources,
                tracks,
                attachments,
                directory_groups,
                last_version_id: last_state.as_ref().map(|s| s.0.clone()),
                last_position_ms: last_state.and_then(|s| s.1),
            })
        })
    }
}

impl Database {
    pub fn save_playback_position(
        &self,
        version_id: &str,
        position_ms: i64,
        completed: bool,
        update_work_recent: bool,
    ) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        let pos = position_ms.max(0);
        let comp_int = if completed { 1 } else { 0 };
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO playback_positions (version_id, position_ms, completed, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(version_id) DO UPDATE SET
                   position_ms = excluded.position_ms,
                   completed = excluded.completed,
                   updated_at = excluded.updated_at",
                params![version_id, pos, comp_int, now],
            )?;

            if update_work_recent {
                if let Some(work_id) = conn
                    .query_row(
                        "SELECT t.work_id FROM track_versions tv
                         JOIN tracks t ON t.id = tv.track_id
                         WHERE tv.id = ?1",
                        params![version_id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
                {
                    conn.execute(
                        "INSERT INTO work_playback_state (work_id, last_version_id, last_played_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(work_id) DO UPDATE SET
                           last_version_id = excluded.last_version_id,
                           last_played_at = excluded.last_played_at,
                           updated_at = excluded.updated_at",
                        params![work_id, version_id, now, now],
                    )?;
                }
            }
            Ok(())
        })
    }

    pub fn get_playback_position(&self, version_id: &str) -> AppResult<(i64, bool)> {
        self.with_conn(|conn| {
            let row: Option<(i64, i64)> = conn
                .query_row(
                    "SELECT position_ms, completed FROM playback_positions WHERE version_id = ?1",
                    params![version_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            Ok(row.map(|(p, c)| (p, c == 1)).unwrap_or((0, false)))
        })
    }

    pub fn get_queue_items(&self) -> AppResult<Vec<QueueItemDto>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT qi.id, qi.sort_order, qi.version_id, t.id, w.id, w.title, t.title,
                        t.logical_dir_path, tv.duration_ms,
                        COALESCE(pp.position_ms, 0), COALESCE(pp.completed, 0),
                        qi.preferred_source_id,
                        EXISTS(
                            SELECT 1 FROM media_sources ms
                            WHERE ms.version_id = qi.version_id
                              AND ms.availability = 'available'
                              AND ms.source_kind IN ('local_imported', 'local_downloaded')
                        ) AS has_local,
                        EXISTS(
                            SELECT 1 FROM media_sources ms
                            WHERE ms.version_id = qi.version_id
                              AND ms.availability = 'available'
                              AND ms.source_kind = 'online_provider'
                        ) AS has_online
                 FROM queue_items qi
                 JOIN track_versions tv ON tv.id = qi.version_id
                 JOIN tracks t ON t.id = tv.track_id
                 JOIN works w ON w.id = t.work_id
                 LEFT JOIN playback_positions pp ON pp.version_id = tv.id
                 ORDER BY qi.sort_order ASC",
            )?;

            let rows = stmt.query_map([], |r| {
                Ok(QueueItemDto {
                    id: r.get(0)?,
                    sort_order: r.get(1)?,
                    version_id: r.get(2)?,
                    track_id: r.get(3)?,
                    work_id: r.get(4)?,
                    work_title: r.get(5)?,
                    track_title: r.get(6)?,
                    logical_dir_path: r.get(7)?,
                    duration_ms: r.get(8)?,
                    position_ms: r.get(9)?,
                    completed: r.get::<_, i64>(10)? == 1,
                    preferred_source_id: r.get(11)?,
                    has_local: r.get::<_, i64>(12)? == 1,
                    has_online: r.get::<_, i64>(13)? == 1,
                })
            })?;

            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn replace_queue_items(
        &self,
        items: &[(String, Option<String>)],
        active_index: usize,
    ) -> AppResult<Option<String>> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE player_session SET current_queue_item_id = NULL WHERE id = 1",
                [],
            )?;
            tx.execute("DELETE FROM queue_items", [])?;

            let mut active_id = None;
            for (idx, (version_id, pref_src)) in items.iter().enumerate() {
                let qid = Uuid::new_v4().to_string();
                if idx == active_index {
                    active_id = Some(qid.clone());
                }
                tx.execute(
                    "INSERT INTO queue_items (id, sort_order, version_id, preferred_source_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![qid, idx as i64, version_id, pref_src, now],
                )?;
            }

            tx.execute(
                "UPDATE player_session SET current_queue_item_id = ?1, updated_at = ?2 WHERE id = 1",
                params![active_id, now],
            )?;
            tx.commit()?;
            Ok(active_id)
        })
    }

    pub fn append_queue_items(&self, items: &[(String, Option<String>)]) -> AppResult<Vec<String>> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let max_order: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(sort_order), -1) FROM queue_items",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(-1);
            let mut ids = Vec::new();
            for (offset, (version_id, pref_src)) in items.iter().enumerate() {
                let qid = Uuid::new_v4().to_string();
                let order = max_order + 1 + (offset as i64);
                tx.execute(
                    "INSERT INTO queue_items (id, sort_order, version_id, preferred_source_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![qid, order, version_id, pref_src, now],
                )?;
                ids.push(qid);
            }
            tx.commit()?;
            Ok(ids)
        })
    }

    pub fn remove_queue_item(&self, queue_item_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM queue_items WHERE id = ?1",
                params![queue_item_id],
            )?;
            Ok(())
        })
    }

    pub fn move_queue_item(&self, queue_item_id: &str, target_index: usize) -> AppResult<()> {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let mut ids: Vec<String> = {
                let mut stmt =
                    tx.prepare("SELECT id FROM queue_items ORDER BY sort_order ASC")?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            if let Some(pos) = ids.iter().position(|id| id == queue_item_id) {
                let item = ids.remove(pos);
                let dest = target_index.min(ids.len());
                ids.insert(dest, item);
                for (idx, id) in ids.iter().enumerate() {
                    tx.execute(
                        "UPDATE queue_items SET sort_order = ?1 WHERE id = ?2",
                        params![idx as i64, id],
                    )?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    pub fn get_player_session_row(&self) -> AppResult<(Option<String>, f64, String, u64)> {
        self.with_conn(|conn| {
            let row = conn.query_row(
                "SELECT current_queue_item_id, volume, loop_mode, session_generation
                 FROM player_session WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)? as u64,
                    ))
                },
            )?;
            Ok(row)
        })
    }

    pub fn update_player_session_row(
        &self,
        current_queue_item_id: Option<&str>,
        volume: f64,
        loop_mode: &str,
        session_generation: u64,
    ) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE player_session
                 SET current_queue_item_id = ?1, volume = ?2, loop_mode = ?3,
                     session_generation = ?4, updated_at = ?5
                 WHERE id = 1",
                params![
                    current_queue_item_id,
                    volume.clamp(0.0, 200.0),
                    loop_mode,
                    session_generation as i64,
                    now
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_bookmarks(&self, work_id: Option<&str>, version_id: Option<&str>) -> AppResult<Vec<BookmarkDto>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT b.id, b.version_id, t.title, t.work_id, b.position_ms, b.note, b.created_at
                 FROM bookmarks b
                 JOIN track_versions tv ON tv.id = b.version_id
                 JOIN tracks t ON t.id = tv.track_id
                 WHERE (?1 IS NULL OR t.work_id = ?1)
                   AND (?2 IS NULL OR b.version_id = ?2)
                 ORDER BY b.position_ms ASC",
            )?;
            let rows = stmt.query_map(params![work_id, version_id], |r| {
                Ok(BookmarkDto {
                    id: r.get(0)?,
                    version_id: r.get(1)?,
                    track_title: r.get(2)?,
                    work_id: r.get(3)?,
                    position_ms: r.get(4)?,
                    note: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn add_bookmark(&self, version_id: &str, position_ms: i64, note: &str) -> AppResult<BookmarkDto> {
        if position_ms < 0 {
            return Err(AppError::new(ErrorCode::InvalidInput, "书签位置不能为负数"));
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            let (track_title, work_id): (String, String) = conn
                .query_row(
                    "SELECT t.title, t.work_id FROM track_versions tv
                     JOIN tracks t ON t.id = tv.track_id WHERE tv.id = ?1",
                    params![version_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到对应音轨版本"))?;

            conn.execute(
                "INSERT INTO bookmarks (id, version_id, position_ms, note, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, version_id, position_ms, note, now, now],
            )?;
            Ok(BookmarkDto {
                id,
                version_id: version_id.to_string(),
                track_title,
                work_id,
                position_ms,
                note: note.to_string(),
                created_at: now,
            })
        })
    }

    pub fn update_bookmark(&self, bookmark_id: &str, note: &str) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE bookmarks SET note = ?1, updated_at = ?2 WHERE id = ?3",
                params![note, now, bookmark_id],
            )?;
            Ok(())
        })
    }

    pub fn delete_bookmark(&self, bookmark_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![bookmark_id])?;
            Ok(())
        })
    }

    pub fn list_ab_loops(&self, version_id: &str) -> AppResult<Vec<AbLoopDto>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, version_id, start_ms, end_ms, label
                 FROM ab_loops WHERE version_id = ?1 ORDER BY start_ms ASC",
            )?;
            let rows = stmt.query_map(params![version_id], |r| {
                Ok(AbLoopDto {
                    id: r.get(0)?,
                    version_id: r.get(1)?,
                    start_ms: r.get(2)?,
                    end_ms: r.get(3)?,
                    label: r.get(4)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn save_ab_loop(
        &self,
        version_id: &str,
        start_ms: i64,
        end_ms: i64,
        label: &str,
    ) -> AppResult<AbLoopDto> {
        self.with_conn(|conn| {
            let duration_opt: Option<i64> = conn
                .query_row(
                    "SELECT duration_ms FROM track_versions WHERE id = ?1",
                    params![version_id],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到对应版本"))?;

            if start_ms < 0 || start_ms >= end_ms {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "A-B 循环必须满足 0 <= A < B",
                ));
            }
            if let Some(dur) = duration_opt {
                if dur > 0 && end_ms > dur {
                    return Err(AppError::new(
                        ErrorCode::InvalidInput,
                        format!("B 点 ({}ms) 不能超过音轨时长 ({}ms)", end_ms, dur),
                    ));
                }
            }

            let id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO ab_loops (id, version_id, start_ms, end_ms, label, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, version_id, start_ms, end_ms, label, now],
            )?;
            Ok(AbLoopDto {
                id,
                version_id: version_id.to_string(),
                start_ms,
                end_ms,
                label: label.to_string(),
            })
        })
    }

    pub fn delete_ab_loop(&self, loop_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM ab_loops WHERE id = ?1", params![loop_id])?;
            Ok(())
        })
    }

    pub fn clear_queue_items(&self) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE player_session SET current_queue_item_id = NULL, updated_at = ?1 WHERE id = 1",
                params![now],
            )?;
            tx.execute("DELETE FROM queue_items", [])?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Removes a work and its cascaded records from SQLite without touching any `local_imported` user files.
    /// Also unmounts any `single_work` library_roots that have no remaining `library_entries`.
    pub fn delete_work(&self, work_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM works WHERE id = ?1", params![work_id])?;
            tx.execute(
                "DELETE FROM library_roots
                 WHERE root_mode = 'single_work'
                   AND NOT EXISTS (SELECT 1 FROM library_entries le WHERE le.root_id = library_roots.id)",
                [],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    /// T405/T407: Associates an online work (`online_work_id`) into a local work (`local_work_id`)
    /// without merging track timelines, recording a reversible snapshot in `association_history`.
    pub fn associate_works(
        &self,
        local_work_id: &str,
        online_work_id: &str,
    ) -> AppResult<String> {
        if local_work_id == online_work_id {
            return Err(AppError::new(ErrorCode::InvalidInput, "同一作品无需关联"));
        }
        let now = Utc::now().to_rfc3339();
        let history_id = Uuid::new_v4().to_string();

        self.with_conn(|conn| {
            let tx = conn.transaction()?;

            let online_work_row: Option<(String, String, Option<String>, String, String, String, Option<String>)> = tx
                .query_row(
                    "SELECT title, description, rj_code, circle_name, voice_actors_json, tags_json, cover_ref
                     FROM works WHERE id = ?1",
                    params![online_work_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
                )
                .optional()?;

            let (o_title, o_desc, o_rj, o_circle, o_vas, o_tags, o_cover) = online_work_row
                .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到待关联的在线作品"))?;

            let moved_sources: Vec<String> = {
                let mut s = tx.prepare("SELECT id FROM work_sources WHERE work_id = ?1")?;
                let r = s.query_map(params![online_work_id], |row| row.get(0))?;
                r.collect::<Result<Vec<_>, _>>()?
            };
            let moved_tracks: Vec<String> = {
                let mut s = tx.prepare("SELECT id FROM tracks WHERE work_id = ?1")?;
                let r = s.query_map(params![online_work_id], |row| row.get(0))?;
                r.collect::<Result<Vec<_>, _>>()?
            };
            let online_was_fav: bool = tx
                .query_row(
                    "SELECT 1 FROM favorites WHERE work_id = ?1",
                    params![online_work_id],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);

            let snapshot = serde_json::json!({
                "online_work": {
                    "id": online_work_id,
                    "title": o_title,
                    "description": o_desc,
                    "rj_code": o_rj,
                    "circle_name": o_circle,
                    "voice_actors_json": o_vas,
                    "tags_json": o_tags,
                    "cover_ref": o_cover,
                    "was_favorite": online_was_fav,
                },
                "moved_sources": moved_sources,
                "moved_tracks": moved_tracks,
            });

            tx.execute(
                "UPDATE work_sources SET work_id = ?1 WHERE work_id = ?2",
                params![local_work_id, online_work_id],
            )?;
            // Keep tracks on separate timelines! Move tracks to local_work_id without merging version_ids
            tx.execute(
                "UPDATE tracks SET work_id = ?1 WHERE work_id = ?2",
                params![local_work_id, online_work_id],
            )?;
            if online_was_fav {
                tx.execute(
                    "INSERT OR IGNORE INTO favorites (work_id, created_at) VALUES (?1, ?2)",
                    params![local_work_id, now],
                )?;
            }
            if let Some(ref rj) = o_rj {
                tx.execute(
                    "UPDATE works SET rj_code = COALESCE(rj_code, ?1), updated_at = ?2 WHERE id = ?3",
                    params![rj, now, local_work_id],
                )?;
            }
            tx.execute("DELETE FROM works WHERE id = ?1", params![online_work_id])?;

            tx.execute(
                "INSERT INTO association_history (id, entity_type, primary_id, secondary_id, snapshot_json, conflict_note, undone, created_at)
                 VALUES (?1, 'work', ?2, ?3, ?4, ?5, 0, ?6)",
                params![
                    history_id,
                    local_work_id,
                    online_work_id,
                    snapshot.to_string(),
                    "作品关联（音轨时间轴保持独立）",
                    now
                ],
            )?;

            tx.commit()?;
            Ok(history_id)
        })
    }

    /// T406/T407: Associates two track versions (e.g. confirmed same version between local and online),
    /// preserving the newer playback position on conflict and recording both positions for undo.
    pub fn associate_versions(
        &self,
        target_version_id: &str,
        source_version_id: &str,
    ) -> AppResult<String> {
        if target_version_id == source_version_id {
            return Err(AppError::new(ErrorCode::InvalidInput, "同一版本无需合并"));
        }
        let now = Utc::now().to_rfc3339();
        let history_id = Uuid::new_v4().to_string();

        self.with_conn(|conn| {
            let tx = conn.transaction()?;

            let src_ver_row: Option<(String, String, Option<i64>, Option<String>)> = tx
                .query_row(
                    "SELECT track_id, version_label, duration_ms, content_fingerprint
                     FROM track_versions WHERE id = ?1",
                    params![source_version_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            let (src_track_id, src_label, src_dur, src_fp) = src_ver_row
                .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到源版本"))?;

            let tgt_pos: Option<(i64, i64, String)> = tx
                .query_row(
                    "SELECT position_ms, completed, updated_at FROM playback_positions WHERE version_id = ?1",
                    params![target_version_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let src_pos: Option<(i64, i64, String)> = tx
                .query_row(
                    "SELECT position_ms, completed, updated_at FROM playback_positions WHERE version_id = ?1",
                    params![source_version_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;

            let moved_media_ids: Vec<String> = {
                let mut s = tx.prepare("SELECT id FROM media_sources WHERE version_id = ?1")?;
                let r = s.query_map(params![source_version_id], |row| row.get(0))?;
                r.collect::<Result<Vec<_>, _>>()?
            };
            let moved_bookmark_ids: Vec<String> = {
                let mut s = tx.prepare("SELECT id FROM bookmarks WHERE version_id = ?1")?;
                let r = s.query_map(params![source_version_id], |row| row.get(0))?;
                r.collect::<Result<Vec<_>, _>>()?
            };

            let conflict_note = match (&tgt_pos, &src_pos) {
                (Some((tp, _, tt)), Some((sp, _, st))) => Some(format!(
                    "冲突进度记录：目标版本 {}ms ({}) vs 源版本 {}ms ({})，已保留较新记录",
                    tp, tt, sp, st
                )),
                _ => None,
            };

            // Keep newer playback position
            if let Some((sp, sc, st)) = &src_pos {
                let use_src = match &tgt_pos {
                    Some((_, _, tt)) => st > tt,
                    None => true,
                };
                if use_src {
                    tx.execute(
                        "INSERT INTO playback_positions (version_id, position_ms, completed, updated_at)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(version_id) DO UPDATE SET
                           position_ms = excluded.position_ms,
                           completed = excluded.completed,
                           updated_at = excluded.updated_at",
                        params![target_version_id, sp, sc, st],
                    )?;
                }
            }

            let snapshot = serde_json::json!({
                "source_version": {
                    "id": source_version_id,
                    "track_id": src_track_id,
                    "version_label": src_label,
                    "duration_ms": src_dur,
                    "content_fingerprint": src_fp,
                },
                "target_position": tgt_pos,
                "source_position": src_pos,
                "moved_media_ids": moved_media_ids,
                "moved_bookmark_ids": moved_bookmark_ids,
            });

            tx.execute(
                "UPDATE media_sources SET version_id = ?1 WHERE version_id = ?2",
                params![target_version_id, source_version_id],
            )?;
            tx.execute(
                "UPDATE bookmarks SET version_id = ?1 WHERE version_id = ?2",
                params![target_version_id, source_version_id],
            )?;
            tx.execute(
                "DELETE FROM track_versions WHERE id = ?1",
                params![source_version_id],
            )?;

            tx.execute(
                "INSERT INTO association_history (id, entity_type, primary_id, secondary_id, snapshot_json, conflict_note, undone, created_at)
                 VALUES (?1, 'version', ?2, ?3, ?4, ?5, 0, ?6)",
                params![
                    history_id,
                    target_version_id,
                    source_version_id,
                    snapshot.to_string(),
                    conflict_note,
                    now
                ],
            )?;

            tx.commit()?;
            Ok(history_id)
        })
    }

    pub fn undo_association(&self, history_id: &str) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let row: Option<(String, String, String, String, i64)> = tx
                .query_row(
                    "SELECT entity_type, primary_id, secondary_id, snapshot_json, undone
                     FROM association_history WHERE id = ?1",
                    params![history_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?;

            let (entity_type, primary_id, secondary_id, snap_str, undone) = row
                .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到关联历史记录"))?;
            if undone == 1 {
                return Err(AppError::new(ErrorCode::InvalidInput, "该关联记录已撤销"));
            }

            let snap: Value = serde_json::from_str(&snap_str)
                .map_err(|e| AppError::new(ErrorCode::InternalError, e.to_string()))?;

            if entity_type == "work" {
                let ow = &snap["online_work"];
                tx.execute(
                    "INSERT INTO works (id, title, description, rj_code, circle_name, voice_actors_json, tags_json, cover_ref, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        secondary_id,
                        ow["title"].as_str().unwrap_or("Restored Work"),
                        ow["description"].as_str().unwrap_or(""),
                        ow["rj_code"].as_str(),
                        ow["circle_name"].as_str().unwrap_or(""),
                        ow["voice_actors_json"].as_str().unwrap_or("[]"),
                        ow["tags_json"].as_str().unwrap_or("[]"),
                        ow["cover_ref"].as_str(),
                        now,
                        now
                    ],
                )?;
                if let Some(arr) = snap["moved_sources"].as_array() {
                    for sid in arr.iter().filter_map(|v| v.as_str()) {
                        tx.execute(
                            "UPDATE work_sources SET work_id = ?1 WHERE id = ?2",
                            params![secondary_id, sid],
                        )?;
                    }
                }
                if let Some(arr) = snap["moved_tracks"].as_array() {
                    for tid in arr.iter().filter_map(|v| v.as_str()) {
                        tx.execute(
                            "UPDATE tracks SET work_id = ?1 WHERE id = ?2",
                            params![secondary_id, tid],
                        )?;
                    }
                }
                if ow["was_favorite"].as_bool() == Some(true) {
                    tx.execute(
                        "INSERT OR IGNORE INTO favorites (work_id, created_at) VALUES (?1, ?2)",
                        params![secondary_id, now],
                    )?;
                }
            } else if entity_type == "version" {
                let sv = &snap["source_version"];
                tx.execute(
                    "INSERT INTO track_versions (id, track_id, version_label, duration_ms, content_fingerprint, association_state, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'confirmed', ?6)",
                    params![
                        secondary_id,
                        sv["track_id"].as_str().unwrap_or(""),
                        sv["version_label"].as_str().unwrap_or("default"),
                        sv["duration_ms"].as_i64(),
                        sv["content_fingerprint"].as_str(),
                        now
                    ],
                )?;
                if let Some(arr) = snap["moved_media_ids"].as_array() {
                    for mid in arr.iter().filter_map(|v| v.as_str()) {
                        tx.execute(
                            "UPDATE media_sources SET version_id = ?1 WHERE id = ?2",
                            params![secondary_id, mid],
                        )?;
                    }
                }
                if let Some(arr) = snap["moved_bookmark_ids"].as_array() {
                    for bid in arr.iter().filter_map(|v| v.as_str()) {
                        tx.execute(
                            "UPDATE bookmarks SET version_id = ?1 WHERE id = ?2",
                            params![secondary_id, bid],
                        )?;
                    }
                }
                if let Some(tp) = snap["target_position"].as_array() {
                    if tp.len() == 3 {
                        tx.execute(
                            "INSERT OR REPLACE INTO playback_positions (version_id, position_ms, completed, updated_at)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![
                                primary_id,
                                tp[0].as_i64().unwrap_or(0),
                                tp[1].as_i64().unwrap_or(0),
                                tp[2].as_str().unwrap_or(&now)
                            ],
                        )?;
                    }
                }
                if let Some(sp) = snap["source_position"].as_array() {
                    if sp.len() == 3 {
                        tx.execute(
                            "INSERT OR REPLACE INTO playback_positions (version_id, position_ms, completed, updated_at)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![
                                secondary_id,
                                sp[0].as_i64().unwrap_or(0),
                                sp[1].as_i64().unwrap_or(0),
                                sp[2].as_str().unwrap_or(&now)
                            ],
                        )?;
                    }
                }
            }

            tx.execute(
                "UPDATE association_history SET undone = 1 WHERE id = ?1",
                params![history_id],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn list_association_history(&self) -> AppResult<Vec<AssociationHistoryDto>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, entity_type, primary_id, secondary_id, conflict_note, undone, created_at
                 FROM association_history ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(AssociationHistoryDto {
                    id: r.get(0)?,
                    entity_type: r.get(1)?,
                    primary_id: r.get(2)?,
                    secondary_id: r.get(3)?,
                    conflict_note: r.get(4)?,
                    undone: r.get::<_, i64>(5)? == 1,
                    created_at: r.get(6)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    /// T801: Loads persisted global DSP config and active preset ID (defaults to `asmr_pure_guard`).
    pub fn get_dsp_global_state(&self) -> AppResult<(DspConfigDto, Option<String>)> {
        self.with_conn(|conn| {
            let row: Option<(Option<String>, String)> = conn
                .query_row(
                    "SELECT active_preset_id, config_json FROM dsp_global_state WHERE id = 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((preset_id, json_str)) = row {
                if let Ok(cfg) = serde_json::from_str::<DspConfigDto>(&json_str) {
                    return Ok((cfg, preset_id));
                }
            }
            Ok((DspConfigDto::default(), Some("asmr_pure_guard".into())))
        })
    }

    pub fn save_dsp_global_state(
        &self,
        cfg: &DspConfigDto,
        active_preset_id: Option<&str>,
    ) -> AppResult<()> {
        let json_str = serde_json::to_string(cfg).map_err(|e| {
            AppError::new(ErrorCode::InternalError, format!("序列化 DSP 配置失败: {}", e))
        })?;
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO dsp_global_state (id, active_preset_id, config_json, updated_at)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                   active_preset_id = excluded.active_preset_id,
                   config_json = excluded.config_json,
                   updated_at = excluded.updated_at",
                params![active_preset_id, json_str, now],
            )?;
            Ok(())
        })
    }

    pub fn list_dsp_presets(&self) -> AppResult<Vec<DspPresetDto>> {
        let mut presets = builtin_dsp_presets();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, description, config_json
                 FROM dsp_presets
                 WHERE is_builtin = 0
                 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            for row in rows {
                let (id, name, description, json_str) = row?;
                if let Ok(config) = serde_json::from_str::<DspConfigDto>(&json_str) {
                    presets.push(DspPresetDto {
                        id,
                        name,
                        description,
                        config,
                        is_builtin: false,
                    });
                }
            }
            Ok(())
        })?;
        Ok(presets)
    }

    pub fn save_custom_dsp_preset(
        &self,
        id: Option<&str>,
        name: &str,
        description: &str,
        cfg: &DspConfigDto,
    ) -> AppResult<DspPresetDto> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::new(ErrorCode::InvalidInput, "预设名称不能为空"));
        }
        let preset_id = id
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("custom_{}", Uuid::new_v4()));
        if builtin_dsp_presets().iter().any(|p| p.id == preset_id) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "不能覆盖内置声学预设，请另存为新预设名称",
            ));
        }
        let json_str = serde_json::to_string(cfg).map_err(|e| {
            AppError::new(ErrorCode::InternalError, format!("序列化预设失败: {}", e))
        })?;
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO dsp_presets (id, name, description, config_json, is_builtin, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   description = excluded.description,
                   config_json = excluded.config_json,
                   updated_at = excluded.updated_at",
                params![preset_id, trimmed, description.trim(), json_str, now],
            )?;
            Ok(())
        })?;
        Ok(DspPresetDto {
            id: preset_id,
            name: trimmed.to_string(),
            description: description.trim().to_string(),
            config: cfg.clone(),
            is_builtin: false,
        })
    }

    pub fn delete_custom_dsp_preset(&self, preset_id: &str) -> AppResult<()> {
        if builtin_dsp_presets().iter().any(|p| p.id == preset_id) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "内置声学预设不可删除",
            ));
        }
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM dsp_presets WHERE id = ?1 AND is_builtin = 0",
                params![preset_id],
            )?;
            Ok(())
        })
    }

    pub fn get_work_dsp_override(
        &self,
        work_id: &str,
    ) -> AppResult<Option<(Option<String>, DspConfigDto, bool)>> {
        self.with_conn(|conn| {
            let row: Option<(Option<String>, String, i64)> = conn
                .query_row(
                    "SELECT preset_id, config_json, enabled FROM work_dsp_overrides WHERE work_id = ?1",
                    params![work_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            if let Some((preset_id, json_str, enabled)) = row {
                if let Ok(cfg) = serde_json::from_str::<DspConfigDto>(&json_str) {
                    return Ok(Some((preset_id, cfg, enabled == 1)));
                }
            }
            Ok(None)
        })
    }

    pub fn set_work_dsp_override(
        &self,
        work_id: &str,
        preset_id: Option<&str>,
        cfg: &DspConfigDto,
        enabled: bool,
    ) -> AppResult<()> {
        let json_str = serde_json::to_string(cfg).map_err(|e| {
            AppError::new(ErrorCode::InternalError, format!("序列化作品音效覆盖失败: {}", e))
        })?;
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO work_dsp_overrides (work_id, preset_id, config_json, enabled, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(work_id) DO UPDATE SET
                   preset_id = excluded.preset_id,
                   config_json = excluded.config_json,
                   enabled = excluded.enabled,
                   updated_at = excluded.updated_at",
                params![work_id, preset_id, json_str, if enabled { 1 } else { 0 }, now],
            )?;
            Ok(())
        })
    }

    pub fn clear_work_dsp_override(&self, work_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM work_dsp_overrides WHERE work_id = ?1",
                params![work_id],
            )?;
            Ok(())
        })
    }

    /// T1006: Exports all user-created assets (favorites, playback positions, bookmarks, A-B loops,
    /// metadata overrides, custom DSP presets, and per-work DSP overrides) into a portable JSON bundle.
    pub fn export_user_backup(&self) -> AppResult<UserBackupBundleDto> {
        let (dsp_global_config, dsp_global_preset_id) = self
            .get_dsp_global_state()
            .unwrap_or_else(|_| (DspConfigDto::default(), Some("asmr_pure_guard".into())));
        let custom_dsp_presets: Vec<DspPresetDto> = self
            .list_dsp_presets()?
            .into_iter()
            .filter(|p| !p.is_builtin)
            .collect();

        self.with_conn(|conn| {
            let favorites = {
                let mut stmt = conn.prepare(
                    "SELECT f.work_id, w.rj_code, w.title, f.created_at
                     FROM favorites f
                     JOIN works w ON w.id = f.work_id
                     ORDER BY f.created_at ASC",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok(BackupFavoriteRecordDto {
                        work_id: r.get(0)?,
                        rj_code: r.get(1)?,
                        work_title: r.get(2)?,
                        created_at: r.get(3)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            let playback_positions = {
                let mut stmt = conn.prepare(
                    "SELECT pp.version_id, w.rj_code, t.title, tv.version_label,
                            pp.position_ms, pp.completed, pp.updated_at
                     FROM playback_positions pp
                     JOIN track_versions tv ON tv.id = pp.version_id
                     JOIN tracks t ON t.id = tv.track_id
                     JOIN works w ON w.id = t.work_id
                     ORDER BY pp.updated_at ASC",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok(BackupPositionRecordDto {
                        version_id: r.get(0)?,
                        rj_code: r.get(1)?,
                        track_title: r.get(2)?,
                        version_label: r.get(3)?,
                        position_ms: r.get(4)?,
                        completed: r.get::<_, i64>(5)? == 1,
                        updated_at: r.get(6)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            let bookmarks = {
                let mut stmt = conn.prepare(
                    "SELECT b.id, b.version_id, w.rj_code, t.title, b.position_ms, b.note, b.created_at
                     FROM bookmarks b
                     JOIN track_versions tv ON tv.id = b.version_id
                     JOIN tracks t ON t.id = tv.track_id
                     JOIN works w ON w.id = t.work_id
                     ORDER BY b.created_at ASC",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok(BackupBookmarkRecordDto {
                        id: r.get(0)?,
                        version_id: r.get(1)?,
                        rj_code: r.get(2)?,
                        track_title: r.get(3)?,
                        position_ms: r.get(4)?,
                        note: r.get(5)?,
                        created_at: r.get(6)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            let ab_loops = {
                let mut stmt = conn.prepare(
                    "SELECT ab.id, ab.version_id, w.rj_code, t.title, ab.start_ms, ab.end_ms, ab.label, ab.created_at
                     FROM ab_loops ab
                     JOIN track_versions tv ON tv.id = ab.version_id
                     JOIN tracks t ON t.id = tv.track_id
                     JOIN works w ON w.id = t.work_id
                     ORDER BY ab.created_at ASC",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok(BackupAbLoopRecordDto {
                        id: r.get(0)?,
                        version_id: r.get(1)?,
                        rj_code: r.get(2)?,
                        track_title: r.get(3)?,
                        start_ms: r.get(4)?,
                        end_ms: r.get(5)?,
                        label: r.get(6)?,
                        created_at: r.get(7)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            let work_overrides = {
                let mut stmt = conn.prepare(
                    "SELECT wo.work_id, w.rj_code, wo.field, wo.override_mode, wo.value_text, wo.updated_at
                     FROM work_overrides wo
                     JOIN works w ON w.id = wo.work_id
                     ORDER BY wo.updated_at ASC",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok(BackupWorkOverrideRecordDto {
                        work_id: r.get(0)?,
                        rj_code: r.get(1)?,
                        field: r.get(2)?,
                        override_mode: r.get(3)?,
                        value_text: r.get(4)?,
                        updated_at: r.get(5)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            let work_dsp_overrides = {
                let mut stmt = conn.prepare(
                    "SELECT wdo.work_id, w.rj_code, wdo.preset_id, wdo.config_json, wdo.enabled, wdo.updated_at
                     FROM work_dsp_overrides wdo
                     JOIN works w ON w.id = wdo.work_id
                     ORDER BY wdo.updated_at ASC",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)? == 1,
                        r.get::<_, String>(5)?,
                    ))
                })?;
                let mut out = Vec::new();
                for row in rows {
                    let (work_id, rj_code, preset_id, cfg_json, enabled, updated_at) = row?;
                    if let Ok(config) = serde_json::from_str::<DspConfigDto>(&cfg_json) {
                        out.push(BackupWorkDspOverrideRecordDto {
                            work_id,
                            rj_code,
                            preset_id,
                            config,
                            enabled,
                            updated_at,
                        });
                    }
                }
                out
            };

            Ok(UserBackupBundleDto {
                schema_version: 1,
                exported_at: Utc::now().to_rfc3339(),
                favorites,
                playback_positions,
                bookmarks,
                ab_loops,
                work_overrides,
                custom_dsp_presets,
                work_dsp_overrides,
                dsp_global_config,
                dsp_global_preset_id,
            })
        })
    }

    /// T1006: Idempotently restores user assets from a `UserBackupBundleDto`, matching works/versions by
    /// exact ID first and falling back to `(rj_code, track_title)` so cross-machine / re-imported libraries work seamlessly.
    pub fn import_user_backup(&self, bundle: &UserBackupBundleDto) -> AppResult<BackupRestoreReportDto> {
        let mut report = BackupRestoreReportDto {
            favorites_restored: 0,
            positions_restored: 0,
            bookmarks_restored: 0,
            ab_loops_restored: 0,
            overrides_restored: 0,
            dsp_presets_restored: 0,
        };

        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let now = Utc::now().to_rfc3339();

            let resolve_work_id = |tx: &rusqlite::Transaction<'_>,
                                   work_id: &str,
                                   rj_code: Option<&str>|
             -> Option<String> {
                if let Ok(Some(id)) = tx
                    .query_row("SELECT id FROM works WHERE id = ?1", params![work_id], |r| {
                        r.get::<_, String>(0)
                    })
                    .optional()
                {
                    return Some(id);
                }
                if let Some(rj) = rj_code.filter(|s| !s.trim().is_empty()) {
                    if let Ok(Some(id)) = tx
                        .query_row(
                            "SELECT id FROM works WHERE rj_code = ?1 ORDER BY updated_at DESC LIMIT 1",
                            params![rj],
                            |r| r.get::<_, String>(0),
                        )
                        .optional()
                    {
                        return Some(id);
                    }
                }
                None
            };

            let resolve_version_id = |tx: &rusqlite::Transaction<'_>,
                                      version_id: &str,
                                      rj_code: Option<&str>,
                                      track_title: &str|
             -> Option<String> {
                if let Ok(Some(id)) = tx
                    .query_row(
                        "SELECT id FROM track_versions WHERE id = ?1",
                        params![version_id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()
                {
                    return Some(id);
                }
                if let Some(rj) = rj_code.filter(|s| !s.trim().is_empty()) {
                    if let Ok(Some(id)) = tx
                        .query_row(
                            "SELECT tv.id
                             FROM track_versions tv
                             JOIN tracks t ON t.id = tv.track_id
                             JOIN works w ON w.id = t.work_id
                             WHERE w.rj_code = ?1 AND t.title = ?2
                             ORDER BY tv.created_at ASC LIMIT 1",
                            params![rj, track_title],
                            |r| r.get::<_, String>(0),
                        )
                        .optional()
                    {
                        return Some(id);
                    }
                }
                None
            };

            // 1. Custom DSP presets
            for preset in &bundle.custom_dsp_presets {
                if preset.is_builtin || preset.name.trim().is_empty() {
                    continue;
                }
                if let Ok(cfg_json) = serde_json::to_string(&preset.config) {
                    tx.execute(
                        "INSERT INTO dsp_presets (id, name, description, config_json, is_builtin, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)
                         ON CONFLICT(id) DO UPDATE SET
                           name = excluded.name,
                           description = excluded.description,
                           config_json = excluded.config_json,
                           updated_at = excluded.updated_at",
                        params![preset.id, preset.name.trim(), preset.description.trim(), cfg_json, now],
                    )?;
                    report.dsp_presets_restored += 1;
                }
            }

            // 2. Global DSP state
            if let Ok(cfg_json) = serde_json::to_string(&bundle.dsp_global_config) {
                tx.execute(
                    "INSERT INTO dsp_global_state (id, active_preset_id, config_json, updated_at)
                     VALUES (1, ?1, ?2, ?3)
                     ON CONFLICT(id) DO UPDATE SET
                       active_preset_id = excluded.active_preset_id,
                       config_json = excluded.config_json,
                       updated_at = excluded.updated_at",
                    params![bundle.dsp_global_preset_id, cfg_json, now],
                )?;
            }

            // 3. Favorites
            for fav in &bundle.favorites {
                if let Some(wid) = resolve_work_id(&tx, &fav.work_id, fav.rj_code.as_deref()) {
                    tx.execute(
                        "INSERT INTO favorites (work_id, created_at)
                         VALUES (?1, ?2)
                         ON CONFLICT(work_id) DO NOTHING",
                        params![wid, fav.created_at],
                    )?;
                    report.favorites_restored += 1;
                }
            }

            // 4. Playback positions (idempotent: keep newer timestamp on conflict)
            for pos in &bundle.playback_positions {
                if let Some(vid) =
                    resolve_version_id(&tx, &pos.version_id, pos.rj_code.as_deref(), &pos.track_title)
                {
                    tx.execute(
                        "INSERT INTO playback_positions (version_id, position_ms, completed, updated_at)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(version_id) DO UPDATE SET
                           position_ms = CASE WHEN excluded.updated_at >= playback_positions.updated_at THEN excluded.position_ms ELSE playback_positions.position_ms END,
                           completed = CASE WHEN excluded.updated_at >= playback_positions.updated_at THEN excluded.completed ELSE playback_positions.completed END,
                           updated_at = CASE WHEN excluded.updated_at >= playback_positions.updated_at THEN excluded.updated_at ELSE playback_positions.updated_at END",
                        params![vid, pos.position_ms.max(0), if pos.completed { 1 } else { 0 }, pos.updated_at],
                    )?;
                    report.positions_restored += 1;
                }
            }

            // 5. Bookmarks
            for bm in &bundle.bookmarks {
                if let Some(vid) =
                    resolve_version_id(&tx, &bm.version_id, bm.rj_code.as_deref(), &bm.track_title)
                {
                    tx.execute(
                        "INSERT INTO bookmarks (id, version_id, position_ms, note, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(id) DO UPDATE SET
                           position_ms = excluded.position_ms,
                           note = excluded.note,
                           updated_at = excluded.updated_at",
                        params![bm.id, vid, bm.position_ms.max(0), bm.note, bm.created_at, now],
                    )?;
                    report.bookmarks_restored += 1;
                }
            }

            // 6. A-B Loops
            for ab in &bundle.ab_loops {
                if ab.start_ms < 0 || ab.start_ms >= ab.end_ms {
                    continue;
                }
                if let Some(vid) =
                    resolve_version_id(&tx, &ab.version_id, ab.rj_code.as_deref(), &ab.track_title)
                {
                    tx.execute(
                        "INSERT INTO ab_loops (id, version_id, start_ms, end_ms, label, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(id) DO UPDATE SET
                           start_ms = excluded.start_ms,
                           end_ms = excluded.end_ms,
                           label = excluded.label",
                        params![ab.id, vid, ab.start_ms, ab.end_ms, ab.label, ab.created_at],
                    )?;
                    report.ab_loops_restored += 1;
                }
            }

            // 7. Work metadata overrides
            for ov in &bundle.work_overrides {
                if ov.override_mode != "set" && ov.override_mode != "cleared" {
                    continue;
                }
                if let Some(wid) = resolve_work_id(&tx, &ov.work_id, ov.rj_code.as_deref()) {
                    tx.execute(
                        "INSERT INTO work_overrides (work_id, field, override_mode, value_text, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(work_id, field) DO UPDATE SET
                           override_mode = excluded.override_mode,
                           value_text = excluded.value_text,
                           updated_at = excluded.updated_at",
                        params![wid, ov.field, ov.override_mode, ov.value_text, ov.updated_at],
                    )?;
                    report.overrides_restored += 1;
                }
            }

            // 8. Per-work DSP overrides
            for wdo in &bundle.work_dsp_overrides {
                if let Some(wid) = resolve_work_id(&tx, &wdo.work_id, wdo.rj_code.as_deref()) {
                    if let Ok(cfg_json) = serde_json::to_string(&wdo.config) {
                        tx.execute(
                            "INSERT INTO work_dsp_overrides (work_id, preset_id, config_json, enabled, updated_at)
                             VALUES (?1, ?2, ?3, ?4, ?5)
                             ON CONFLICT(work_id) DO UPDATE SET
                               preset_id = excluded.preset_id,
                               config_json = excluded.config_json,
                               enabled = excluded.enabled,
                               updated_at = excluded.updated_at",
                            params![wid, wdo.preset_id, cfg_json, if wdo.enabled { 1 } else { 0 }, wdo.updated_at],
                        )?;
                        report.overrides_restored += 1;
                    }
                }
            }

            tx.commit()?;
            Ok(report)
        })
    }
}

#[cfg(test)]
mod repo_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_override_priority_and_online_refresh_preservation_r09() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, description, circle_name, created_at, updated_at)
                 VALUES ('w1', 'Local Folder Name', 'Local Desc', 'Local Circle', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO work_sources (id, work_id, provider, source_work_id, raw_metadata_json, refreshed_at)
                 VALUES ('ws1', 'w1', 'asmr_one', '1705068', '{\"title\":\"Online Title v1\",\"circle_name\":\"Online Circle\"}', '2026-01-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        // Without user override, online title wins over local fallback
        let detail1 = db.get_work_detail("w1").unwrap();
        assert_eq!(detail1.title, "Online Title v1");
        assert_eq!(detail1.circle_name, "Online Circle");

        // User sets title override and explicitly clears circle_name
        db.set_work_override("w1", "title", "set", Some("My Custom Title"))
            .unwrap();
        db.set_work_override("w1", "circle_name", "cleared", None)
            .unwrap();

        // Simulate online metadata refresh (R09)
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE work_sources SET raw_metadata_json = '{\"title\":\"Online Title v2\",\"circle_name\":\"New Online Circle\"}' WHERE id = 'ws1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let detail2 = db.get_work_detail("w1").unwrap();
        assert_eq!(detail2.title, "My Custom Title", "User override must survive online refresh");
        assert_eq!(detail2.circle_name, "", "Explicitly cleared field must stay empty");

        // Reset title override -> falls back to refreshed online title v2
        db.reset_work_override("w1", "title").unwrap();
        let detail3 = db.get_work_detail("w1").unwrap();
        assert_eq!(detail3.title, "Online Title v2");
    }

    #[test]
    fn test_version_association_conflict_and_undo_r09() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w1', 'Work 1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at) VALUES ('t1', 'w1', 'Track 1', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v_local', 't1', 'local', 100000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at) VALUES ('v_online', 't1', 'online', 100000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, format, availability, created_at, updated_at)
                 VALUES ('ms_local', 'v_local', 'local_imported', 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_sources (id, version_id, source_kind, provider, source_work_id, remote_file_id, format, availability, created_at, updated_at)
                 VALUES ('ms_online', 'v_online', 'online_provider', 'asmr_one', '1705068', '1705068/1', 'mp3', 'available', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO playback_positions (version_id, position_ms, completed, updated_at)
                 VALUES ('v_local', 12000, 0, '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO playback_positions (version_id, position_ms, completed, updated_at)
                 VALUES ('v_online', 45000, 0, '2026-01-02T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let hist_id = db.associate_versions("v_local", "v_online").unwrap();
        let (pos_merged, _) = db.get_playback_position("v_local").unwrap();
        assert_eq!(pos_merged, 45000, "Newer position must win on version association");

        // Undo association -> restores both versions and their original positions & sources
        db.undo_association(&hist_id).unwrap();
        let (pos_local, _) = db.get_playback_position("v_local").unwrap();
        let (pos_online, _) = db.get_playback_position("v_online").unwrap();
        assert_eq!(pos_local, 12000);
        assert_eq!(pos_online, 45000);
    }

    #[test]
    fn test_user_backup_export_and_idempotent_restore_t1006() {
        let dir = tempdir().unwrap();
        let db1 = Database::open_and_migrate(&dir.path().join("d1"), &dir.path().join("c1")).unwrap();

        db1.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, rj_code, created_at, updated_at)
                 VALUES ('w_orig', 'Backup Test Work', 'RJ01999999', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at)
                 VALUES ('t_orig', 'w_orig', '01_ear_care.wav', '001', '2026-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v_orig', 't_orig', 'WAV', 180000, '2026-01-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        db1.toggle_favorite("w_orig").unwrap();
        db1.save_playback_position("v_orig", 64_500, false, true)
            .unwrap();
        db1.add_bookmark("v_orig", 30_000, "轻柔掏耳起点").unwrap();
        db1.save_ab_loop("v_orig", 10_000, 40_000, "助眠循环段")
            .unwrap();
        db1.set_work_override("w_orig", "title", "set", Some("自定义备份标题"))
            .unwrap();
        db1.save_custom_dsp_preset(None, "我的夜间专属EQ", "自定义测试预设", &DspConfigDto::default())
            .unwrap();

        let bundle = db1.export_user_backup().unwrap();
        assert_eq!(bundle.favorites.len(), 1);
        assert_eq!(bundle.playback_positions.len(), 1);
        assert_eq!(bundle.bookmarks.len(), 1);
        assert_eq!(bundle.ab_loops.len(), 1);
        assert_eq!(bundle.work_overrides.len(), 1);
        assert_eq!(bundle.custom_dsp_presets.len(), 1);

        // Restore into a second fresh database where the work was re-imported with a DIFFERENT work_id/version_id
        // but the same RJ code and track title!
        let db2 = Database::open_and_migrate(&dir.path().join("d2"), &dir.path().join("c2")).unwrap();
        db2.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, rj_code, created_at, updated_at)
                 VALUES ('w_new_id', 'Raw Scanned Title', 'RJ01999999', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tracks (id, work_id, title, sort_key, created_at)
                 VALUES ('t_new_id', 'w_new_id', '01_ear_care.wav', '001', '2026-02-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO track_versions (id, track_id, version_label, duration_ms, created_at)
                 VALUES ('v_new_id', 't_new_id', 'WAV', 180000, '2026-02-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let rep1 = db2.import_user_backup(&bundle).unwrap();
        assert_eq!(rep1.favorites_restored, 1);
        assert_eq!(rep1.positions_restored, 1);
        assert_eq!(rep1.bookmarks_restored, 1);
        assert_eq!(rep1.ab_loops_restored, 1);
        assert_eq!(rep1.overrides_restored, 1);
        assert_eq!(rep1.dsp_presets_restored, 1);

        // Calling import_user_backup a second time must be 100% idempotent without duplicating records
        let _rep2 = db2.import_user_backup(&bundle).unwrap();
        let detail = db2.get_work_detail("w_new_id").unwrap();
        assert!(detail.is_favorite);
        assert_eq!(detail.title, "自定义备份标题");
        assert_eq!(detail.tracks[0].versions[0].position_ms, 64_500);
        assert_eq!(db2.list_bookmarks(Some("w_new_id"), None).unwrap().len(), 1);
        assert_eq!(db2.list_ab_loops("v_new_id").unwrap().len(), 1);
    }
}
