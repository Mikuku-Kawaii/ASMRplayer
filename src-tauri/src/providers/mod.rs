use crate::db::Database;
use crate::domain::*;
use chrono::{Duration as ChronoDuration, Utc};
use parking_lot::RwLock;
use reqwest::{header, StatusCode};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const PROVIDER_ASMR_ONE: &str = "asmr_one";
pub const DEFAULT_API_BASE: &str = "https://api.asmr-200.com";
const KEYRING_SERVICE: &str = "com.asmrplayer.desktop";
const KEYRING_ACCOUNT: &str = "asmr_one_auth_token";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaPurpose {
    Play,
    Download,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMediaRef {
    pub provider: String,
    pub source_work_id: String,
    pub remote_file_id: String,
    pub remote_relative_path: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedMedia {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub byte_size: Option<i64>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderWork {
    pub source_work_id: String,
    pub rj_code: Option<String>,
    pub title: String,
    pub circle_name: String,
    pub voice_actors: Vec<String>,
    pub tags: Vec<String>,
    pub cover_url: Option<String>,
    pub duration_sec: Option<i64>,
    pub has_subtitle: bool,
    #[serde(default)]
    pub nsfw: bool,
    pub release_date: Option<String>,
    #[serde(default)]
    pub dl_count: Option<i64>,
    #[serde(default)]
    pub rate_average_2dp: Option<f64>,
    #[serde(default)]
    pub review_count: Option<i64>,
    #[serde(default)]
    pub price: Option<i64>,
    pub raw_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLeafNode {
    pub node_type: String, // "audio" | "image" | "text" | "subtitle" | "other"
    pub title: String,
    pub logical_dir_path: String,
    pub relative_path: String,
    pub remote_file_id: String,
    pub format: String,
    pub duration_ms: Option<i64>,
    pub byte_size: Option<i64>,
    pub stream_url: Option<String>,
    pub download_url: Option<String>,
}

/// Normalizes user input (`"RJ01705068"`, `"01705068"`, `"1705068"`) into canonical numeric `source_work_id` (`"1705068"`).
pub fn normalize_work_id_input(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidInput, "请输入作品 ID 或 RJ 编号"));
    }
    let without_prefix = if trimmed.len() > 2 && trimmed[..2].eq_ignore_ascii_case("rj") {
        &trimmed[2..]
    } else {
        trimmed
    };
    if !without_prefix.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            format!("无效的作品编号格式: {}", raw),
        ));
    }
    let stripped = without_prefix.trim_start_matches('0');
    if stripped.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidInput, "作品编号不能全为 0"));
    }
    Ok(stripped.to_string())
}

pub fn parse_work_info_json(body: &str) -> AppResult<ProviderWork> {
    let v: Value = serde_json::from_str(body).map_err(|e| {
        AppError::new(
            ErrorCode::ProviderSchemaChanged,
            format!("解析作品详情 JSON 失败: {}", e),
        )
    })?;

    let id_num = v
        .get("id")
        .and_then(|i| {
            i.as_i64()
                .map(|n| n.to_string())
                .or_else(|| i.as_str().map(|s| s.to_string()))
        })
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ProviderSchemaChanged,
                "作品详情响应缺少必需的 id 字段",
            )
        })?;

    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ProviderSchemaChanged,
                "作品详情响应缺少必需的 title 字段",
            )
        })?
        .to_string();

    let rj_code = v
        .get("source_id")
        .and_then(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_uppercase())
        .or_else(|| {
            id_num.parse::<u64>().ok().map(|n| {
                if id_num.len() <= 6 {
                    format!("RJ{:06}", n)
                } else {
                    format!("RJ{:08}", n)
                }
            })
        });

    let circle_name = v
        .get("name")
        .and_then(|s| s.as_str())
        .or_else(|| {
            v.get("circle")
                .and_then(|c| c.get("name"))
                .and_then(|s| s.as_str())
        })
        .unwrap_or("")
        .to_string();

    let voice_actors: Vec<String> = v
        .get("vas")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.get("name")
                        .and_then(|n| n.as_str())
                        .or_else(|| item.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    let tags: Vec<String> = v
        .get("tags")
        .and_then(|a| a.as_array())
        .map(|arr| {
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
        })
        .unwrap_or_default();

    let cover_url = v
        .get("mainCoverUrl")
        .or_else(|| v.get("thumbnailCoverUrl"))
        .or_else(|| v.get("samCoverUrl"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());

    let duration_sec = v.get("duration").and_then(|d| d.as_i64());
    let has_subtitle = v
        .get("has_subtitle")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let nsfw = v
        .get("nsfw")
        .and_then(|b| b.as_bool())
        .or_else(|| {
            v.get("age_category_string")
                .and_then(|s| s.as_str())
                .map(|s| s.eq_ignore_ascii_case("r18") || s.eq_ignore_ascii_case("adult"))
        })
        .unwrap_or(false)
        || tags.iter().any(|t| {
            let u = t.trim().to_ascii_uppercase();
            u == "R18" || u == "R-18" || u == "NSFW" || u == "成人向"
        });
    let release_date = v
        .get("release")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let dl_count = v.get("dl_count").and_then(|d| d.as_i64());
    let rate_average_2dp = v.get("rate_average_2dp").and_then(|r| r.as_f64());
    let review_count = v.get("review_count").and_then(|r| r.as_i64());
    let price = v.get("price").and_then(|p| p.as_i64());

    let mut enriched: Value = v;
    if let Some(obj) = enriched.as_object_mut() {
        obj.insert("circle_name".into(), Value::String(circle_name.clone()));
        obj.insert("nsfw".into(), Value::Bool(nsfw));
        obj.insert(
            "voice_actors_list".into(),
            Value::Array(voice_actors.iter().cloned().map(Value::String).collect()),
        );
        obj.insert(
            "tags_list".into(),
            Value::Array(tags.iter().cloned().map(Value::String).collect()),
        );
        if let Some(ref c_url) = cover_url {
            obj.insert("cover_url".into(), Value::String(c_url.clone()));
        }
    }

    Ok(ProviderWork {
        source_work_id: id_num,
        rj_code,
        title,
        circle_name,
        voice_actors,
        tags,
        cover_url,
        duration_sec,
        has_subtitle,
        nsfw,
        release_date,
        dl_count,
        rate_average_2dp,
        review_count,
        price,
        raw_json: enriched.to_string(),
    })
}

pub fn parse_tracks_v2_json(source_work_id: &str, body: &str) -> AppResult<Vec<ProviderLeafNode>> {
    let v: Value = serde_json::from_str(body).map_err(|e| {
        AppError::new(
            ErrorCode::ProviderSchemaChanged,
            format!("解析 tracks?v=2 JSON 失败: {}", e),
        )
    })?;

    let arr = v.as_array().ok_or_else(|| {
        AppError::new(
            ErrorCode::ProviderSchemaChanged,
            "tracks?v=2 顶层结构不是数组",
        )
    })?;

    let mut out = Vec::new();
    fn walk_nodes(
        source_work_id: &str,
        nodes: &[Value],
        dir_parts: &[String],
        out: &mut Vec<ProviderLeafNode>,
    ) {
        for node in nodes {
            let ntype = node
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("other");
            let title = node
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("unnamed")
                .trim()
                .to_string();

            if ntype == "folder" {
                if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
                    let mut next_parts = dir_parts.to_vec();
                    next_parts.push(title);
                    walk_nodes(source_work_id, children, &next_parts, out);
                }
                continue;
            }

            let logical_dir_path = dir_parts.join("/");
            let relative_path = if logical_dir_path.is_empty() {
                title.clone()
            } else {
                format!("{}/{}", logical_dir_path, title)
            };

            let ext = std::path::Path::new(&title)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            let classified_type = if ntype == "audio"
                || matches!(ext.as_str(), "mp3" | "wav" | "flac" | "m4a" | "ogg" | "opus")
            {
                "audio"
            } else if matches!(ext.as_str(), "srt" | "vtt" | "lrc" | "ass") {
                "subtitle"
            } else if ntype == "image" || matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
                "image"
            } else if ntype == "text" || matches!(ext.as_str(), "txt" | "md") {
                "text"
            } else {
                "other"
            };

            let duration_ms = node
                .get("duration")
                .and_then(|d| d.as_f64())
                .map(|secs| (secs * 1000.0).round() as i64);
            let byte_size = node.get("size").and_then(|s| s.as_i64());

            // T303: Prefer remote stable file ID (`hash`), fallback to relative path + size/duration signature
            let remote_file_id = node
                .get("hash")
                .and_then(|h| h.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    format!(
                        "{}:path:{}:{}",
                        source_work_id,
                        relative_path,
                        byte_size.unwrap_or(0)
                    )
                });

            let stream_url = node
                .get("mediaStreamUrl")
                .and_then(|u| u.as_str())
                .map(|s| s.to_string());
            let download_url = node
                .get("mediaDownloadUrl")
                .and_then(|u| u.as_str())
                .map(|s| s.to_string());

            out.push(ProviderLeafNode {
                node_type: classified_type.to_string(),
                title,
                logical_dir_path,
                relative_path,
                remote_file_id,
                format: if ext.is_empty() {
                    "bin".to_string()
                } else {
                    ext
                },
                duration_ms,
                byte_size,
                stream_url,
                download_url,
            });
        }
    }

    walk_nodes(source_work_id, arr, &[], &mut out);
    Ok(out)
}

