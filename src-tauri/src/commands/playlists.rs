use super::AppState;
use crate::db::playlists::WorkItem;
use crate::db::Database;
use crate::domain::ListeningStatus;
use crate::domain::WorkDetailDto;
use crate::domain::{AppError, AppResult, ErrorCode};
use crate::services::file_preferences::{Effects, FilePreferences, Format};
use crate::services::playback::PlaybackService;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use tauri::{Emitter, State};

fn string<'a>(input: &'a Value, key: &str) -> AppResult<&'a str> {
    input[key]
        .as_str()
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, format!("缺少 {key}")))
}
fn ids(input: &Value) -> AppResult<Vec<String>> {
    serde_json::from_value(input.get("versionIds").cloned().unwrap_or(json!([])))
        .map_err(|_| AppError::new(ErrorCode::InvalidInput, "音轨列表无效"))
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}
fn works(input: &Value) -> AppResult<Option<Vec<WorkItem>>> {
    input
        .get("workItems")
        .map(|raw| serde_json::from_value(raw.clone()).map_err(|_| invalid("作品列表无效")))
        .transpose()
}

fn collection_versions(
    mut detail: WorkDetailDto,
    work: &WorkItem,
    prefs: &FilePreferences,
) -> Vec<(String, Option<String>)> {
    let mut groups = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut resume = None;
    for track in &detail.tracks {
        if work
            .directory
            .as_ref()
            .is_some_and(|path| path != &track.logical_dir_path)
        {
            continue;
        }
        for version in track
            .versions
            .iter()
            .filter(|v| v.association_state == "confirmed")
        {
            for source in version
                .sources
                .iter()
                .filter(|s| s.availability == "available")
            {
                let format = source.format.trim_start_matches('.').to_lowercase();
                if format.is_empty()
                    || work
                        .format
                        .as_ref()
                        .is_some_and(|f| !format.eq_ignore_ascii_case(f))
                {
                    continue;
                }
                let group = (track.logical_dir_path.clone(), format);
                if detail.last_version_id.as_ref() == Some(&version.id)
                    && version.position_ms > 0
                    && !version.completed
                {
                    resume = Some(group.clone());
                }
                if seen.insert(group.clone()) {
                    groups.push(group);
                }
            }
        }
    }
    let Some((directory, format)) = preferred_group(&groups, prefs, resume.as_ref()) else {
        return vec![];
    };
    detail.tracks.retain(|t| &t.logical_dir_path == directory);
    for track in &mut detail.tracks {
        track
            .versions
            .retain(|v| v.association_state == "confirmed");
        for v in &mut track.versions {
            v.sources
                .retain(|s| s.availability == "available" && s.format.eq_ignore_ascii_case(format));
        }
    }
    PlaybackService::select_preferred_tracks_for_work(&detail)
}

