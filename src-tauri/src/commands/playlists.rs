use super::AppState;
use crate::domain::{AppError, AppResult, ErrorCode};
use crate::services::playback::PlaybackService;
use serde_json::{json, Value};
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

#[tauri::command]
pub async fn playlist_command(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action: String,
    input: Option<Value>,
) -> AppResult<Value> {
    let input = input.unwrap_or(json!({}));
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
        let playable = state
            .db
            .playlist_tracks(Some(&group.version_ids), state.provider.nsfw_enabled())?
            .into_iter()
            .filter(|t| t.available)
            .map(|t| t.version_id)
            .collect();
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
    let mutate = !matches!(action.as_str(), "list" | "tracks");
    let result=tauri::async_runtime::spawn_blocking(move||->AppResult<Value>{match action.as_str(){
        "list"=>{let groups=db.list_playlists()?;let mut out=Vec::new();for group in groups{let tracks=db.playlist_tracks(Some(&group.version_ids),nsfw)?;out.push(json!({"id":group.id,"name":group.name,"versionIds":group.version_ids,"sortOrder":group.sort_order,"tracks":tracks}));}Ok(json!(out))},
        "tracks"=>Ok(json!(db.playlist_tracks(None,nsfw)?.into_iter().filter(|t|t.available).collect::<Vec<_>>())),
        "create"=>Ok(json!(db.create_playlist(string(&input,"name")?,ids(&input)?)?)),
        "delete"=>{db.delete_playlist(string(&input,"id")?)?;Ok(Value::Null)},
        "update"=>{let expected=input["expected"].as_array().map(|a|a.iter().filter_map(|v|v.as_str().map(str::to_owned)).collect::<Vec<_>>());Ok(json!(db.update_playlist(string(&input,"id")?,input["name"].as_str(),&ids(&input)?,input["versionId"].as_str(),input["target"].as_u64().map(|v|v as usize),expected.as_deref(),input["groupDelta"].as_i64())?))},
        _=>Err(AppError::new(ErrorCode::InvalidInput,"未知歌单操作")),
    }}).await.map_err(|_|AppError::new(ErrorCode::InternalError,"歌单任务中断"))??;
    if mutate {
        let _ = app.emit("playlists:updated", ());
    }
    Ok(result)
}
