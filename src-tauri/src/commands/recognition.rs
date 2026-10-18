use super::AppState;
use crate::{
    domain::{AppError, AppResult, ErrorCode},
    services::recognition::{AvailableModel, RecognitionSettings, RecognitionSnapshot},
};
use tauri::State;

#[tauri::command]
pub async fn list_local_scripts(state: State<'_, AppState>, version_id: String) -> AppResult<Vec<crate::services::script_alignment::LocalScript>> {
    let service=state.subtitle_tasks.clone();
    service.list_work_scripts(&version_id).await
}
#[tauri::command]
pub async fn read_online_script(state: State<'_, AppState>, version_id: String, attachment_id: String) -> AppResult<String> {
    state.subtitle_tasks.read_online_script(&version_id,&attachment_id).await
}
#[tauri::command]
pub async fn read_local_script(path: String) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || crate::services::script_alignment::read_local_script(std::path::Path::new(&path))).await
        .map_err(|_|crate::domain::AppError::new(crate::domain::ErrorCode::InternalError,"无法读取台本"))?
}

#[tauri::command]
pub async fn preview_script_alignment(state: State<'_, AppState>, version_id: String, script: String) -> AppResult<crate::services::script_alignment::ScriptPreview> {
    let service=state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || service.preview_script_alignment(&version_id,&script)).await
        .map_err(|_|crate::domain::AppError::new(crate::domain::ErrorCode::InternalError,"台本对齐失败"))?
}
#[tauri::command]
pub async fn export_script_alignment(state: State<'_, AppState>, version_id: String, script: String, revision: String, target_path: String) -> AppResult<usize> {
    let service=state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || service.export_script_alignment(&version_id,&script,&revision,std::path::Path::new(&target_path))).await
        .map_err(|_|crate::domain::AppError::new(crate::domain::ErrorCode::InternalError,"无法导出台本字幕"))?
}

#[tauri::command]
pub async fn import_local_subtitle(state: State<'_, AppState>, version_id: String, path: String) -> AppResult<()> {
    let service = state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || service.import_local_subtitle(&version_id, std::path::Path::new(&path))).await
        .map_err(|_| crate::domain::AppError::new(crate::domain::ErrorCode::InternalError, "无法加载本地字幕"))?
}

#[tauri::command]
pub async fn preview_subtitle_sync(state: State<'_, AppState>, version_ids: Vec<String>) -> AppResult<Vec<crate::services::subtitle_sync::SubtitleSyncCandidate>> {
    let service = state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || service.preview_sync(&version_ids)).await
        .map_err(|_| crate::domain::AppError::new(crate::domain::ErrorCode::InternalError, "无法预览字幕同步"))?
}
#[tauri::command]
pub async fn sync_recognized_subtitles(state: State<'_, AppState>, candidates: Vec<crate::services::subtitle_sync::SubtitleSyncCandidate>) -> AppResult<usize> {
    let service = state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || service.sync_subtitles(&candidates)).await
        .map_err(|_| crate::domain::AppError::new(crate::domain::ErrorCode::InternalError, "无法同步字幕"))?
}

#[tauri::command]
pub fn save_subtitle_preferences(
    state: State<'_, AppState>,
    preferences: crate::services::subtitle_preferences::SubtitlePreferences,
) -> AppResult<crate::services::subtitle_preferences::SubtitlePreferences> {
    state.recognition.save_preferences(preferences)
}
#[tauri::command]
pub fn get_subtitle_work_context(
    state: State<'_, AppState>,
) -> AppResult<crate::services::subtitle_preferences::SubtitleWorkContext> {
    state.subtitle_tasks.subtitle_work_context(None)
}
#[tauri::command]
pub fn add_subtitle_glossary_term(
    state: State<'_, AppState>,
    version_id: String,
    source: String,
    target: String,
) -> AppResult<()> {
    state.subtitle_tasks.add_subtitle_term(
        &version_id,
        crate::services::subtitle_preferences::Term {
            source: source.trim().into(),
            target: target.trim().into(), ..Default::default()
        },
    )
}
#[tauri::command]
pub fn apply_subtitle_correction_rules(
    state: State<'_, AppState>,
    version_id: String,
    undo: bool,
) -> AppResult<usize> {
    state
        .subtitle_tasks
        .apply_current_subtitle_rules(&version_id, undo)
}