fn preferred_group<'a>(
    groups: &'a [(String, String)],
    prefs: &FilePreferences,
    resume: Option<&(String, String)>,
) -> Option<&'a (String, String)> {
    let paths: Vec<Vec<_>> = groups
        .iter()
        .map(|(dir, _)| dir.split(['/', '\\']).collect())
        .collect();
    let first = paths.first()?;
    let common = first
        .iter()
        .enumerate()
        .take_while(|(i, part)| paths.iter().all(|p| p.get(*i) == Some(*part)))
        .count();
    let preference = match prefs.format {
        Format::Mp3 => "mp3",
        Format::Wav => "wav",
        Format::Any => "",
    };
    groups
        .iter()
        .enumerate()
        .max_by_key(|(i, group)| {
            let path = &paths[*i];
            let mut effect = Effects::Any;
            for part in path.iter().rev() {
                let s = part.to_lowercase().replace([' ', '_', '-'], "");
                if [
                    "seなし",
                    "se無し",
                    "seナシ",
                    "se無",
                    "无音效",
                    "無音效",
                    "无效果音",
                    "無効果音",
                    "効果音なし",
                    "無し版",
                    "なし版",
                    "nose",
                    "withoutse",
                ]
                .iter()
                .any(|marker| s.contains(marker))
                {
                    effect = Effects::Without;
                    break;
                }
                if [
                    "seあり",
                    "seアリ",
                    "se有",
                    "有se",
                    "有音效",
                    "有效果音",
                    "効果音あり",
                    "withse",
                    "あり版",
                    "有り版",
                ]
                .iter()
                .any(|marker| s.contains(marker))
                {
                    effect = Effects::With;
                    break;
                }
            }
            let relative = path[common..].join("/").to_lowercase();
            let bonus = [
                "特典",
                "おまけ",
                "試聴",
                "试听",
                "試聽",
                "体験版",
                "體驗版",
                "サンプル",
                "フリートーク",
                "过往作品",
                "额外",
                "自由谈话",
                "预告",
                "予告",
                "sample",
                "preview",
                "trailer",
                "bonus",
                "extra",
                "free talk",
            ]
            .iter()
            .any(|tag| relative.contains(tag));
            let match_effect = if prefs.effects == Effects::Any {
                0
            } else if effect == prefs.effects {
                2
            } else if effect == Effects::Any {
                1
            } else {
                0
            };
            (
                !bonus,
                match_effect,
                group.1 == preference,
                std::cmp::Reverse(path.iter().filter(|part| !part.is_empty()).count()),
                resume == Some(*group),
                std::cmp::Reverse(*i),
            )
        })
        .map(|(_, g)| g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_cards_preserve_order_shared_members_and_hidden_placeholders() {
        use crate::db::playlists::{put, Playlist};
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        db.with_conn(|conn| {
            conn.execute_batch(
                "
                INSERT INTO works(id,title,nsfw_detected,created_at,updated_at) VALUES
                    ('visible','可见作品',0,'now','now'), ('hidden','私密标题',1,'now','now');
                INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at)
                    VALUES ('shown-source','visible','asmr_one','100','now');",
            )?;
            for (i, ids) in [
                vec!["visible", "missing", "hidden"],
                vec!["hidden", "visible"],
            ]
            .iter()
            .enumerate()
            {
                put(
                    conn,
                    &Playlist {
                        id: format!("p{i}"),
                        name: format!("组 {i}"),
                        sort_order: i as i64,
                        version_ids: vec![],
                        work_items: Some(
                            ids.iter().map(|id| WorkItem::new((*id).into())).collect(),
                        ),
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();
        let lists = list_with_members(&db, false).unwrap();
        let rows = lists[0]["works"].as_array().unwrap();
        assert_eq!(rows[0]["title"], "可见作品");
        assert_eq!(rows[0]["available"], true);
        assert_eq!(rows[1]["title"], "作品待匹配");
        assert_eq!(
            rows[2],
            json!({"id":"hidden","title":"内容已隐藏","available":false})
        );
        assert_eq!(lists[1]["works"][1], rows[0]);
        assert_eq!(
            list_with_members(&db, true).unwrap()[1]["works"][0]["title"],
            "私密标题"
        );
        assert!(db.list_works("all", None).unwrap().is_empty());
    }

    #[test]
    fn one_directory_and_format_follow_preferences_and_resume_ties() {
        let groups = [
            ("特典/voice".into(), "wav".into()),
            ("本篇/SEあり".into(), "wav".into()),
            ("本篇/SEなし".into(), "wav".into()),
            ("本篇/SEなし".into(), "mp3".into()),
        ];
        let prefs = FilePreferences {
            effects: Effects::Without,
            format: Format::Mp3,
        };
        assert_eq!(
            preferred_group(&groups, &prefs, Some(&groups[1])),
            Some(&groups[3])
        );
        assert_eq!(
            preferred_group(&groups, &FilePreferences::default(), Some(&groups[2])),
            Some(&groups[2])
        );
    }
}

/// Collect lightweight work cards and resolve each distinct pinned version once across all groups.
fn list_with_members(db: &Database, nsfw: bool) -> AppResult<Vec<Value>> {
    let groups = db.list_playlists()?;
    let work_ids: BTreeSet<_> = groups
        .iter()
        .flat_map(|p| p.work_items.iter().flatten().map(|w| w.id.clone()))
        .collect();
    let work_ids: Vec<_> = work_ids.into_iter().collect();
    let cards: HashMap<_, _> = db
        .playlist_work_summaries(&work_ids)?
        .into_iter()
        .map(|w| {
            let card = if w.nsfw && !nsfw {
                json!({"id":w.id, "title":"内容已隐藏", "available":false})
            } else {
                json!({
                    "id":w.id, "title":w.title, "coverRef":w.cover_ref, "openable":true,
                    "available":w.has_online || w.availability == "available",
                    "lastPositionMs":w.last_position_ms,
                    "completed":matches!(w.listening_status, ListeningStatus::Played)
                })
            };
            (w.id, card)
        })
        .collect();

    let version_ids: BTreeSet<_> = groups
        .iter()
        .flat_map(|p| p.version_ids.iter().cloned())
        .collect();
    let track_ids: Vec<_> = version_ids.into_iter().collect();
    let mut tracks = HashMap::new();
    if !track_ids.is_empty() {
        for track in db.playlist_tracks(Some(&track_ids), nsfw)? {
            tracks.insert(track.version_id.clone(), track);
        }
    }

    Ok(groups.into_iter().map(|group| {
        let works: Vec<_> = group.work_items.iter().flatten().map(|w| {
            cards.get(&w.id).cloned().unwrap_or_else(|| json!({"id":w.id,"title":"作品待匹配","available":false}))
        }).collect();
        let rows: Vec<_> = group.version_ids.iter().filter_map(|id| tracks.get(id)).collect();
        json!({
            "id":group.id, "name":group.name, "sortOrder":group.sort_order,
            "versionIds":group.version_ids, "workItems":group.work_items, "works":works, "tracks":rows
        })
    }).collect())
}

#[tauri::command]
pub async fn playlist_command(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action: String,
    input: Option<Value>,
) -> AppResult<Value> {
    let input = input.unwrap_or(json!({}));
    if action == "cacheWork" {
        return Ok(json!(
            state
                .provider
                .collection_work_id(string(&input, "sourceWorkId")?)
                .await?
        ));
    }
    if action == "resolveWork" {
        let id = if let Some(source) = input["sourceWorkId"].as_str() {
            state.provider.open_online_work(source).await?.id
        } else {
            string(&input, "workId")?.into()
        };
        let mut detail = state.db.get_work_detail(&id)?;
        if detail.tracks.is_empty() && !detail.online_sources.is_empty() {
            state.provider.ensure_work_online_tracks(&id, None).await?;
            detail = state.db.get_work_detail(&id)?;
        }
        return Ok(json!(PlaybackService::select_preferred_tracks_for_work(
            &detail
        )
        .into_iter()
        .map(|(v, _)| v)
        .collect::<Vec<_>>()));
    }
    if action == "play" || action == "append" {
        let group = state
            .db
            .list_playlists()?
            .into_iter()
            .find(|p| Some(p.id.as_str()) == input["id"].as_str())
            .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "歌单不存在"))?;
        let mut playable = Vec::new();
        if let Some(works) = group.work_items {
            let prefs = crate::services::file_preferences::read(&state.db)?;
            for w in works {
                let detail = match state.db.get_work_detail(&w.id) {
                    Err(e) if e.code == ErrorCode::SourceNotFound => continue,
                    other => other?,
                };
                if detail.nsfw && !state.provider.nsfw_enabled() {
                    continue;
                }
                let local = detail
                    .tracks
                    .iter()
                    .flat_map(|t| &t.versions)
                    .flat_map(|v| &v.sources)
                    .any(|s| s.source_kind != "online_provider" && s.availability == "available");
                let source = detail
                    .online_sources
                    .iter()
                    .find(|s| s.provider == "asmr_one");
                let detail = if let Some(source) = source.filter(|_| !local) {
                    // Sync can restore just one saved track. Only a cached tree describes a whole work.
                    let complete=state.db.with_conn(|c|Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM work_sources WHERE work_id=?1 AND provider='asmr_one' AND length(cached_tree_json)>2)",[&w.id],|r|r.get::<_,bool>(0))?))?;
                    if complete && !detail.tracks.is_empty() {
                        detail
                    } else {
                        state
                            .provider
                            .open_online_work_for_detail(&source.source_work_id)
                            .await?
                    }
                } else {
                    detail
                };
                if state.provider.nsfw_enabled() || !detail.nsfw {
                    playable.extend(collection_versions(detail, &w, &prefs));
                }
            }
        } else {
            playable = state
                .db
                .playlist_tracks(Some(&group.version_ids), state.provider.nsfw_enabled())?
                .into_iter()
                .filter(|t| t.available)
                .map(|t| (t.version_id, None))
                .collect();
        }
        return Ok(json!(
            state
                .playback
                .play_playlist(
                    playable,
                    action == "append",
                    input["startVersionId"].as_str().map(str::to_owned)
                )
                .await?
        ));
    }
    let db = state.db.clone();
    let nsfw = state.provider.nsfw_enabled();
    let mutate = !matches!(action.as_str(), "list" | "groups" | "tracks" | "works");
    let result = tauri::async_runtime::spawn_blocking(move || -> AppResult<Value> {
        match action.as_str() {
            "groups" => Ok(json!(db.list_playlists()?)),
            "list" => Ok(json!(list_with_members(&db, nsfw)?)),
            "tracks" => Ok(json!(db
                .playlist_tracks(None, nsfw)?
                .into_iter()
                .filter(|t| t.available)
                .collect::<Vec<_>>())),
            "works" => Ok(json!(db
                .list_works("all", None)?
                .into_iter()
                .filter(|w| nsfw || !w.nsfw)
                .map(|w| json!({"id":w.id,"title":w.title,"coverRef":w.cover_ref}))
                .collect::<Vec<_>>())),
            "create" => {
                let name = string(&input, "name")?;
                let p = match works(&input)? {
                    Some(items) => db.create_typed_playlist(name, vec![], Some(items))?,
                    None => db.create_playlist(name, ids(&input)?)?,
                };
                Ok(json!(p))
            }
            "changeMembers" => {
                let remove: Vec<String> =
                    serde_json::from_value(input.get("remove").cloned().unwrap_or(json!([])))
                        .map_err(|_| invalid("移出项目无效"))?;
                let expected: Vec<String> =
                    serde_json::from_value(input.get("expected").cloned().unwrap_or(json!([])))
                        .map_err(|_| invalid("顺序无效"))?;
                let movement = input["target"].as_u64().map(|index| {
                    (
                        input["itemId"].as_str().unwrap_or(""),
                        index as usize,
                        expected.as_slice(),
                    )
                });
                Ok(json!(db.change_members(
                    string(&input, "id")?,
                    works(&input)?.as_deref(),
                    &ids(&input)?,
                    &remove,
                    movement
                )?))
            }
            "delete" => {
                db.delete_playlist(string(&input, "id")?)?;
                Ok(Value::Null)
            }
            "update" => {
                let expected = input["expected"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                });
                Ok(json!(db.update_playlist(
                    string(&input, "id")?,
                    input["name"].as_str(),
                    &ids(&input)?,
                    input["versionId"].as_str(),
                    input["target"].as_u64().map(|v| v as usize),
                    expected.as_deref(),
                    input["groupDelta"].as_i64()
                )?))
            }
            _ => Err(invalid("未知歌单操作")),
        }
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::InternalError, "歌单任务中断"))??;
    if mutate {
        let _ = app.emit("playlists:updated", ());
    }
    Ok(result)
}
