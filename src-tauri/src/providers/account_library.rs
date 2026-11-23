use super::{parse_work_info_json, AsmrOneProvider, ProviderWork, PROVIDER_ASMR_ONE};
use crate::db::playlists::{self, Playlist, WorkItem};
use crate::domain::{AppError, AppResult, ErrorCode};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLibraryPreview {
    pub id: String,
    pub favorites: usize,
    pub playlists: usize,
    pub playlist_works: usize,
    pub already_in_library: usize,
    pub works: Vec<ImportTitle>,
    pub playlist_names: Vec<String>,
    pub site_playlists: Vec<ImportPlaylist>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlaylist {
    pub id: String,
    pub name: String,
    pub work_count: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLibrarySelection {
    pub work_ids: HashSet<String>,
    pub playlist_ids: HashSet<String>,
}

#[derive(Serialize)]
pub struct ImportTitle {
    pub id: String,
    pub title: String,
    pub code: String,
}

pub(super) struct PendingImport {
    id: String,
    revision: u64,
    works: Vec<ProviderWork>,
    existing: HashMap<String, String>,
    lists: Vec<SiteList>,
}

struct SiteList {
    id: String,
    name: String,
    members: Vec<String>,
}

fn cache_card(tx: &Connection, work: &ProviderWork, now: &str) -> AppResult<String> {
    let work_id = match existing_work(tx, work)? {
        Some((id, _)) => id,
        None => {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO works(id,title,rj_code,circle_name,voice_actors_json,tags_json,
                    cover_ref,nsfw_detected,has_subtitle_metadata,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
                params![
                    id,
                    work.title,
                    work.rj_code,
                    work.circle_name,
                    json!(work.voice_actors).to_string(),
                    json!(work.tags).to_string(),
                    work.cover_url,
                    work.nsfw,
                    work.has_subtitle,
                    now
                ],
            )?;
            id
        }
    };
    let metadata = json!({
        "id": work.source_work_id, "source_id": work.rj_code, "title": work.title,
        "nsfw": work.nsfw, "has_subtitle": work.has_subtitle, "release": work.release_date,
        "dl_count": work.dl_count, "rate_average_2dp": work.rate_average_2dp,
    });
    tx.execute(
        "INSERT INTO work_sources(id,work_id,provider,source_work_id,raw_metadata_json,refreshed_at)
         VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(provider,source_work_id) DO NOTHING",
        params![Uuid::new_v4().to_string(), work_id, PROVIDER_ASMR_ONE, work.source_work_id, metadata.to_string(), now],
    )?;
    Ok(work_id)
}

fn changed() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "账号或预览已更改，请重新读取收藏与播放列表。",
    )
}

fn schema(reason: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::ProviderSchemaChanged, reason)
}

fn page_integer(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let text = value.as_str()?;
        if text.bytes().all(|b| b.is_ascii_digit()) {
            text.parse().ok()
        } else {
            None
        }
    })
}