pub fn parse_works_page_json(body: &str, requested_page: u32) -> AppResult<Page<ProviderWork>> {
    let v: Value = serde_json::from_str(body).map_err(|e| {
        AppError::new(
            ErrorCode::ProviderSchemaChanged,
            format!("解析作品列表 JSON 失败: {}", e),
        )
    })?;

    let works_arr = v
        .get("works")
        .and_then(|w| w.as_array())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ProviderSchemaChanged,
                "作品列表响应缺少 works 数组",
            )
        })?;

    let mut items = Vec::new();
    for item in works_arr {
        if let Ok(pw) = parse_work_info_json(&item.to_string()) {
            items.push(pw);
        }
    }

    let pagination = v.get("pagination");
    let page = pagination
        .and_then(|p| p.get("currentPage"))
        .and_then(|c| c.as_u64())
        .map(|c| c as u32)
        .unwrap_or(requested_page);
    let page_size = pagination
        .and_then(|p| p.get("pageSize"))
        .and_then(|c| c.as_u64())
        .map(|c| c as u32)
        .unwrap_or(20);
    let total_count = pagination
        .and_then(|p| p.get("totalCount"))
        .and_then(|c| c.as_u64())
        .unwrap_or(items.len() as u64);

    Ok(Page {
        items,
        page,
        page_size,
        total_count,
    })
}

#[derive(Clone)]
pub struct AsmrOneProvider {
    db: Database,
    api_base: Arc<RwLock<String>>,
    proxy_url: Arc<RwLock<Option<String>>>,
    auth_token_mem: Arc<RwLock<Option<String>>>,
    nsfw_enabled: Arc<RwLock<bool>>,
    media_cache: Arc<RwLock<HashMap<(String, String), (Vec<ProviderLeafNode>, chrono::DateTime<Utc>)>>>,
}