#[tauri::command]
pub async fn get_editable_subtitle(
    state: State<'_, AppState>,
    version_id: String,
) -> AppResult<crate::services::subtitle_documents::EditableDocument> {
    let svc = state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || svc.editable_document(&version_id))
        .await
        .map_err(|_| {
            crate::domain::AppError::new(
                crate::domain::ErrorCode::InternalError,
                "无法读取字幕文档",
            )
        })?
}
#[tauri::command]
pub async fn edit_subtitle_cue(
    state: State<'_, AppState>,
    edit: crate::services::subtitle_documents::SubtitleEdit,
) -> AppResult<crate::services::subtitle_documents::EditableDocument> {
    let svc = state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || svc.edit_document(edit))
        .await
        .map_err(|_| {
            crate::domain::AppError::new(
                crate::domain::ErrorCode::InternalError,
                "无法保存字幕修正",
            )
        })?
}
#[tauri::command]
pub async fn export_subtitle_document(
    state: State<'_, AppState>,
    version_id: String,
    revision: String,
    format: crate::services::subtitle_documents::SubtitleFormat,
    mode: crate::services::subtitle_documents::SubtitleMode,
    target_path: String,
) -> AppResult<crate::services::subtitle_documents::SubtitleExportResult> {
    let svc = state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || {
        svc.export_document(
            &version_id,
            &revision,
            format,
            mode,
            std::path::Path::new(&target_path),
        )
    })
    .await
    .map_err(|_| {
        crate::domain::AppError::new(crate::domain::ErrorCode::InternalError, "无法导出字幕")
    })?
}

#[tauri::command]
pub async fn download_recognition_runtime(state: State<'_, AppState>) -> AppResult<()> {
    state.recognition.start_runtime()
}
#[tauri::command]
pub async fn delete_recognition_model(state: State<'_, AppState>, engine: String) -> AppResult<()> {
    if matches!(state.subtitle_tasks.snapshot().state.as_str(), "running" | "cancelling") {
        return Err(crate::domain::AppError::new(crate::domain::ErrorCode::InvalidInput, "请先取消识别或翻译任务再删除模型"));
    }
    let service = state.recognition.clone();
    tauri::async_runtime::spawn_blocking(move || service.delete_recognition_model(&engine)).await
        .map_err(|_| crate::domain::AppError::new(crate::domain::ErrorCode::InternalError, "删除模型任务失败"))?
}
#[tauri::command]
pub async fn delete_ollama_model(state: State<'_, AppState>, base: String, model: String) -> AppResult<()> {
    if matches!(state.subtitle_tasks.snapshot().state.as_str(), "running" | "cancelling") {
        return Err(crate::domain::AppError::new(crate::domain::ErrorCode::InvalidInput, "请先取消识别或翻译任务再删除模型"));
    }
    state.recognition.delete_ollama_model(base, model).await
}
#[tauri::command]
pub fn download_sensevoice_fp16(state: State<'_, AppState>) -> AppResult<()> {
    state.recognition.start_sensevoice_fp16()
}
#[tauri::command]
pub async fn check_sensevoice_gpu(state: State<'_, AppState>) -> AppResult<String> {
    let service = state.recognition.clone();
    tauri::async_runtime::spawn_blocking(move || service.check_sensevoice_gpu()).await
        .map_err(|_| crate::domain::AppError::new(crate::domain::ErrorCode::InternalError, "GPU 检测任务失败"))?
}
#[tauri::command]
pub fn get_subtitle_task(
    state: State<'_, AppState>,
) -> crate::services::subtitle_tasks::SubtitleTaskSnapshot {
    state.subtitle_tasks.snapshot()
}
#[tauri::command]
pub async fn start_subtitle_task(
    state: State<'_, AppState>,
    version_id: String,
    translate: bool,
) -> AppResult<crate::services::subtitle_tasks::SubtitleTaskSnapshot> {
    state.subtitle_tasks.start(version_id, translate)
}
#[tauri::command]
pub async fn get_subtitle_readiness(
    state: State<'_, AppState>,
    version_ids: Vec<String>,
    translate: Option<bool>,
) -> AppResult<Vec<crate::services::subtitle_tasks::SubtitleReadiness>> {
    if translate == Some(true) {
        state.recognition.prepare_translation_service(&state.recognition.settings()?).await?;
    }
    state.subtitle_tasks.readiness(&version_ids).await
}
#[tauri::command]
pub async fn prepare_subtitle_tasks(
    state: State<'_, AppState>,
    version_ids: Vec<String>,
) -> AppResult<Vec<String>> {
    state.subtitle_tasks.prepare(&version_ids).await
}
#[tauri::command]
pub async fn inspect_subtitle_tasks(state: State<'_, AppState>, version_ids: Vec<String>) -> AppResult<serde_json::Value> {
    state.subtitle_tasks.inspect(&version_ids).await
}
#[tauri::command]
pub async fn delete_generated_subtitles(state: State<'_, AppState>, version_ids: Vec<String>, translate: bool) -> AppResult<()> {
    let service = state.subtitle_tasks.clone();
    tauri::async_runtime::spawn_blocking(move || service.delete_generated(&version_ids, translate)).await
        .map_err(|e| AppError::new(ErrorCode::InternalError, e.to_string()))?
}
#[tauri::command]
pub async fn start_subtitle_tasks(
    state: State<'_, AppState>,
    version_ids: Vec<String>,
    translate: bool,
    confirmed_chinese: bool,
    restart: Option<bool>,
) -> AppResult<crate::services::subtitle_tasks::SubtitleTaskSnapshot> {
    state.subtitle_tasks.prepare(&version_ids).await?;
    state
        .subtitle_tasks
        .start_batch_mode(version_ids, translate, confirmed_chinese, false, restart)
}
#[tauri::command]
pub fn cancel_subtitle_task(state: State<'_, AppState>) {
    state.subtitle_tasks.cancel();
}