/// The site uses both page-number fields. Playlist responses use `page`.
fn account_page<'a>(
    data: &'a Value,
    key: &str,
    requested: u64,
) -> Result<(&'a [Value], bool), &'static str> {
    let rows = data[key].as_array().ok_or("站点未返回条目列表")?;
    let pagination = data["pagination"].as_object().ok_or("站点未返回分页信息")?;
    let total = pagination
        .get("totalCount")
        .and_then(page_integer)
        .ok_or("作品或歌单总数无效")?;
    let empty = total == 0 && requested == 1 && rows.is_empty();
    let mut reported = ["page", "currentPage"]
        .iter()
        .filter_map(|field| pagination.get(*field))
        .peekable();
    if reported.peek().is_none() && !empty {
        return Err("站点未返回页码");
    }
    if reported.any(|v| page_integer(v) != Some(requested)) {
        return Err("返回的页码与请求不符");
    }
    if empty {
        return Ok((rows, false));
    }
    let size = pagination
        .get("pageSize")
        .and_then(page_integer)
        .filter(|size| *size > 0)
        .ok_or("每页条目数无效")?;
    if total == 0 || rows.is_empty() {
        return Err("返回的条目与分页总数不符");
    }
    Ok((rows, requested.saturating_mul(size) < total))
}

fn existing_work(conn: &Connection, work: &ProviderWork) -> AppResult<Option<(String, bool)>> {
    let known = conn
        .prepare_cached(
            "SELECT s.work_id, EXISTS(SELECT 1 FROM collected_works c WHERE c.work_id=s.work_id)
         FROM work_sources s WHERE s.provider=?1 AND s.source_work_id=?2",
        )?
        .query_row(params![PROVIDER_ASMR_ONE, work.source_work_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    if known.as_ref().is_some_and(|(_, collected)| *collected) {
        return Ok(known);
    }
    // An unambiguous local edition may already have been imported from a folder.
    let mut stmt = conn.prepare_cached(
        "SELECT w.id, EXISTS(SELECT 1 FROM collected_works c WHERE c.work_id=w.id)
         FROM works w WHERE w.rj_code=?1 AND EXISTS(SELECT 1 FROM library_entries e WHERE e.work_id=w.id)
         AND NOT EXISTS(SELECT 1 FROM work_sources s WHERE s.work_id=w.id AND s.provider=?2) LIMIT 2"
    )?;
    let mut rows = stmt.query(params![work.rj_code, PROVIDER_ASMR_ONE])?;
    let Some(row) = rows.next()? else {
        return Ok(known);
    };
    let local: (String, bool) = (row.get(0)?, row.get(1)?);
    if rows.next()?.is_some() {
        return Ok(known);
    }
    Ok(if local.1 {
        Some(local)
    } else {
        known.or(Some(local))
    })
}

impl AsmrOneProvider {
    pub async fn collection_work_id(&self, raw: &str) -> AppResult<String> {
        let revision = self.account.read().revision;
        let lookup = self.resolve_work_input(raw).await?;
        let work = self.fetch_work_info(&lookup).await?;
        let account = self.account.read();
        if revision != account.revision {
            return Err(changed());
        }
        self.db.with_conn(|conn| {
            let tx = conn.transaction()?;
            let id = cache_card(&tx, &work, &Utc::now().to_rfc3339())?;
            tx.commit()?;
            Ok(id)
        })
    }

    async fn account_pages(
        &self,
        base: &str,
        token: &str,
        revision: u64,
        route: &str,
        key: &str,
        section: &str,
    ) -> AppResult<Vec<Value>> {
        let mut rows = Vec::new();
        for page in 1u64.. {
            if self.account.read().revision != revision {
                return Err(changed());
            }
            let body = self
                .request_api_text(&format!("{base}{route}&page={page}"), None, Some(token))
                .await?;
            let page_error =
                |reason: &str| schema(format!("无法读取「{section}」第 {page} 页，{reason}。"));
            let data: Value =
                serde_json::from_str(&body).map_err(|_| page_error("站点未返回有效的 JSON"))?;
            let (batch, has_more) = account_page(&data, key, page).map_err(page_error)?;
            rows.extend(batch.iter().cloned());
            if !has_more {
                break;
            }
        }
        Ok(rows)
    }

    pub async fn preview_account_library(&self) -> AppResult<AccountLibraryPreview> {
        let (base, token, revision) = {
            let account = self.account.read();
            let credential = account
                .credential
                .as_ref()
                .ok_or_else(|| AppError::new(ErrorCode::AuthRequired, "请先登录 asmr.one。"))?;
            (
                account.base.clone(),
                credential.token.clone(),
                account.revision,
            )
        };
        let id = Uuid::new_v4().to_string();
        *self.library_preview.lock() = Some(PendingImport {
            id: id.clone(),
            revision,
            works: vec![],
            existing: HashMap::new(),
            lists: Vec::new(),
        });
        let mut works = Vec::new();
        let mut seen = HashMap::new();
        let favorites = self
            .account_pages(
                &base,
                &token,
                revision,
                "/api/review?order=updated_at&sort=desc",
                "works",
                "我的收藏",
            )
            .await?;
        let mut add_work = |row: Value, playlist: bool| -> AppResult<String> {
            let mut work = parse_work_info_json(&row.to_string())?;
            work.source_work_id = super::normalize_work_id_input(&work.source_work_id)
                .map_err(|_| schema("站点返回的作品编号无效。"))?;
            // Private review fields from the list response are not part of the work card.
            work.raw_json = String::new();
            let source = work.source_work_id.clone();
            let flags = seen.entry(source.clone()).or_insert_with(|| {
                works.push(work);
                (false, false)
            });
            if playlist {
                flags.1 = true;
            } else {
                flags.0 = true;
            }
            Ok(source)
        };
        for row in favorites {
            add_work(row, false)?;
        }
        let mut playlists = HashSet::new();
        let mut lists = Vec::new();
        for (filter, section) in [("owned", "我创建的歌单"), ("liked", "我收藏的歌单")]
        {
            let route = format!("/api/playlist/get-playlists?filterBy={filter}&pageSize=96");
            for item in self
                .account_pages(&base, &token, revision, &route, "playlists", section)
                .await?
            {
                let playlist = match &item["id"] {
                    Value::String(s) if !s.is_empty() => s.clone(),
                    Value::Number(n) if n.is_u64() => n.to_string(),
                    _ => return Err(schema(format!("「{section}」中有一个歌单缺少编号。"))),
                };
                if !playlists.insert(playlist.clone()) {
                    continue;
                }
                let route = format!(
                    "/api/playlist/get-playlist-works?id={}&pageSize=96",
                    urlencoding::encode(&playlist)
                );
                let mut members = Vec::new();
                let mut unique = HashSet::new();
                for row in self
                    .account_pages(&base, &token, revision, &route, "works", "歌单作品")
                    .await?
                {
                    let source = add_work(row, true)?;
                    if unique.insert(source.clone()) {
                        members.push(source);
                    }
                }
                let name = item["name"]
                    .as_str()
                    .or_else(|| item["title"].as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("官网播放列表")
                    .to_string();
                let namespace = if super::OFFICIAL_API_BASES.contains(&base.as_str()) {
                    "asmr.one"
                } else {
                    base.as_str()
                };
                let id = format!(
                    "asmr-list-{}",
                    hex::encode(Sha256::digest(
                        format!("{namespace}/playlist/{playlist}").as_bytes()
                    ))
                );
                lists.push(SiteList { id, name, members });
            }
        }
        let total = works.len();
        let db = self.db.clone();
        let (missing, existing) = tokio::task::spawn_blocking(move || {
            db.with_conn(|conn| {
                let mut missing = Vec::new();
                let mut existing = HashMap::new();
                for work in works {
                    if let Some((id, true)) = existing_work(conn, &work)? {
                        existing.insert(work.source_work_id.clone(), id);
                    } else {
                        missing.push(work);
                    }
                }
                Ok((missing, existing))
            })
        })
        .await
        .map_err(|e| AppError::new(ErrorCode::InternalError, e.to_string()))??;
        let preview = AccountLibraryPreview {
            id: id.clone(),
            favorites: seen.values().filter(|f| f.0).count(),
            playlists: playlists.len(),
            playlist_names: lists.iter().map(|p| p.name.clone()).collect(),
            site_playlists: lists
                .iter()
                .map(|p| ImportPlaylist {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    work_count: p.members.len(),
                })
                .collect(),
            playlist_works: seen.values().filter(|f| f.1).count(),
            already_in_library: total - missing.len(),
            works: missing
                .iter()
                .map(|w| ImportTitle {
                    id: w.source_work_id.clone(),
                    title: w.title.clone(),
                    code: w
                        .rj_code
                        .clone()
                        .unwrap_or_else(|| w.source_work_id.clone()),
                })
                .collect(),
        };
        let account = self.account.read();
        let mut pending = self.library_preview.lock();
        let slot = pending
            .as_mut()
            .filter(|p| p.id == id && account.revision == revision)
            .ok_or_else(changed)?;
        slot.works = missing;
        slot.existing = existing;
        slot.lists = lists;
        Ok(preview)
    }

    #[cfg(test)]
    pub fn decide_account_library(&self, id: &str, accept: bool) -> AppResult<usize> {
        self.apply_account_library(id, accept, false, None)
    }
    pub fn apply_account_library(
        &self,
        id: &str,
        accept: bool,
        keep_lists: bool,
        selection: Option<&AccountLibrarySelection>,
    ) -> AppResult<usize> {
        let account = self.account.read();
        let mut pending = self.library_preview.lock();
        let Some(preview) = pending.as_ref().filter(|p| p.id == id) else {
            return if accept { Err(changed()) } else { Ok(0) };
        };
        if !accept {
            pending.take();
            return Ok(0);
        }
        if account.revision != preview.revision {
            pending.take();
            return Err(changed());
        }
        if let Some(selected) = selection {
            let works = preview
                .works
                .iter()
                .filter(|w| selected.work_ids.contains(&w.source_work_id))
                .count();
            let lists = preview
                .lists
                .iter()
                .filter(|p| selected.playlist_ids.contains(&p.id))
                .count();
            if works != selected.work_ids.len() || lists != selected.playlist_ids.len() {
                return Err(changed());
            }
        }
        let save_lists = selection.map_or(keep_lists, |s| !s.playlist_ids.is_empty());
        let added = self.db.with_conn(|conn| {
            let tx = conn.transaction()?;
            let now = Utc::now().to_rfc3339();
            let mut added = 0;
            let mut owners = preview.existing.clone();
            let mut skipped_cached = HashMap::new();
            for work in &preview.works {
                if selection.is_some_and(|s| !s.work_ids.contains(&work.source_work_id)) {
                    if save_lists {
                        if let Some((id, collected)) = existing_work(&tx, work)? {
                            if collected {
                                owners.insert(work.source_work_id.clone(), id);
                            } else {
                                skipped_cached.insert(work.source_work_id.clone(), id);
                            }
                        }
                    }
                    continue;
                }
                let work_id = cache_card(&tx, work, &now)?;
                owners.insert(work.source_work_id.clone(), work_id.clone());
                added += tx.execute(
                    "INSERT OR IGNORE INTO collected_works(work_id,created_at) VALUES(?1,?2)",
                    params![work_id, now],
                )?;
            }
            if save_lists {
                let mut stored: HashMap<_, _> = playlists::read_all(&tx)?
                    .into_iter()
                    .map(|p| (p.id.clone(), p))
                    .collect();
                let mut next_order = stored.values().map(|p| p.sort_order).max().unwrap_or(-1) + 1;
                for list in &preview.lists {
                    if selection.is_some_and(|s| !s.playlist_ids.contains(&list.id)) {
                        continue;
                    }
                    let mut p = stored.remove(&list.id).unwrap_or_else(|| {
                        let sort_order = next_order;
                        next_order += 1;
                        Playlist {
                            id: list.id.clone(),
                            name: list.name.clone(),
                            version_ids: vec![],
                            work_items: Some(vec![]),
                            sort_order,
                        }
                    });
                    let previous = p.work_items.take().ok_or_else(changed)?;
                    let choices: HashMap<_, _> =
                        previous.iter().map(|w| (w.id.clone(), w)).collect();
                    let mut unique = HashSet::new();
                    let mut items = Vec::new();
                    for source in &list.members {
                        // A skipped, uncollected work can only keep an existing place in this collection.
                        let Some(id) = owners.get(source).or_else(|| {
                            skipped_cached
                                .get(source)
                                .filter(|id| choices.contains_key(*id))
                        }) else {
                            continue;
                        };
                        if unique.insert(id.clone()) {
                            let mut work = choices
                                .get(id)
                                .map(|w| (*w).clone())
                                .unwrap_or_else(|| WorkItem::new(id.clone()));
                            work.from_site = true;
                            items.push(work);
                        }
                    }
                    items.extend(
                        previous
                            .into_iter()
                            .filter(|w| !w.from_site && unique.insert(w.id.clone())),
                    );
                    p.name = list.name.clone();
                    p.work_items = Some(items);
                    playlists::put(&tx, &p)?;
                }
            }
            tx.commit()?;
            Ok(added)
        })?;
        pending.take();
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::tempdir;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn page(key: &str, rows: Vec<Value>, number: u32, total: u32) -> Value {
        json!({key: rows, "pagination":{"currentPage":number,"pageSize":1,"totalCount":total}})
    }
    async fn serve(server: &MockServer, endpoint: &str, params: &[(&str, &str)], body: Value) {
        let mut mock = Mock::given(method("GET"))
            .and(path(endpoint))
            .and(header("authorization", "Bearer fixture"));
        for &(key, value) in params {
            mock = mock.and(query_param(key, value));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
    #[tokio::test]
    async fn preview_reject_confirm_and_open_account_works() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let server = MockServer::start().await;
        let provider = AsmrOneProvider::new(db.clone());
        provider.set_api_base(&server.uri()).unwrap();
        provider.set_auth_token(Some("fixture".into())).unwrap();
        let a = json!({"id":101,"title":"Local","source_id":"RJ000101"});
        let b = json!({"id":102,"title":"收藏中的作品"});
        let c = json!({"id":100000041,"title":"歌单中的作品","source_id":"VJ01002150"});
        serve(
            &server,
            "/api/review",
            &[("page", "1"), ("order", "updated_at")],
            page("works", vec![a], 1, 2),
        )
        .await;
        serve(
            &server,
            "/api/review",
            &[("page", "2")],
            page("works", vec![b], 2, 2),
        )
        .await;
        for filter in ["owned", "liked"] {
            serve(
                &server,
                "/api/playlist/get-playlists",
                &[("filterBy", filter)],
                page(
                    "playlists",
                    vec![json!({"id":filter,"name":format!("官网 {filter}")})],
                    1,
                    1,
                ),
            )
            .await;
        }
        serve(
            &server,
            "/api/playlist/get-playlist-works",
            &[("id", "owned")],
            page(
                "works",
                vec![json!({"id":"000102","title":"收藏中的作品"})],
                1,
                1,
            ),
        )
        .await;
        serve(
            &server,
            "/api/playlist/get-playlist-works",
            &[("id", "liked")],
            page("works", vec![c.clone()], 1, 1),
        )
        .await;
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,rj_code,created_at,updated_at) VALUES('local','自定标题','RJ000101','now','now');
                INSERT INTO library_roots(id,absolute_path,display_name,created_at) VALUES('root','fixture','fixture','now');
                INSERT INTO library_entries(root_id,relative_work_path,work_id,last_seen_at) VALUES('root','.','local','now');
                INSERT INTO collected_works(work_id,created_at) VALUES('local','now');
                INSERT INTO works(id,title,rj_code,created_at,updated_at) VALUES('cached','曾浏览的作品','RJ000101','now','now');
                INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES('source','cached','asmr_one','101','now');")?;
            Ok(())
        }).unwrap();
        let preview = provider.preview_account_library().await.unwrap();
        assert_eq!(
            (
                preview.favorites,
                preview.playlists,
                preview.playlist_works,
                preview.already_in_library
            ),
            (2, 2, 2, 1)
        );
        assert_eq!(preview.works.len(), 2);
        assert_eq!(
            provider.decide_account_library(&preview.id, false).unwrap(),
            0
        );
        assert!(provider.decide_account_library(&preview.id, true).is_err());
        assert_eq!(db.list_works("all", None).unwrap().len(), 1);
        let preview = provider.preview_account_library().await.unwrap();
        provider.set_auth_token(Some("fixture".into())).unwrap();
        assert!(provider.decide_account_library(&preview.id, true).is_err());
        let preview = provider.preview_account_library().await.unwrap();
        assert_eq!(
            provider.decide_account_library(&preview.id, true).unwrap(),
            2
        );
        let again = provider.preview_account_library().await.unwrap();
        assert!(again.works.is_empty());
        assert!(db.list_playlists().unwrap().is_empty());
        assert_eq!(
            provider
                .apply_account_library(&again.id, true, true, None)
                .unwrap(),
            0
        );
        let lists = db.list_playlists().unwrap();
        assert_eq!(lists.len(), 2);
        assert!(lists.iter().all(|p| p.name.starts_with("官网 ")
            && p.work_items.as_ref().unwrap().len() == 1
            && p.version_ids.is_empty()));
        let third = provider.preview_account_library().await.unwrap();
        provider
            .apply_account_library(&third.id, true, true, None)
            .unwrap();
        assert_eq!(
            lists.iter().map(|p| &p.id).collect::<Vec<_>>(),
            db.list_playlists()
                .unwrap()
                .iter()
                .map(|p| &p.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(db.list_works("all", None).unwrap().len(), 3);
        assert_eq!(db.get_work_detail("local").unwrap().title, "自定标题");
        assert!(db.list_works("favorites", None).unwrap().is_empty());
        let imported = db
            .list_works("online", None)
            .unwrap()
            .into_iter()
            .find(|w| w.rj_code.as_deref() == Some("VJ01002150"))
            .unwrap();
        assert!(server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| !r.url.path().starts_with("/api/tracks/")));
        serve(&server, "/api/workInfo/100000041", &[], c).await;
        serve(
            &server,
            "/api/tracks/100000041",
            &[],
            json!([{"type":"audio","title":"01.wav","hash":"fixture-audio"}]),
        )
        .await;
        let detail = provider.library_work_detail(&imported.id).await.unwrap();
        assert_eq!(detail.id, imported.id);
        assert_eq!(detail.tracks.len(), 1);
        provider.set_auth_token(None).unwrap();
    }

    #[tokio::test]
    async fn site_collections_preserve_titles_source_order_shared_members_and_empty_lists() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let server = MockServer::start().await;
        provider.set_api_base(&server.uri()).unwrap();
        provider.set_auth_token(Some("fixture".into())).unwrap();
        serve(&server, "/api/review", &[], page("works", vec![], 1, 0)).await;
        let site_title = "长名称".repeat(40);
        for (filter, num, total, id, name) in [
            ("owned", "1", 2, "pair", "睡前"),
            ("owned", "2", 2, "empty", "睡前"),
            ("liked", "1", 1, "shared", site_title.as_str()),
        ] {
            serve(
                &server,
                "/api/playlist/get-playlists",
                &[("filterBy", filter), ("page", num)],
                page(
                    "playlists",
                    vec![json!({"id":id,"name":name})],
                    num.parse().unwrap(),
                    total,
                ),
            )
            .await;
        }
        for (id, num, total, work) in [
            ("pair", "1", 2, 42),
            ("pair", "2", 2, 17),
            ("shared", "1", 1, 17),
        ] {
            serve(
                &server,
                "/api/playlist/get-playlist-works",
                &[("id", id), ("page", num)],
                page(
                    "works",
                    vec![json!({"id":work,"title":format!("作品{work}")})],
                    num.parse().unwrap(),
                    total,
                ),
            )
            .await;
        }
        serve(
            &server,
            "/api/playlist/get-playlist-works",
            &[("id", "empty")],
            page("works", vec![], 1, 0),
        )
        .await;
        let preview = provider.preview_account_library().await.unwrap();
        let pair_id = preview.site_playlists[0].id.clone();
        assert_ne!(pair_id, preview.site_playlists[1].id);
        assert_eq!(preview.site_playlists[0].work_count, 2);
        let mut selected = AccountLibrarySelection {
            work_ids: HashSet::from(["17".to_string()]),
            playlist_ids: HashSet::from(["foreign".to_string()]),
        };
        assert!(provider
            .apply_account_library(&preview.id, true, false, Some(&selected))
            .is_err());
        assert!(db.list_works("all", None).unwrap().is_empty());
        assert!(db.list_playlists().unwrap().is_empty());

        selected.playlist_ids = HashSet::from([pair_id.clone()]);
        assert_eq!(
            provider
                .apply_account_library(&preview.id, true, false, Some(&selected))
                .unwrap(),
            1
        );
        let saved = db.list_playlists().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, pair_id);
        let members = saved[0].work_items.as_ref().unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(db.get_work_detail(&members[0].id).unwrap().title, "作品17");

        let preview = provider.preview_account_library().await.unwrap();
        assert_eq!(
            preview
                .works
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            vec!["42"]
        );
        let empty_only = AccountLibrarySelection {
            work_ids: HashSet::new(),
            playlist_ids: HashSet::from([preview.site_playlists[1].id.clone()]),
        };
        assert_eq!(
            provider
                .apply_account_library(&preview.id, true, false, Some(&empty_only))
                .unwrap(),
            0
        );
        let saved = db.list_playlists().unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].work_items.as_ref().unwrap().len(), 1);
        assert!(saved[1].work_items.as_ref().unwrap().is_empty());

        let preview = provider.preview_account_library().await.unwrap();
        assert_eq!(
            provider
                .apply_account_library(&preview.id, true, true, None)
                .unwrap(),
            1
        );
        let groups = db.list_playlists().unwrap();
        assert_eq!(
            groups.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["睡前", "睡前", site_title.as_str()]
        );
        let pair = groups[0].work_items.as_ref().unwrap();
        assert_eq!(
            pair.iter()
                .map(|w| db.get_work_detail(&w.id).unwrap().title)
                .collect::<Vec<_>>(),
            vec!["作品42", "作品17"]
        );
        assert!(groups[1].work_items.as_ref().unwrap().is_empty());
        assert_eq!(groups[2].work_items.as_ref().unwrap()[0].id, pair[1].id);
        assert!(pair.iter().all(|w| w.from_site));
        let again = provider.preview_account_library().await.unwrap();
        assert!(again.works.is_empty());
        assert_eq!(
            provider
                .apply_account_library(&again.id, true, true, None)
                .unwrap(),
            0
        );
        assert_eq!(db.list_playlists().unwrap().len(), 3);

        // A saved member can remain in its collection after the user removes it from the library.
        assert!(!db.toggle_collected(&pair[0].id).unwrap());
        let preview = provider.preview_account_library().await.unwrap();
        let keep_pair = AccountLibrarySelection {
            work_ids: HashSet::new(),
            playlist_ids: HashSet::from([pair_id]),
        };
        assert_eq!(
            provider
                .apply_account_library(&preview.id, true, false, Some(&keep_pair))
                .unwrap(),
            0
        );
        assert_eq!(
            db.list_playlists().unwrap()[0].work_items.as_ref(),
            Some(pair)
        );
        assert_eq!(db.list_works("all", None).unwrap().len(), 1);
        provider.set_auth_token(None).unwrap();
    }

    #[tokio::test]
    async fn playlist_page_alias_reads_all_100_works_in_site_order() {
        let pages: Vec<Value> = serde_json::from_str(include_str!(
            "../../../android/app/src/test/resources/asmr-one-playlist-pages.json"
        ))
        .unwrap();
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let server = MockServer::start().await;
        provider.set_api_base(&server.uri()).unwrap();
        provider.set_auth_token(Some("fixture".into())).unwrap();
        serve(&server, "/api/review", &[], page("works", vec![], 1, 0)).await;
        for (filter, rows) in [
            ("owned", vec![json!({"id":"sample","name":"测试歌单"})]),
            ("liked", vec![]),
        ] {
            let size = rows.len();
            serve(
                &server,
                "/api/playlist/get-playlists",
                &[("filterBy", filter)],
                json!({"playlists":rows,"pagination":{"page":1,"pageSize":96,"totalCount":size}}),
            )
            .await;
        }
        for (index, data) in pages.into_iter().enumerate() {
            let number = (index + 1).to_string();
            serve(
                &server,
                "/api/playlist/get-playlist-works",
                &[("page", &number), ("id", "sample")],
                data,
            )
            .await;
        }
        let preview = provider.preview_account_library().await.unwrap();
        assert_eq!(preview.playlist_works, 100);
        assert_eq!(
            preview
                .works
                .iter()
                .map(|w| w.id.parse::<usize>().unwrap())
                .collect::<Vec<_>>(),
            (1..=100).collect::<Vec<_>>()
        );
        assert!(db.list_playlists().unwrap().is_empty());
        assert_eq!(
            provider
                .apply_account_library(&preview.id, true, true, None)
                .unwrap(),
            100
        );
        let lists = db.list_playlists().unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].work_items.as_ref().unwrap().len(), 100);
        provider.set_auth_token(None).unwrap();
    }

    #[test]
    fn account_pagination_rejects_stale_pages_and_incomplete_lists() {
        let mut data =
            json!({"works":[{"id":1}], "pagination":{"page":"2","pageSize":"1","totalCount":"2"}});
        assert!(!account_page(&data, "works", 2).unwrap().1);
        assert!(account_page(&data, "works", 1).is_err());
        data["pagination"]["currentPage"] = json!(1);
        assert!(account_page(&data, "works", 2).is_err());
        data["pagination"] = json!({"pageSize":1,"totalCount":2});
        assert!(account_page(&data, "works", 1).is_err());
        data["pagination"] = json!({"currentPage":1,"pageSize":1,"totalCount":2});
        assert!(account_page(&data, "works", 1).unwrap().1);
        data["works"] = json!([]);
        assert!(account_page(&data, "works", 1).is_err());
        data["pagination"] = json!({"totalCount":0});
        assert!(!account_page(&data, "works", 1).unwrap().1);
    }

    #[tokio::test]
    async fn later_page_failure_never_supplies_a_partial_import() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let server = MockServer::start().await;
        let provider = AsmrOneProvider::new(db.clone());
        provider.set_api_base(&server.uri()).unwrap();
        assert!(provider.preview_account_library().await.is_err());
        provider.set_auth_token(Some("fixture".into())).unwrap();
        serve(
            &server,
            "/api/review",
            &[("page", "1")],
            page("works", vec![json!({"id":10,"title":"部分结果"})], 1, 2),
        )
        .await;
        Mock::given(path("/api/review"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert!(
            matches!(provider.preview_account_library().await, Err(error) if error.code == ErrorCode::AuthRequired)
        );
        assert!(db.list_works("all", None).unwrap().is_empty());
        provider.set_auth_token(None).unwrap();
    }
}
