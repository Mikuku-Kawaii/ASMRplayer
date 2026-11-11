use super::AppState;
use crate::{
    domain::{AppError, AppResult, ErrorCode},
    services::device_sync::codec,
};
use serde_json::Value;
use tauri::State;
#[tauri::command]
pub fn supabase_sync_reply(window: tauri::WebviewWindow, id: String, result: Value) {
    if window.label() == "main" {
        crate::services::device_sync::providers::supabase::reply(&id,result);
    }
}
#[tauri::command]
pub async fn device_sync_command(
    state: State<'_, AppState>,
    action: String,
    input: Option<Value>,
    provider: Option<crate::services::device_sync::providers::ProviderId>,
) -> AppResult<Value> {
    let service = state.device_sync.clone();
    if action == "resume" {
        let id=input.as_ref().and_then(|v|v["id"].as_str()).unwrap_or("").to_owned();
        let reader=service.clone();let candidate_id=id.clone();
        let candidate=tauri::async_runtime::spawn_blocking(move || reader.resume_candidate(provider,Some(&candidate_id))).await
            .map_err(|_| AppError::new(ErrorCode::InternalError,"无法读取接续位置"))??
            .ok_or_else(|| AppError::new(ErrorCode::InvalidInput,"此位置已失效或已在本机继续收听"))?;
        let snapshot=state.playback.resume_synced(candidate["versionId"].as_str().unwrap_or(""),candidate["positionMs"].as_i64().unwrap_or(0)).await?;
        service.dismiss_resume(provider,&id)?;
        return codec::to_value(snapshot);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let input = input.unwrap_or(Value::Null);
        match action.as_str() {
            "githubStart" => codec::to_value(service.github_start()?),
            "githubCancel" => {
                service.github_cancel();
                Ok(Value::Null)
            }
            "login" => codec::to_value(service.login(provider, &input)?),
            "logout" => codec::to_value(service.logout(provider)?),
            "sync" => codec::to_value(service.run(provider)?),
            "select" => codec::to_value(service.select(codec::from_value(input)?)?),
            "dismissResume" => { service.dismiss_resume(provider,input["id"].as_str().unwrap_or(""))?; codec::to_value(service.status()?) },
            "status" => codec::to_value(service.status()?),
            "autoPolicy" => {
                codec::to_value(service.set_auto_policy(codec::from_value(input)?, provider)?)
            }
            "resolve" => Ok(crate::services::device_sync::codec::to_value(
                service.resolve(
                    input["key"].as_str().unwrap_or(""),
                    input["device"].as_str().unwrap_or(""),
                    input["seq"].as_u64().unwrap_or(0),
                )?,
            )?),
            _ => Err(AppError::new(ErrorCode::InvalidInput, "未知同步操作")),
        }
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::InternalError, "同步任务中断"))?
}