impl AsmrOneProvider {
    pub fn new(db: Database) -> Self {
        let api_base = db
            .get_setting("api_base_url")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string());
        let proxy_url = db
            .get_setting("proxy_url")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty());
        let nsfw_enabled = db
            .get_setting("nsfw_enabled")
            .ok()
            .flatten()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let token_initial = if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
            entry.get_password().ok().filter(|s| !s.trim().is_empty())
        } else {
            None
        };

        Self {
            db,
            api_base: Arc::new(RwLock::new(api_base)),
            proxy_url: Arc::new(RwLock::new(proxy_url)),
            auth_token_mem: Arc::new(RwLock::new(token_initial)),
            nsfw_enabled: Arc::new(RwLock::new(nsfw_enabled)),
            media_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn nsfw_enabled(&self) -> bool {
        *self.nsfw_enabled.read()
    }

    pub fn set_nsfw_enabled(&self, enabled: bool) -> AppResult<()> {
        *self.nsfw_enabled.write() = enabled;
        self.db
            .set_setting("nsfw_enabled", if enabled { "true" } else { "false" })?;
        Ok(())
    }

    pub fn api_base(&self) -> String {
        self.api_base.read().clone()
    }

    pub fn set_api_base(&self, url: &str) -> AppResult<()> {
        let trimmed = url.trim().trim_end_matches('/').to_string();
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "API 地址必须以 http:// 或 https:// 开头",
            ));
        }
        *self.api_base.write() = trimmed.clone();
        self.db.set_setting("api_base_url", &trimmed)?;
        Ok(())
    }

    pub fn proxy_url(&self) -> Option<String> {
        self.proxy_url.read().clone()
    }

    pub fn set_proxy_url(&self, proxy: Option<String>) -> AppResult<()> {
        let clean = proxy.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(ref p) = clean {
            reqwest::Proxy::all(p).map_err(|e| {
                AppError::new(ErrorCode::InvalidInput, format!("无效的代理地址: {}", e))
            })?;
        }
        *self.proxy_url.write() = clean.clone();
        self.db
            .set_setting("proxy_url", clean.as_deref().unwrap_or(""))?;
        Ok(())
    }

    pub fn set_auth_token(&self, token: Option<String>) -> AppResult<()> {
        let clean = token.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
            match &clean {
                Some(t) => {
                    let _ = entry.set_password(t);
                }
                None => {
                    let _ = entry.delete_credential();
                }
            }
        }
        *self.auth_token_mem.write() = clean.clone();
        self.db
            .set_setting("has_auth_token_ref", if clean.is_some() { "true" } else { "false" })?;
        Ok(())
    }

    pub fn has_auth_token(&self) -> bool {
        self.auth_token_mem.read().is_some()
    }

    pub fn build_client(&self) -> AppResult<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(8))
            .user_agent("ASMRPlayer-Desktop/0.3.0 (Windows NT 10.0; Win64; x64)");

        if let Some(proxy_str) = self.proxy_url() {
            let proxy = reqwest::Proxy::all(&proxy_str).map_err(|e| {
                AppError::new(ErrorCode::InvalidInput, format!("代理配置无效: {}", e))
            })?;
            builder = builder.proxy(proxy);
        }

        builder.build().map_err(|e| {
            AppError::new(
                ErrorCode::NetworkUnavailable,
                format!("创建 HTTP 客户端失败: {}", e),
            )
        })
    }

    pub async fn get_text_with_retry(&self, url: &str) -> AppResult<String> {
        let client = self.build_client()?;
        let token = self.auth_token_mem.read().clone();
        let mut last_err = None;

        for attempt in 0..3 {
            let mut req = client.get(url);
            if let Some(ref t) = token {
                req = req.header(header::AUTHORIZATION, format!("Bearer {}", t));
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return resp.text().await.map_err(|e| {
                            AppError::new(
                                ErrorCode::NetworkUnavailable,
                                format!("读取响应内容失败: {}", e),
                            )
                        });
                    }
                    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                        return Err(AppError::new(
                            ErrorCode::AuthRequired,
                            format!("接口需要认证或权限不足 (HTTP {})", status.as_u16()),
                        ));
                    }
                    if status == StatusCode::NOT_FOUND {
                        return Err(AppError::new(
                            ErrorCode::SourceNotFound,
                            "远端未找到该作品或资源 (HTTP 404)",
                        ));
                    }
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_secs = resp
                            .headers()
                            .get(header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(2)
                            .min(10);
                        if attempt < 2 {
                            tokio::time::sleep(Duration::from_secs(retry_secs)).await;
                            continue;
                        }
                        return Err(AppError::new(
                            ErrorCode::RateLimited,
                            format!("触发站点请求频率限制 (HTTP 429)，建议 {} 秒后重试", retry_secs),
                        ));
                    }
                    if status.is_server_error() && attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(300 * (1 << attempt))).await;
                        continue;
                    }
                    return Err(AppError::new(
                        ErrorCode::NetworkUnavailable,
                        format!("远端返回错误状态码: HTTP {}", status.as_u16()),
                    ));
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(300 * (1 << attempt))).await;
                        continue;
                    }
                }
            }
        }

        Err(AppError::new(
            ErrorCode::NetworkUnavailable,
            format!(
                "网络请求失败: {}",
                last_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "未知错误".into())
            ),
        ))
    }

    pub async fn fetch_work_and_tree(
        &self,
        raw_work_id: &str,
    ) -> AppResult<(ProviderWork, Vec<ProviderLeafNode>, String)> {
        let source_work_id = normalize_work_id_input(raw_work_id)?;
        let base = self.api_base();
        let info_url = format!("{}/api/workInfo/{}", base, source_work_id);
        let tracks_url = format!("{}/api/tracks/{}?v=2", base, source_work_id);

        let info_body = self.get_text_with_retry(&info_url).await?;
        let work = parse_work_info_json(&info_body)?;

        let tracks_body = self.get_text_with_retry(&tracks_url).await?;
        let nodes = parse_tracks_v2_json(&work.source_work_id, &tracks_body)?;

        self.media_cache.write().insert(
            (PROVIDER_ASMR_ONE.to_string(), work.source_work_id.clone()),
            (nodes.clone(), Utc::now()),
        );

        Ok((work, nodes, tracks_body))
    }

    pub async fn download_and_cache_cover(
        &self,
        source_work_id: &str,
        cover_url: Option<&str>,
    ) -> Option<String> {
        let covers_dir = self.db.cache_dir.join("covers");
        let _ = std::fs::create_dir_all(&covers_dir);
        let cover_file = covers_dir.join(format!("asmr_{}.jpg", source_work_id));
        if let Ok(meta) = std::fs::metadata(&cover_file) {
            if meta.is_file() && meta.len() > 64 {
                return Some(
                    cover_file
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string(),
                );
            }
        }

        let target_url = cover_url
            .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!("{}/api/cover/{}.jpg?type=main", self.api_base(), source_work_id)
            });

        let client = self.build_client().ok()?;
        let token = self.auth_token_mem.read().clone();
        let mut req = client.get(&target_url).timeout(Duration::from_secs(6));
        if let Some(ref t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {}", t));
        }

        let resp = req.send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() <= 64 || bytes.len() > 15 * 1024 * 1024 {
            return None;
        }
        if std::fs::write(&cover_file, &bytes).is_ok() {
            Some(
                cover_file
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_string(),
            )
        } else {
            None
        }
    }

    /// T302/T303/T403: Fetches an online work by ID/RJ and persists or refreshes it in SQLite.
    /// Does not overwrite user overrides; detects remote file changes without blindly mapping old progress.
    pub async fn open_online_work(&self, raw_work_id: &str) -> AppResult<WorkDetailDto> {
        let (mut pwork, nodes, tracks_raw_json) = self.fetch_work_and_tree(raw_work_id).await?;
        if let Some(cached_cover) = self
            .download_and_cache_cover(&pwork.source_work_id, pwork.cover_url.as_deref())
            .await
        {
            if let Ok(mut v) = serde_json::from_str::<Value>(&pwork.raw_json) {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("cached_cover_path".into(), Value::String(cached_cover.clone()));
                }
                pwork.raw_json = v.to_string();
            }
            pwork.cover_url = Some(cached_cover);
        }
        let work_id = self.upsert_online_work_in_db(&pwork, &nodes, &tracks_raw_json)?;
        self.db.get_work_detail(&work_id)
    }

    pub fn upsert_online_work_in_db(
        &self,
        pwork: &ProviderWork,
        nodes: &[ProviderLeafNode],
        tracks_raw_json: &str,
    ) -> AppResult<String> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let expires_str = (now + ChronoDuration::hours(18)).to_rfc3339();

        self.db.with_conn(|conn| {
            let tx = conn.transaction()?;

            let existing_ws: Option<(String, String)> = tx
                .query_row(
                    "SELECT id, work_id FROM work_sources WHERE provider = ?1 AND source_work_id = ?2",
                    params![PROVIDER_ASMR_ONE, pwork.source_work_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;

            let vas_json = serde_json::to_string(&pwork.voice_actors).unwrap_or_else(|_| "[]".into());
            let tags_json = serde_json::to_string(&pwork.tags).unwrap_or_else(|_| "[]".into());

            let work_id = if let Some((_ws_id, w_id)) = &existing_ws {
                tx.execute(
                    "UPDATE works
                     SET rj_code = COALESCE(rj_code, ?1),
                         circle_name = CASE WHEN circle_name = '' THEN ?2 ELSE circle_name END,
                         voice_actors_json = ?3,
                         tags_json = ?4,
                         cover_ref = CASE
                             WHEN ?5 IS NOT NULL AND ?5 != '' AND ?5 NOT LIKE 'http%' THEN ?5
                             ELSE COALESCE(cover_ref, ?5)
                         END,
                         updated_at = ?6
                     WHERE id = ?7",
                    params![
                        pwork.rj_code,
                        pwork.circle_name,
                        vas_json,
                        tags_json,
                        pwork.cover_url,
                        now_str,
                        w_id
                    ],
                )?;
                w_id.clone()
            } else {
                let w_id = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO works (id, title, description, rj_code, circle_name, voice_actors_json, tags_json, cover_ref, created_at, updated_at)
                     VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        w_id,
                        pwork.title,
                        pwork.rj_code,
                        pwork.circle_name,
                        vas_json,
                        tags_json,
                        pwork.cover_url,
                        now_str,
                        now_str
                    ],
                )?;
                w_id
            };

            let mut any_source_changed = false;

            for (idx, node) in nodes.iter().enumerate() {
                if node.node_type == "audio" {
                    // Check if media_source already exists by stable remote_file_id
                    let existing_ms: Option<(String, String, Option<i64>, Option<String>)> = tx
                        .query_row(
                            "SELECT id, version_id, byte_size, remote_relative_path
                             FROM media_sources
                             WHERE provider = ?1 AND source_work_id = ?2 AND remote_file_id = ?3",
                            params![PROVIDER_ASMR_ONE, pwork.source_work_id, node.remote_file_id],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                        )
                        .optional()?;

                    if let Some((ms_id, ver_id, old_size, old_rel)) = existing_ms {
                        // T303: Detect if remote size or relative path changed under the same remote_file_id
                        let size_changed = match (old_size, node.byte_size) {
                            (Some(a), Some(b)) => a != b,
                            _ => false,
                        };
                        let path_changed = old_rel
                            .as_deref()
                            .map(|p| p != node.relative_path)
                            .unwrap_or(false);

                        if size_changed || path_changed {
                            any_source_changed = true;
                            tx.execute(
                                "UPDATE track_versions SET association_state = 'pending_confirm' WHERE id = ?1",
                                params![ver_id],
                            )?;
                            tx.execute(
                                "UPDATE media_sources SET byte_size = ?1, remote_relative_path = ?2, availability = 'pending_confirm', updated_at = ?3 WHERE id = ?4",
                                params![node.byte_size, node.relative_path, now_str, ms_id],
                            )?;
                        } else {
                            tx.execute(
                                "UPDATE media_sources SET availability = 'available', updated_at = ?1 WHERE id = ?2",
                                params![now_str, ms_id],
                            )?;
                        }
                    } else {
                        // Check if a track at the same relative path existed with a different remote_file_id!
                        let path_collision_ver: Option<String> = tx
                            .query_row(
                                "SELECT tv.id FROM media_sources ms
                                 JOIN track_versions tv ON tv.id = ms.version_id
                                 JOIN tracks t ON t.id = tv.track_id
                                 WHERE t.work_id = ?1 AND ms.source_kind = 'online_provider' AND ms.remote_relative_path = ?2",
                                params![work_id, node.relative_path],
                                |r| r.get(0),
                            )
                            .optional()?;

                        if path_collision_ver.is_some() {
                            any_source_changed = true;
                        }

                        let track_id = Uuid::new_v4().to_string();
                        let sort_key = crate::services::library::natural_sort_key(&format!(
                            "{}/{:05}_{}",
                            node.logical_dir_path, idx, node.title
                        ));
                        let group_label = if node.logical_dir_path.is_empty() {
                            "默认目录".to_string()
                        } else {
                            node.logical_dir_path.clone()
                        };

                        tx.execute(
                            "INSERT INTO tracks (id, work_id, title, logical_dir_path, group_label, sort_key, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            params![
                                track_id,
                                work_id,
                                node.title,
                                node.logical_dir_path,
                                group_label,
                                sort_key,
                                now_str
                            ],
                        )?;

                        let version_id = Uuid::new_v4().to_string();
                        let assoc_state = if path_collision_ver.is_some() {
                            "pending_confirm"
                        } else {
                            "confirmed"
                        };
                        tx.execute(
                            "INSERT INTO track_versions (id, track_id, version_label, duration_ms, content_fingerprint, association_state, created_at)
                             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
                            params![
                                version_id,
                                track_id,
                                node.format.to_uppercase(),
                                node.duration_ms,
                                assoc_state,
                                now_str
                            ],
                        )?;

                        let ms_id = Uuid::new_v4().to_string();
                        tx.execute(
                            "INSERT INTO media_sources (id, version_id, source_kind, provider, source_work_id, remote_file_id, remote_relative_path, format, byte_size, availability, created_at, updated_at)
                             VALUES (?1, ?2, 'online_provider', ?3, ?4, ?5, ?6, ?7, ?8, 'available', ?9, ?10)",
                            params![
                                ms_id,
                                version_id,
                                PROVIDER_ASMR_ONE,
                                pwork.source_work_id,
                                node.remote_file_id,
                                node.relative_path,
                                node.format,
                                node.byte_size,
                                now_str,
                                now_str
                            ],
                        )?;
                    }
                } else if matches!(node.node_type.as_str(), "subtitle" | "image" | "text") {
                    let att_type = if node.node_type == "image" {
                        "cover"
                    } else {
                        node.node_type.as_str()
                    };
                    let exists: Option<String> = tx
                        .query_row(
                            "SELECT id FROM attachments WHERE work_id = ?1 AND provider = ?2 AND remote_file_id = ?3",
                            params![work_id, PROVIDER_ASMR_ONE, node.remote_file_id],
                            |r| r.get(0),
                        )
                        .optional()?;
                    if exists.is_none() {
                        tx.execute(
                            "INSERT INTO attachments (id, work_id, version_id, attachment_type, title, relative_path, provider, source_work_id, remote_file_id, subtitle_offset_ms, encoding, availability, created_at)
                             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, 0, 'UTF-8', 'available', ?9)",
                            params![
                                Uuid::new_v4().to_string(),
                                work_id,
                                att_type,
                                node.title,
                                node.relative_path,
                                PROVIDER_ASMR_ONE,
                                pwork.source_work_id,
                                node.remote_file_id,
                                now_str
                            ],
                        )?;
                    }
                }
            }

            let sync_status = if any_source_changed {
                "pending_confirm"
            } else {
                "ok"
            };

            if let Some((ws_id, _)) = existing_ws {
                tx.execute(
                    "UPDATE work_sources
                     SET raw_metadata_json = ?1, cached_tree_json = ?2, sync_status = ?3, refreshed_at = ?4, expires_at = ?5
                     WHERE id = ?6",
                    params![
                        pwork.raw_json,
                        tracks_raw_json,
                        sync_status,
                        now_str,
                        expires_str,
                        ws_id
                    ],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO work_sources (id, work_id, provider, source_work_id, raw_metadata_json, cached_tree_json, sync_status, refreshed_at, expires_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        Uuid::new_v4().to_string(),
                        work_id,
                        PROVIDER_ASMR_ONE,
                        pwork.source_work_id,
                        pwork.raw_json,
                        tracks_raw_json,
                        sync_status,
                        now_str,
                        expires_str
                    ],
                )?;
            }

            let _ = crate::services::library::auto_pair_subtitles_in_conn(&tx, &work_id);

            tx.commit()?;
            Ok(work_id)
        })
    }

    /// T1106: Fetches remote subtitle files (.lrc/.srt/.vtt) for a work from asmr.one,
    /// saves them into `cache_dir/subtitles`, and auto-pairs them to local/online track versions.
    pub async fn fetch_online_subtitles_for_work(
        &self,
        work_id: &str,
        rj_or_id: Option<&str>,
    ) -> AppResult<usize> {
        let source_work_id = if let Some(raw) = rj_or_id.filter(|s| !s.trim().is_empty()) {
            normalize_work_id_input(raw)?
        } else {
            let db_id: Option<String> = self.db.with_conn(|conn| {
                let ws: Option<String> = conn
                    .query_row(
                        "SELECT source_work_id FROM work_sources WHERE work_id = ?1 LIMIT 1",
                        params![work_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if ws.is_some() {
                    return Ok(ws);
                }
                let rj: Option<String> = conn
                    .query_row(
                        "SELECT rj_code FROM works WHERE id = ?1",
                        params![work_id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .flatten();
                Ok(rj)
            })?;
            let raw = db_id.ok_or_else(|| {
                AppError::new(
                    ErrorCode::InvalidInput,
                    "该作品尚未关联 RJ 号，无法检索云端字幕",
                )
            })?;
            normalize_work_id_input(&raw)?
        };

        let base = self.api_base();
        let tracks_url = format!("{}/api/tracks/{}?v=2", base, source_work_id);
        let tracks_body = self.get_text_with_retry(&tracks_url).await?;
        let leaves = parse_tracks_v2_json(&source_work_id, &tracks_body)?;

        let sub_dir = self.db.cache_dir.join("subtitles");
        let _ = std::fs::create_dir_all(&sub_dir);
        let now_str = Utc::now().to_rfc3339();
        let mut downloaded_count = 0usize;

        for node in leaves.into_iter().filter(|n| n.node_type == "subtitle") {
            let Some(url) = node.download_url.clone().or(node.stream_url.clone()) else {
                continue;
            };
            let Ok(text_content) = self.get_text_with_retry(&url).await else {
                continue;
            };
            let safe_name: String = node
                .title
                .chars()
                .map(|c| {
                    if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            let local_file = sub_dir.join(format!("{}_{}", work_id, safe_name));
            if std::fs::write(&local_file, text_content.as_bytes()).is_err() {
                continue;
            }
            let abs_str = local_file
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string();

            if let Ok(cues) =
                crate::services::library::parse_subtitle_bytes(text_content.as_bytes(), 0)
            {
                if !cues.is_empty() {
                    let corpus = cues
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let _ = self.db.index_subtitle_text_for_work(work_id, None, &corpus);
                }
            }

            let _ = self.db.with_conn(|conn| {
                let existing: Option<String> = conn
                    .query_row(
                        "SELECT id FROM attachments WHERE work_id = ?1 AND (remote_file_id = ?2 OR LOWER(title) = LOWER(?3)) LIMIT 1",
                        params![work_id, node.remote_file_id, node.title],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(aid) = existing {
                    conn.execute(
                        "UPDATE attachments SET absolute_path = ?1, availability = 'available' WHERE id = ?2",
                        params![abs_str, aid],
                    )?;
                } else {
                    conn.execute(
                        "INSERT INTO attachments (id, work_id, version_id, attachment_type, title, relative_path, absolute_path, provider, source_work_id, remote_file_id, subtitle_offset_ms, encoding, availability, created_at)
                         VALUES (?1, ?2, NULL, 'subtitle', ?3, ?4, ?5, ?6, ?7, ?8, 0, 'UTF-8', 'available', ?9)",
                        params![
                            Uuid::new_v4().to_string(),
                            work_id,
                            node.title,
                            node.relative_path,
                            abs_str,
                            PROVIDER_ASMR_ONE,
                            source_work_id,
                            node.remote_file_id,
                            now_str
                        ],
                    )?;
                }
                Ok(())
            });
            downloaded_count += 1;
        }

        let paired = self
            .db
            .with_conn(|conn| crate::services::library::auto_pair_subtitles_in_conn(conn, work_id))?;

        Ok(downloaded_count.max(paired))
    }

    /// Automatically downloads and caches an online subtitle attachment into `cache_dir/subtitles`
    /// if its `absolute_path` is not yet populated on disk.
    pub async fn ensure_online_subtitle_cached(&self, attachment_id: &str) -> AppResult<String> {
        let row: Option<(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT work_id, title, relative_path, absolute_path, provider, source_work_id, remote_file_id
                 FROM attachments WHERE id = ?1",
                params![attachment_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(Into::into)
        })?;

        let (work_id, title, rel_path, abs_path, provider, source_work_id, remote_file_id) =
            row.ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到字幕附件"))?;

        if let Some(ref p) = abs_path {
            if std::path::Path::new(p).is_file() {
                return Ok(p.clone());
            }
        }

        let Some(src_wid) = source_work_id.filter(|s| !s.trim().is_empty()) else {
            return Err(AppError::new(
                ErrorCode::SourceNotFound,
                "未找到本地字幕文件，且该附件缺少在线作品源信息",
            ));
        };

        let media_ref = ProviderMediaRef {
            provider: provider.unwrap_or_else(|| PROVIDER_ASMR_ONE.to_string()),
            source_work_id: src_wid,
            remote_file_id: remote_file_id.unwrap_or_default(),
            remote_relative_path: rel_path.unwrap_or_else(|| title.clone()),
        };

        let resolved = self
            .resolve_media(&media_ref, MediaPurpose::Download, false)
            .await?;
        let text_content = self.get_text_with_retry(&resolved.url).await?;

        let sub_dir = self.db.cache_dir.join("subtitles");
        let _ = std::fs::create_dir_all(&sub_dir);
        let safe_name: String = title
            .chars()
            .map(|c| {
                if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let local_file = sub_dir.join(format!("{}_{}", work_id, safe_name));
        std::fs::write(&local_file, text_content.as_bytes())?;
        let abs_str = local_file
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();

        let _ = self.db.with_conn(|conn| {
            conn.execute(
                "UPDATE attachments SET absolute_path = ?1, availability = 'available' WHERE id = ?2",
                params![abs_str, attachment_id],
            )?;
            Ok(())
        });

        Ok(abs_str)
    }

    /// T304: Resolves fresh stream or download URL on demand from `ProviderMediaRef`.
    /// Never persists temporary daily URLs in the database.
    pub async fn resolve_media(
        &self,
        media_ref: &ProviderMediaRef,
        purpose: MediaPurpose,
        force_refresh: bool,
    ) -> AppResult<ResolvedMedia> {
        let key = (
            media_ref.provider.clone(),
            media_ref.source_work_id.clone(),
        );

        let nodes = if !force_refresh {
            let cached = self.media_cache.read().get(&key).cloned();
            if let Some((list, ts)) = cached {
                if Utc::now() - ts < ChronoDuration::minutes(15) {
                    Some(list)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let leaf_list = match nodes {
            Some(list) => list,
            None => {
                let base = self.api_base();
                let tracks_url = format!("{}/api/tracks/{}?v=2", base, media_ref.source_work_id);
                let tracks_body = self.get_text_with_retry(&tracks_url).await?;
                let parsed = parse_tracks_v2_json(&media_ref.source_work_id, &tracks_body)?;
                self.media_cache
                    .write()
                    .insert(key, (parsed.clone(), Utc::now()));
                parsed
            }
        };

        let matched = leaf_list
            .iter()
            .find(|n| n.remote_file_id == media_ref.remote_file_id)
            .or_else(|| {
                leaf_list
                    .iter()
                    .find(|n| n.relative_path == media_ref.remote_relative_path)
            })
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::SourceChanged,
                    format!(
                        "远端目录中未找到文件引用 {} ({})",
                        media_ref.remote_file_id, media_ref.remote_relative_path
                    ),
                )
            })?;

        let url = match purpose {
            MediaPurpose::Play => matched
                .stream_url
                .clone()
                .or_else(|| matched.download_url.clone()),
            MediaPurpose::Download => matched
                .download_url
                .clone()
                .or_else(|| matched.stream_url.clone()),
        }
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::ProviderSchemaChanged,
                "远端节点缺少可用的媒体流/下载地址",
            )
        })?;

        Ok(ResolvedMedia {
            url,
            headers: Vec::new(),
            byte_size: matched.byte_size,
            expires_at: Some((Utc::now() + ChronoDuration::hours(6)).to_rfc3339()),
        })
    }

    pub async fn post_json_with_retry(&self, url: &str, payload: &Value) -> AppResult<String> {
        let client = self.build_client()?;
        let token = self.auth_token_mem.read().clone();
        let mut last_err = None;

        for attempt in 0..2 {
            let mut req = client.post(url).json(payload);
            if let Some(ref t) = token {
                req = req.header(header::AUTHORIZATION, format!("Bearer {}", t));
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return resp.text().await.map_err(|e| {
                            AppError::new(
                                ErrorCode::NetworkUnavailable,
                                format!("读取响应内容失败: {}", e),
                            )
                        });
                    }
                    if status.is_server_error() && attempt < 1 {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                    return Err(AppError::new(
                        ErrorCode::NetworkUnavailable,
                        format!("远端返回错误状态码: HTTP {}", status.as_u16()),
                    ));
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt < 1 {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                }
            }
        }

        Err(AppError::new(
            ErrorCode::NetworkUnavailable,
            format!(
                "网络请求失败: {}",
                last_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "未知错误".into())
            ),
        ))
    }

    fn fill_default_cover_urls(base: &str, mut page_res: Page<ProviderWork>) -> Page<ProviderWork> {
        for item in &mut page_res.items {
            if item.cover_url.is_none() {
                item.cover_url = Some(format!(
                    "{}/api/cover/{}.jpg?type=main",
                    base, item.source_work_id
                ));
            }
        }
        page_res
    }

    /// T1119: Filters out NSFW works when `nsfw_enabled == false` (default OFF).
    fn apply_nsfw_filter(&self, mut page_res: Page<ProviderWork>) -> Page<ProviderWork> {
        if !self.nsfw_enabled() {
            let before = page_res.items.len();
            page_res.items.retain(|w| !w.nsfw);
            let removed = (before - page_res.items.len()) as u64;
            if removed > 0 && page_res.total_count >= removed {
                page_res.total_count -= removed;
            }
        }
        page_res
    }

    pub const ONLINE_PAGE_SIZE: usize = 12;
    const UPSTREAM_PAGE_SIZE: usize = 20;

    async fn fetch_single_upstream_browse_page(
        &self,
        up_page: u32,
        order: &str,
        sort: &str,
        subtitle_only: bool,
    ) -> AppResult<(Page<ProviderWork>, usize)> {
        let base = self.api_base();
        let sub_flag = if subtitle_only { 1 } else { 0 };
        let nsfw_allowed = self.nsfw_enabled();

        if order == "popular" {
            // In SFW mode (!nsfw_allowed), `/api/recommender/popular` on asmr.one is ~95% R18.
            // Query server-side `$age:general$` sorted by dl_count desc first so every page has full SFW items.
            if !nsfw_allowed {
                let sfw_url = format!(
                    "{}/api/search/%24age%3Ageneral%24?order=dl_count&sort=desc&page={}&subtitle={}",
                    base, up_page, sub_flag
                );
                if let Ok(body) = self.get_text_with_retry(&sfw_url).await {
                    if let Ok(parsed) = parse_works_page_json(&body, up_page) {
                        let raw_len = parsed.items.len();
                        if raw_len > 0 {
                            let filtered = self
                                .apply_nsfw_filter(Self::fill_default_cover_urls(&base, parsed));
                            if !filtered.items.is_empty() {
                                return Ok((filtered, raw_len));
                            }
                        }
                    }
                }
            }

            let pop_url = format!("{}/api/recommender/popular", base);
            let payload = serde_json::json!({
                "keyword": " ",
                "page": up_page,
                "subtitle": sub_flag,
                "localSubtitledWorks": [],
                "withPlaylistStatus": []
            });
            if let Ok(body) = self.post_json_with_retry(&pop_url, &payload).await {
                if let Ok(parsed) = parse_works_page_json(&body, up_page) {
                    let raw_len = parsed.items.len();
                    if raw_len > 0 {
                        let filtered =
                            self.apply_nsfw_filter(Self::fill_default_cover_urls(&base, parsed));
                        return Ok((filtered, raw_len));
                    }
                }
            }

            let fallback_url = format!(
                "{}/api/works?order=dl_count&sort=desc&page={}&subtitle={}",
                base, up_page, sub_flag
            );
            let body = self.get_text_with_retry(&fallback_url).await?;
            let parsed = parse_works_page_json(&body, up_page)?;
            let raw_len = parsed.items.len();
            return Ok((
                self.apply_nsfw_filter(Self::fill_default_cover_urls(&base, parsed)),
                raw_len,
            ));
        }

        if !nsfw_allowed {
            let sfw_url = format!(
                "{}/api/search/%24age%3Ageneral%24?order={}&sort={}&page={}&subtitle={}",
                base,
                urlencoding::encode(order),
                urlencoding::encode(sort),
                up_page,
                sub_flag
            );
            if let Ok(body) = self.get_text_with_retry(&sfw_url).await {
                if let Ok(parsed) = parse_works_page_json(&body, up_page) {
                    let raw_len = parsed.items.len();
                    if raw_len > 0 {
                        let filtered =
                            self.apply_nsfw_filter(Self::fill_default_cover_urls(&base, parsed));
                        if !filtered.items.is_empty() {
                            return Ok((filtered, raw_len));
                        }
                    }
                }
            }
        }

        let url = format!(
            "{}/api/works?order={}&sort={}&page={}&subtitle={}",
            base,
            urlencoding::encode(order),
            urlencoding::encode(sort),
            up_page,
            sub_flag
        );
        let body = self.get_text_with_retry(&url).await?;
        let parsed = parse_works_page_json(&body, up_page)?;
        let raw_len = parsed.items.len();
        Ok((
            self.apply_nsfw_filter(Self::fill_default_cover_urls(&base, parsed)),
            raw_len,
        ))
    }

    async fn fetch_single_upstream_search_page(
        &self,
        q: &str,
        up_page: u32,
        effective_order: &str,
        sort: &str,
        subtitle_only: bool,
    ) -> AppResult<(Page<ProviderWork>, usize)> {
        let base = self.api_base();
        let sub_flag = if subtitle_only { 1 } else { 0 };

        if !self.nsfw_enabled() && !q.contains("$age:") && !q.contains("$nsfw:") {
            let sfw_q = format!("{} $age:general$", q);
            let sfw_url = format!(
                "{}/api/search/{}?order={}&sort={}&page={}&subtitle={}",
                base,
                urlencoding::encode(&sfw_q),
                urlencoding::encode(effective_order),
                urlencoding::encode(sort),
                up_page,
                sub_flag
            );
            if let Ok(body) = self.get_text_with_retry(&sfw_url).await {
                if let Ok(parsed) = parse_works_page_json(&body, up_page) {
                    let raw_len = parsed.items.len();
                    if raw_len > 0 {
                        let filtered =
                            self.apply_nsfw_filter(Self::fill_default_cover_urls(&base, parsed));
                        if !filtered.items.is_empty() {
                            return Ok((filtered, raw_len));
                        }
                    }
                }
            }
        }

        let url = format!(
            "{}/api/search/{}?order={}&sort={}&page={}&subtitle={}",
            base,
            urlencoding::encode(q),
            urlencoding::encode(effective_order),
            urlencoding::encode(sort),
            up_page,
            sub_flag
        );
        let body = self.get_text_with_retry(&url).await?;
        let parsed = parse_works_page_json(&body, up_page)?;
        let raw_len = parsed.items.len();
        Ok((
            self.apply_nsfw_filter(Self::fill_default_cover_urls(&base, parsed)),
            raw_len,
        ))
    }

    pub async fn browse_works(
        &self,
        page: u32,
        order: &str,
        sort: &str,
        subtitle_only: bool,
    ) -> AppResult<Page<ProviderWork>> {
        let safe_page = page.max(1);
        let start_global = ((safe_page - 1) as usize) * Self::ONLINE_PAGE_SIZE;
        let first_up_page = ((start_global / Self::UPSTREAM_PAGE_SIZE) + 1) as u32;
        let skip_in_first = start_global % Self::UPSTREAM_PAGE_SIZE;
        let needed_total = skip_in_first + Self::ONLINE_PAGE_SIZE;

        let mut collected: Vec<ProviderWork> = Vec::with_capacity(needed_total + 8);
        let mut total_count: u64 = 0;

        for offset_page in 0..4u32 {
            let up_page = first_up_page + offset_page;
            match self
                .fetch_single_upstream_browse_page(up_page, order, sort, subtitle_only)
                .await
            {
                Ok((up_res, raw_len)) => {
                    total_count = total_count.max(up_res.total_count);
                    for item in up_res.items {
                        if !collected
                            .iter()
                            .any(|w| w.source_work_id == item.source_work_id)
                        {
                            collected.push(item);
                        }
                    }
                    if collected.len() >= needed_total || raw_len < Self::UPSTREAM_PAGE_SIZE {
                        break;
                    }
                }
                Err(e) => {
                    if offset_page == 0 {
                        return Err(e);
                    }
                    break;
                }
            }
        }

        let items: Vec<ProviderWork> = collected
            .into_iter()
            .skip(skip_in_first)
            .take(Self::ONLINE_PAGE_SIZE)
            .collect();

        Ok(Page {
            items,
            page: safe_page,
            page_size: Self::ONLINE_PAGE_SIZE as u32,
            total_count,
        })
    }

    pub async fn search_works(
        &self,
        keyword: &str,
        page: u32,
        order: &str,
        sort: &str,
        subtitle_only: bool,
    ) -> AppResult<Page<ProviderWork>> {
        let q = keyword.trim();
        if q.is_empty() {
            return self.browse_works(page, order, sort, subtitle_only).await;
        }
        let effective_order = if order == "popular" { "dl_count" } else { order };
        let safe_page = page.max(1);
        let start_global = ((safe_page - 1) as usize) * Self::ONLINE_PAGE_SIZE;
        let first_up_page = ((start_global / Self::UPSTREAM_PAGE_SIZE) + 1) as u32;
        let skip_in_first = start_global % Self::UPSTREAM_PAGE_SIZE;
        let needed_total = skip_in_first + Self::ONLINE_PAGE_SIZE;

        let mut collected: Vec<ProviderWork> = Vec::with_capacity(needed_total + 8);
        let mut total_count: u64 = 0;

        for offset_page in 0..4u32 {
            let up_page = first_up_page + offset_page;
            match self
                .fetch_single_upstream_search_page(q, up_page, effective_order, sort, subtitle_only)
                .await
            {
                Ok((up_res, raw_len)) => {
                    total_count = total_count.max(up_res.total_count);
                    for item in up_res.items {
                        if !collected
                            .iter()
                            .any(|w| w.source_work_id == item.source_work_id)
                        {
                            collected.push(item);
                        }
                    }
                    if collected.len() >= needed_total || raw_len < Self::UPSTREAM_PAGE_SIZE {
                        break;
                    }
                }
                Err(e) => {
                    if offset_page == 0 {
                        return Err(e);
                    }
                    break;
                }
            }
        }

        let items: Vec<ProviderWork> = collected
            .into_iter()
            .skip(skip_in_first)
            .take(Self::ONLINE_PAGE_SIZE)
            .collect();

        Ok(Page {
            items,
            page: safe_page,
            page_size: Self::ONLINE_PAGE_SIZE as u32,
            total_count,
        })
    }

    /// Ensures a work in SQLite has `online_provider` media sources attached (by fetching from `asmr.one`
    /// using its RJ code or `rj_or_id` if it currently only has local metadata), enabling direct download from Library.
    pub async fn ensure_work_online_tracks(
        &self,
        work_id: &str,
        rj_or_id: Option<&str>,
    ) -> AppResult<()> {
        let has_online_sources: bool = self.db.with_conn(|conn| {
            let found: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM media_sources ms
                     JOIN track_versions tv ON tv.id = ms.version_id
                     JOIN tracks t ON t.id = tv.track_id
                     WHERE t.work_id = ?1 AND ms.source_kind = 'online_provider'
                     LIMIT 1",
                    params![work_id],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(found.is_some())
        })?;

        if has_online_sources {
            return Ok(());
        }

        let resolved_raw_id = if let Some(s) = rj_or_id.map(|v| v.trim()).filter(|v| !v.is_empty()) {
            s.to_string()
        } else {
            let (db_rj, db_title, ws_source_id): (Option<String>, String, Option<String>) =
                self.db.with_conn(|conn| {
                    conn.query_row(
                        "SELECT w.rj_code, w.title,
                                (SELECT ws.source_work_id FROM work_sources ws WHERE ws.work_id = w.id LIMIT 1)
                         FROM works w WHERE w.id = ?1",
                        params![work_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?
                    .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到该作品"))
                })?;

            ws_source_id
                .or(db_rj)
                .or_else(|| crate::services::library::extract_rj_code(&db_title))
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::InvalidInput,
                        "该作品尚未关联 RJ 编号，请先在详情页输入 RJ 号同步在线目录后再下载",
                    )
                })?
        };

        let (mut pwork, nodes, tracks_raw_json) = self.fetch_work_and_tree(&resolved_raw_id).await?;
        if let Some(cached_cover) = self
            .download_and_cache_cover(&pwork.source_work_id, pwork.cover_url.as_deref())
            .await
        {
            pwork.cover_url = Some(cached_cover);
        }

        let online_wid = self.upsert_online_work_in_db(&pwork, &nodes, &tracks_raw_json)?;
        if online_wid != work_id {
            let _ = self.db.associate_works(work_id, &online_wid);
        }
        Ok(())
    }

    /// Fetches `workInfo` and cover from `asmr.one` for an existing work in SQLite (by `rj_or_id`
    /// or its detected `rj_code`) and populates its cover, circle, voice actors (CV), and tags
    /// without altering local track versions or overriding user `work_overrides` (R03, R09).
    pub async fn sync_work_from_asmr_one(
        &self,
        work_id: &str,
        rj_or_id: Option<&str>,
        prefer_online_cover: bool,
    ) -> AppResult<WorkDetailDto> {
        let resolved_raw_id = if let Some(s) = rj_or_id.map(|v| v.trim()).filter(|v| !v.is_empty()) {
            s.to_string()
        } else {
            let (db_rj, db_title, ws_source_id): (Option<String>, String, Option<String>) =
                self.db.with_conn(|conn| {
                    conn.query_row(
                        "SELECT w.rj_code, w.title,
                                (SELECT ws.source_work_id FROM work_sources ws WHERE ws.work_id = w.id LIMIT 1)
                         FROM works w WHERE w.id = ?1",
                        params![work_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?
                    .ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到该作品"))
                })?;

            ws_source_id
                .or(db_rj)
                .or_else(|| crate::services::library::extract_rj_code(&db_title))
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::InvalidInput,
                        "该作品未检测到 RJ 编号，请手动输入 RJ 号或作品 ID 同步",
                    )
                })?
        };

        let source_work_id = normalize_work_id_input(&resolved_raw_id)?;
        let info_url = format!("{}/api/workInfo/{}", self.api_base(), source_work_id);
        let info_body = self.get_text_with_retry(&info_url).await?;
        let mut pwork = parse_work_info_json(&info_body)?;

        if let Some(cached_cover) = self
            .download_and_cache_cover(&pwork.source_work_id, pwork.cover_url.as_deref())
            .await
        {
            if let Ok(mut v) = serde_json::from_str::<Value>(&pwork.raw_json) {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("cached_cover_path".into(), Value::String(cached_cover.clone()));
                }
                pwork.raw_json = v.to_string();
            }
            pwork.cover_url = Some(cached_cover);
        }

        let vas_json = serde_json::to_string(&pwork.voice_actors).unwrap_or_else(|_| "[]".into());
        let tags_json = serde_json::to_string(&pwork.tags).unwrap_or_else(|_| "[]".into());
        let now_str = Utc::now().to_rfc3339();
        let prefer_flag: i64 = if prefer_online_cover { 1 } else { 0 };

        self.db.with_conn(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE works
                 SET rj_code = COALESCE(NULLIF(rj_code, ''), ?1),
                     circle_name = CASE WHEN ?2 != '' THEN ?2 ELSE circle_name END,
                     voice_actors_json = CASE WHEN ?3 != '[]' THEN ?3 ELSE voice_actors_json END,
                     tags_json = CASE WHEN ?4 != '[]' THEN ?4 ELSE tags_json END,
                     cover_ref = CASE WHEN ?6 = 1 THEN COALESCE(?5, cover_ref) ELSE COALESCE(cover_ref, ?5) END,
                     updated_at = ?7
                 WHERE id = ?8",
                params![
                    pwork.rj_code,
                    pwork.circle_name,
                    vas_json,
                    tags_json,
                    pwork.cover_url,
                    prefer_flag,
                    now_str,
                    work_id
                ],
            )?;

            // If work_sources already belongs to this work, refresh its raw_metadata_json
            let existing_ws: Option<(String, String)> = tx
                .query_row(
                    "SELECT id, work_id FROM work_sources WHERE provider = ?1 AND source_work_id = ?2",
                    params![PROVIDER_ASMR_ONE, pwork.source_work_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;

            if let Some((ws_id, owner_wid)) = existing_ws {
                if owner_wid == work_id {
                    tx.execute(
                        "UPDATE work_sources SET raw_metadata_json = ?1, refreshed_at = ?2 WHERE id = ?3",
                        params![pwork.raw_json, now_str, ws_id],
                    )?;
                }
            }

            tx.commit()?;
            Ok(())
        })?;

        self.db.get_work_detail(work_id)
    }

    /// Best-effort automatic metadata & cover enrichment for local works that have an RJ code
    /// but are missing cover, voice actors (CV), or tags.
    pub async fn auto_enrich_local_works(&self) -> usize {
        let candidates: Vec<(String, String)> = match self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, rj_code FROM works
                 WHERE rj_code IS NOT NULL AND rj_code != ''
                   AND (voice_actors_json = '[]' OR tags_json = '[]' OR cover_ref IS NULL OR circle_name = '')
                 ORDER BY updated_at DESC LIMIT 15",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        }) {
            Ok(list) => list,
            Err(_) => return 0,
        };

        let mut enriched_count = 0usize;
        for (wid, rj) in candidates {
            if self
                .sync_work_from_asmr_one(&wid, Some(&rj), false)
                .await
                .is_ok()
            {
                enriched_count += 1;
            }
        }
        enriched_count
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_normalize_work_id_input() {
        assert_eq!(normalize_work_id_input("RJ01705068").unwrap(), "1705068");
        assert_eq!(normalize_work_id_input("rj01705068").unwrap(), "1705068");
        assert_eq!(normalize_work_id_input("01705068").unwrap(), "1705068");
        assert_eq!(normalize_work_id_input("1705068").unwrap(), "1705068");
        assert!(normalize_work_id_input("").is_err());
        assert!(normalize_work_id_input("RJ0000").is_err());
        assert!(normalize_work_id_input("RJ12AB").is_err());
    }

    #[test]
    fn test_parse_sanitized_fixtures_and_unknown_field_tolerance_t301() {
        let wi_str = include_str!("../../../tests/fixtures/provider/work_info_sample.json");
        let work = parse_work_info_json(wi_str).unwrap();
        assert_eq!(work.source_work_id, "1705068");
        assert_eq!(work.rj_code.as_deref(), Some("RJ01705068"));
        assert_eq!(work.circle_name, "サンプルサークル");
        assert_eq!(work.voice_actors, vec!["テスト声優"]);
        assert!(work.tags.contains(&"耳かき".to_string()));

        let tr_str = include_str!("../../../tests/fixtures/provider/tracks_v2_sample.json");
        let nodes = parse_tracks_v2_json("1705068", tr_str).unwrap();
        assert_eq!(nodes.len(), 6);
        let first_mp3 = &nodes[0];
        assert_eq!(first_mp3.node_type, "audio");
        assert_eq!(first_mp3.remote_file_id, "1705068/1944933");
        assert_eq!(first_mp3.logical_dir_path, "mp3");
        assert_eq!(first_mp3.format, "mp3");

        let wp_str = include_str!("../../../tests/fixtures/provider/works_page_sample.json");
        let page = parse_works_page_json(wp_str, 1).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.total_count, 1);
    }

    #[tokio::test]
    async fn test_provider_error_classification_r08() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db);
        provider.set_api_base(&server.uri()).unwrap();

        // 401 -> AuthRequired
        Mock::given(method("GET"))
            .and(path("/api/workInfo/1001"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let err_401 = provider.open_online_work("1001").await.unwrap_err();
        assert_eq!(err_401.code, ErrorCode::AuthRequired);

        // 404 -> SourceNotFound
        Mock::given(method("GET"))
            .and(path("/api/workInfo/1004"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err_404 = provider.open_online_work("1004").await.unwrap_err();
        assert_eq!(err_404.code, ErrorCode::SourceNotFound);

        // 429 -> RateLimited
        Mock::given(method("GET"))
            .and(path("/api/workInfo/1029"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
            .mount(&server)
            .await;
        let err_429 = provider.open_online_work("1029").await.unwrap_err();
        assert_eq!(err_429.code, ErrorCode::RateLimited);

        // Invalid JSON schema -> ProviderSchemaChanged
        Mock::given(method("GET"))
            .and(path("/api/workInfo/1050"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"unexpected\":true}"))
            .mount(&server)
            .await;
        let err_schema = provider.open_online_work("1050").await.unwrap_err();
        assert_eq!(err_schema.code, ErrorCode::ProviderSchemaChanged);
    }

    #[tokio::test]
    async fn test_sync_work_from_asmr_one_and_auto_enrich_local_works() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        provider.set_api_base(&server.uri()).unwrap();

        let wi_str = include_str!("../../../tests/fixtures/provider/work_info_sample.json")
            .replace("https://api.example.invalid", &server.uri());
        Mock::given(method("GET"))
            .and(path("/api/workInfo/1705068"))
            .respond_with(ResponseTemplate::new(200).set_body_string(wi_str))
            .mount(&server)
            .await;

        let fake_jpg = vec![0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F']
            .into_iter()
            .chain(std::iter::repeat(0x42u8).take(128))
            .collect::<Vec<u8>>();
        Mock::given(method("GET"))
            .and(path("/api/cover/1705068.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(fake_jpg))
            .mount(&server)
            .await;

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, rj_code, circle_name, voice_actors_json, tags_json, cover_ref, created_at, updated_at)
                 VALUES ('w_local', 'RJ01705068 本地作品', 'RJ01705068', '', '[]', '[]', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let enriched = provider.auto_enrich_local_works().await;
        assert_eq!(enriched, 1);

        let detail = db.get_work_detail("w_local").unwrap();
        assert_eq!(detail.circle_name, "サンプルサークル");
        assert_eq!(detail.voice_actors, vec!["テスト声優"]);
        assert!(detail.tags.contains(&"耳かき".to_string()));
        assert!(
            detail
                .cover_ref
                .as_deref()
                .unwrap_or("")
                .starts_with("data:image/jpeg;base64,"),
            "Cached asmr.one cover should be resolved as data URL for WebView2"
        );
    }

    #[tokio::test]
    async fn test_browse_popular_works_and_fallback() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db);
        provider.set_api_base(&server.uri()).unwrap();

        let wp_str = include_str!("../../../tests/fixtures/provider/works_page_sample.json");
        Mock::given(method("POST"))
            .and(path("/api/recommender/popular"))
            .respond_with(ResponseTemplate::new(200).set_body_string(wp_str))
            .mount(&server)
            .await;

        let page = provider
            .browse_works(1, "popular", "desc", false)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].rj_code.as_deref(), Some("RJ01705068"));
        assert!(page.items[0]
            .cover_url
            .as_deref()
            .unwrap_or("")
            .contains("/api/cover/1705068.jpg"));
    }

    #[tokio::test]
    async fn test_nsfw_filter_defaults_to_false_and_toggles_t1119_r39() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db);
        provider.set_api_base(&server.uri()).unwrap();

        assert!(
            !provider.nsfw_enabled(),
            "NSFW setting must default to OFF (false)"
        );

        let mixed_page_json = serde_json::json!({
            "works": [
                {
                    "id": 100001,
                    "title": "全年齢 癒やし耳かき作品",
                    "name": "健全サークル",
                    "nsfw": false,
                    "vas": [{"name": "CV_A"}],
                    "tags": [{"name": "耳かき"}],
                    "source_id": "RJ01000001"
                },
                {
                    "id": 100002,
                    "title": "R18 大人向けオトナ作品",
                    "name": "大人サークル",
                    "nsfw": true,
                    "vas": [{"name": "CV_B"}],
                    "tags": [{"name": "R18"}],
                    "source_id": "RJ01000002"
                }
            ],
            "pagination": {
                "currentPage": 1,
                "pageSize": 20,
                "totalCount": 2
            }
        })
        .to_string();

        Mock::given(method("POST"))
            .and(path("/api/recommender/popular"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mixed_page_json))
            .mount(&server)
            .await;

        // 1. Default OFF (false) -> NSFW work must be filtered out!
        let page_filtered = provider
            .browse_works(1, "popular", "desc", false)
            .await
            .unwrap();
        assert_eq!(page_filtered.items.len(), 1);
        assert_eq!(page_filtered.items[0].rj_code.as_deref(), Some("RJ01000001"));
        assert!(!page_filtered.items[0].nsfw);
        assert_eq!(page_filtered.total_count, 1);

        // 2. Enable NSFW -> both works must be returned!
        provider.set_nsfw_enabled(true).unwrap();
        assert!(provider.nsfw_enabled());
        let page_unfiltered = provider
            .browse_works(1, "popular", "desc", false)
            .await
            .unwrap();
        assert_eq!(page_unfiltered.items.len(), 2);
        assert_eq!(page_unfiltered.total_count, 2);
        assert!(page_unfiltered.items.iter().any(|w| w.nsfw));
    }
}
