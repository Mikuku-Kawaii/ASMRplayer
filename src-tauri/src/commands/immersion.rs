use super::AppState;
use crate::{domain::AppResult, services::immersion::ImmersionSettings};
use tauri::State;

#[tauri::command]
pub async fn immersion_command(
    state: State<'_, AppState>,
    settings: Option<ImmersionSettings>,
) -> AppResult<ImmersionSettings> {
    match settings {
        Some(settings) => state.playback.update_immersion_settings(settings).await,
        None => Ok(state.playback.immersion_settings()),
    }
}