#[tauri::command]
pub fn get_recognition_snapshot(state: State<'_, AppState>) -> AppResult<RecognitionSnapshot> {
    state.recognition.snapshot()
}
#[tauri::command]
pub async fn save_recognition_settings(
    state: State<'_, AppState>,
    mut settings: RecognitionSettings,
) -> AppResult<RecognitionSettings> {
    // Preferences have a dedicated save action; unrelated controls must not overwrite them.
    settings.preferences = state.recognition.settings()?.preferences;
    if settings.auto_translation {
        state.recognition.prepare_translation_service(&settings).await?;
    }
    state.recognition.save_settings(settings)
}
#[tauri::command]
pub fn save_deepseek_key(state: State<'_, AppState>, key: String) -> AppResult<()> {
    state.recognition.save_key(key)
}
#[tauri::command]
pub fn clear_deepseek_key(state: State<'_, AppState>) -> AppResult<()> {
    state.recognition.clear_key()
}
#[tauri::command]
pub async fn check_deepseek_connection(
    state: State<'_, AppState>,
) -> AppResult<Vec<AvailableModel>> {
    state.recognition.check_deepseek().await
}
#[tauri::command]
pub async fn check_ollama_connection(
    state: State<'_, AppState>,
    base: String,
) -> AppResult<Vec<AvailableModel>> {
    state.recognition.check_ollama(base).await
}
#[tauri::command]
pub async fn install_ollama(state: State<'_, AppState>, base: String) -> AppResult<()> {
    state.recognition.ollama_installer.start(base)
}
#[tauri::command]
pub async fn download_recognition_model(state: State<'_, AppState>) -> AppResult<()> {
    state.recognition.start_recognition(None)
}
#[tauri::command]
pub fn download_whisper_model(state: State<'_, AppState>) -> AppResult<()> {
    state.recognition.start_whisper()
}
#[tauri::command]
pub fn download_ct2_model(state: State<'_, AppState>, engine: String) -> AppResult<()> {
    state.recognition.start_ct2(engine)
}
#[tauri::command]
pub async fn check_ct2_model(state: State<'_, AppState>, engine: String) -> AppResult<String> {
    let service=state.recognition.clone();
    tauri::async_runtime::spawn_blocking(move ||service.check_ct2(&engine)).await
        .map_err(|_|crate::domain::AppError::new(crate::domain::ErrorCode::InternalError,"识别模型检测失败"))?
}
#[tauri::command]
pub async fn import_recognition_model(
    state: State<'_, AppState>,
    directory: String,
) -> AppResult<()> {
    state.recognition.start_recognition(Some(directory.into()))
}
#[tauri::command]
pub async fn download_translation_model(state: State<'_, AppState>) -> AppResult<()> {
    state.recognition.start_translation()
}
#[tauri::command]
pub fn download_translation_model_alternative(state: State<'_, AppState>) -> AppResult<()> {
    state.recognition.start_translation_alternative()
}
#[tauri::command]
pub fn import_translation_gguf(state: State<'_, AppState>, path: String) -> AppResult<()> {
    state.recognition.start_translation_import(path.into())
}
#[tauri::command]
pub fn cancel_model_download(state: State<'_, AppState>, kind: String) -> AppResult<()> {
    state.recognition.cancel(&kind)
}
