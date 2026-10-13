use crate::db::Database;
use crate::domain::*;
use crate::providers::{AsmrOneProvider, ProviderWork};
use crate::services::download::DownloadService;
use crate::services::library::LibraryService;
use crate::services::playback::PlaybackService;
use crate::services::session::SessionService;
use crate::services::updater::{UpdateService, UpdateSnapshot};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub db: Database,
    pub library: LibraryService,
    pub provider: AsmrOneProvider,
    pub playback: PlaybackService,
    pub downloads: DownloadService,
    pub session: SessionService,
    pub updater: UpdateService,
    pub island: crate::services::island::IslandService,
}

#[tauri::command]
pub fn get_island_preferences(state: State<'_, AppState>) -> AppResult<crate::services::island::IslandPreferences> { state.island.preferences() }

#[tauri::command]
pub async fn set_island_preferences(state: State<'_, AppState>, size: Option<crate::services::island::IslandSize>, monitor_id: Option<String>) -> AppResult<crate::services::island::IslandPreferences> { state.island.set_preferences(size, monitor_id) }

#[tauri::command]
pub async fn island_set_form(state: State<'_, AppState>, form: crate::services::island::IslandForm, animate: Option<bool>, compact_width: Option<f64>) -> AppResult<crate::services::island::IslandLayout> {
    let island = state.island.clone();
    tokio::task::spawn_blocking(move || island.layout_form(form, animate.unwrap_or(true), compact_width)).await
        .map_err(|e| AppError::new(ErrorCode::InternalError, format!("灵动岛过渡失败：{e}")))?
}

#[tauri::command]
pub async fn island_settle_layout(state: State<'_, AppState>, revision: u64) -> AppResult<()> { state.island.settle(revision) }

#[tauri::command]
pub async fn island_save_current_position(state: State<'_, AppState>) -> AppResult<()> { state.island.save_position() }

#[tauri::command]
pub fn get_update_snapshot(state: State<'_, AppState>) -> UpdateSnapshot { state.updater.snapshot() }

#[tauri::command]
pub async fn check_for_updates(state: State<'_, AppState>) -> AppResult<UpdateSnapshot> { state.updater.check(true).await }

#[tauri::command]
pub fn set_update_auto_check(state: State<'_, AppState>, enabled: bool) -> AppResult<UpdateSnapshot> { state.updater.set_auto_check(enabled) }

#[tauri::command]
pub async fn download_update(state: State<'_, AppState>) -> AppResult<UpdateSnapshot> { state.updater.download().await }

#[tauri::command]
pub fn cancel_update_download(state: State<'_, AppState>) -> UpdateSnapshot { state.updater.cancel_download() }

#[tauri::command]
pub async fn dismiss_update(state: State<'_, AppState>, skip: bool) -> AppResult<UpdateSnapshot> { state.updater.dismiss(skip).await }

#[tauri::command]
pub async fn install_update(state: State<'_, AppState>) -> AppResult<UpdateSnapshot> { state.updater.install(&state.playback).await }

#[tauri::command]
pub fn open_release_page() -> AppResult<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .arg(crate::services::updater::RELEASES_URL).creation_flags(0x08000000).spawn()?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_works(
    state: State<'_, AppState>,
    filter: String,
    search: Option<String>,
) -> AppResult<Vec<WorkSummary>> {
    state.db.list_works(&filter, search.as_deref())
}

#[tauri::command]
pub fn get_work_detail(state: State<'_, AppState>, work_id: String) -> AppResult<WorkDetailDto> {
    state.db.get_work_detail(&work_id)
}

#[tauri::command]
pub fn toggle_favorite(state: State<'_, AppState>, work_id: String) -> AppResult<bool> {
    state.db.toggle_favorite(&work_id)
}

#[tauri::command]
pub fn toggle_collected(state: State<'_, AppState>, work_id: String) -> AppResult<bool> {
    state.db.toggle_collected(&work_id)
}

#[tauri::command]
pub fn set_work_override(
    state: State<'_, AppState>,
    work_id: String,
    field: String,
    override_mode: String,
    value_text: Option<String>,
) -> AppResult<WorkDetailDto> {
    state
        .db
        .set_work_override(&work_id, &field, &override_mode, value_text.as_deref())?;
    state.db.get_work_detail(&work_id)
}

#[tauri::command]
pub fn reset_work_override(
    state: State<'_, AppState>,
    work_id: String,
    field: String,
) -> AppResult<WorkDetailDto> {
    state.db.reset_work_override(&work_id, &field)?;
    state.db.get_work_detail(&work_id)
}

#[tauri::command]
pub fn list_library_roots(state: State<'_, AppState>) -> AppResult<Vec<LibraryRootDto>> {
    state.library.list_roots()
}

#[tauri::command]
pub fn preview_import(
    state: State<'_, AppState>,
    root_path: String,
    root_mode: String,
) -> AppResult<ImportPreviewDto> {
    state.library.preview_import(&root_path, &root_mode)
}

#[tauri::command]
pub fn preview_smart_import(
    state: State<'_, AppState>,
    selected_path: String,
    shift_level: Option<i32>,
) -> AppResult<ImportPreviewDto> {
    state
        .library
        .preview_smart_import(&selected_path, shift_level.unwrap_or(0))
}

#[tauri::command]
pub async fn scan_and_import(
    state: State<'_, AppState>,
    root_path: String,
    root_mode: String,
    selected_candidates: Option<Vec<(String, String)>>,
) -> AppResult<ScanReportDto> {
    let lib = state.library.clone();
    let report = tokio::task::spawn_blocking(move || {
        lib.scan_and_import(&root_path, &root_mode, selected_candidates)
    })
    .await
    .map_err(|e| AppError::new(ErrorCode::InternalError, e.to_string()))??;

    if !report.canceled {
        let _ = state.provider.auto_enrich_local_works().await;
    }

    Ok(report)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) -> AppResult<()> {
    state.library.cancel_scan();
    Ok(())
}

#[tauri::command]
pub fn relocate_library_root(
    state: State<'_, AppState>,
    root_id: String,
    new_path: String,
) -> AppResult<LibraryRootDto> {
    state.library.relocate_root(&root_id, &new_path)
}

#[tauri::command]
pub async fn open_online_work(
    state: State<'_, AppState>,
    work_id_or_rj: String,
) -> AppResult<WorkDetailDto> {
    state.provider.open_online_work(&work_id_or_rj).await
}

#[tauri::command]
pub async fn sync_work_from_asmr_one(
    state: State<'_, AppState>,
    work_id: String,
    rj_or_id: Option<String>,
    prefer_online_cover: Option<bool>,
) -> AppResult<WorkDetailDto> {
    state
        .provider
        .sync_work_from_asmr_one(
            &work_id,
            rj_or_id.as_deref(),
            prefer_online_cover.unwrap_or(false),
        )
        .await
}

#[tauri::command]
pub async fn browse_online_works(
    state: State<'_, AppState>,
    page: u32,
    order: String,
    sort: String,
    subtitle_only: bool,
) -> AppResult<Page<ProviderWork>> {
    state
        .provider
        .browse_works(page, &order, &sort, subtitle_only)
        .await
}

#[tauri::command]
pub async fn search_online_works(
    state: State<'_, AppState>,
    keyword: String,
    page: u32,
    order: String,
    sort: String,
    subtitle_only: bool,
) -> AppResult<Page<ProviderWork>> {
    state
        .provider
        .search_works(&keyword, page, &order, &sort, subtitle_only)
        .await
}

#[tauri::command]
pub fn get_playback_snapshot(state: State<'_, AppState>) -> AppResult<PlaybackSnapshot> {
    state.playback.get_snapshot()
}

#[tauri::command]
pub async fn play_version(
    state: State<'_, AppState>,
    version_id: String,
    preferred_source_id: Option<String>,
) -> AppResult<PlaybackSnapshot> {
    state.playback.play(&version_id, preferred_source_id).await
}

#[tauri::command]
pub async fn play_directory_group(
    state: State<'_, AppState>,
    work_id: String,
    group_path: String,
    start_version_id: Option<String>,
) -> AppResult<PlaybackSnapshot> {
    state
        .playback
        .play_directory_group(&work_id, &group_path, start_version_id)
        .await
}

#[tauri::command]
pub async fn play_favorites_queue(
    state: State<'_, AppState>,
    work_ids: Option<Vec<String>>,
) -> AppResult<PlaybackSnapshot> {
    state.playback.play_favorites_queue(work_ids).await
}

#[tauri::command]
pub async fn toggle_pause(state: State<'_, AppState>) -> AppResult<PlaybackSnapshot> {
    state.playback.toggle_pause().await
}

#[tauri::command]
pub async fn seek_ms(state: State<'_, AppState>, position_ms: i64) -> AppResult<PlaybackSnapshot> {
    state.playback.seek_ms(position_ms).await
}

#[tauri::command]
pub async fn set_volume(state: State<'_, AppState>, volume: f64) -> AppResult<PlaybackSnapshot> {
    state.playback.set_volume(volume).await
}

#[tauri::command]
pub fn set_loop_mode(state: State<'_, AppState>, loop_mode: String) -> AppResult<PlaybackSnapshot> {
    state.playback.set_loop_mode(&loop_mode)
}

#[tauri::command]
pub async fn step_queue(state: State<'_, AppState>, delta: isize) -> AppResult<PlaybackSnapshot> {
    state.playback.step_queue(delta).await
}

#[tauri::command]
pub fn append_to_queue(
    state: State<'_, AppState>,
    version_id: String,
    preferred_source_id: Option<String>,
) -> AppResult<PlaybackSnapshot> {
    state
        .db
        .append_queue_items(&[(version_id, preferred_source_id)])?;
    state.playback.emit_snapshot();
    state.playback.get_snapshot()
}

#[tauri::command]
pub fn remove_from_queue(
    state: State<'_, AppState>,
    queue_item_id: String,
) -> AppResult<PlaybackSnapshot> {
    state.db.remove_queue_item(&queue_item_id)?;
    state.playback.emit_snapshot();
    state.playback.get_snapshot()
}

#[tauri::command]
pub fn move_in_queue(
    state: State<'_, AppState>,
    queue_item_id: String,
    target_index: usize,
) -> AppResult<PlaybackSnapshot> {
    state.db.move_queue_item(&queue_item_id, target_index)?;
    state.playback.emit_snapshot();
    state.playback.get_snapshot()
}

#[tauri::command]
pub fn clear_queue(state: State<'_, AppState>) -> AppResult<PlaybackSnapshot> {
    state.db.clear_queue_items()?;
    state.playback.on_queue_cleared();
    state.playback.get_snapshot()
}

#[tauri::command]
pub async fn restart_player_engine(state: State<'_, AppState>) -> AppResult<PlaybackSnapshot> {
    state.playback.restart_engine().await
}

#[tauri::command]
pub fn set_sleep_timer(
    state: State<'_, AppState>,
    mode: String,
    minutes: Option<u32>,
    fade_out_seconds: Option<i64>,
) -> AppResult<PlaybackSnapshot> {
    state
        .playback
        .set_sleep_timer_with_fade(&mode, minutes, fade_out_seconds)
}

#[tauri::command]
pub fn set_ab_loop(
    state: State<'_, AppState>,
    start_ms: i64,
    end_ms: i64,
    label: Option<String>,
    save_to_db: bool,
) -> AppResult<PlaybackSnapshot> {
    state
        .playback
        .set_active_ab_loop(start_ms, end_ms, label, save_to_db)
}

#[tauri::command]
pub fn clear_ab_loop(state: State<'_, AppState>) -> AppResult<PlaybackSnapshot> {
    state.playback.clear_active_ab_loop()
}

#[tauri::command]
pub fn list_ab_loops(state: State<'_, AppState>, version_id: String) -> AppResult<Vec<AbLoopDto>> {
    state.db.list_ab_loops(&version_id)
}

#[tauri::command]
pub fn delete_ab_loop(state: State<'_, AppState>, loop_id: String) -> AppResult<()> {
    state.db.delete_ab_loop(&loop_id)
}

#[tauri::command]
pub fn list_bookmarks(
    state: State<'_, AppState>,
    work_id: Option<String>,
    version_id: Option<String>,
) -> AppResult<Vec<BookmarkDto>> {
    state
        .db
        .list_bookmarks(work_id.as_deref(), version_id.as_deref())
}

#[tauri::command]
pub fn add_bookmark(
    state: State<'_, AppState>,
    version_id: String,
    position_ms: i64,
    note: String,
) -> AppResult<BookmarkDto> {
    state.db.add_bookmark(&version_id, position_ms, &note)
}

#[tauri::command]
pub fn update_bookmark(
    state: State<'_, AppState>,
    bookmark_id: String,
    note: String,
) -> AppResult<()> {
    state.db.update_bookmark(&bookmark_id, &note)
}

#[tauri::command]
pub fn delete_bookmark(state: State<'_, AppState>, bookmark_id: String) -> AppResult<()> {
    state.db.delete_bookmark(&bookmark_id)
}

#[tauri::command]
pub async fn load_subtitle_cues(
    state: State<'_, AppState>,
    attachment_id: String,
) -> AppResult<Vec<SubtitleCueDto>> {
    let _ = state
        .provider
        .ensure_online_subtitle_cached(&attachment_id)
        .await;
    let cues = state.library.load_subtitle_cues(&attachment_id)?;
    state
        .playback
        .set_active_subtitle_cues_for_version(None, cues.clone());
    Ok(cues)
}

#[tauri::command]
pub async fn auto_match_subtitle_for_version(
    state: State<'_, AppState>,
    version_id: String,
) -> AppResult<Option<AutoMatchedSubtitleDto>> {
    let Some(mut attachment) = state.library.find_best_subtitle_for_version(&version_id)? else {
        state
            .playback
            .set_active_subtitle_cues_for_version(Some(&version_id), Vec::new());
        return Ok(None);
    };
    if let Ok(cached_path) = state
        .provider
        .ensure_online_subtitle_cached(&attachment.id)
        .await
    {
        attachment.absolute_path = Some(cached_path);
        attachment.availability = "ready".into();
    }
    match state.library.load_subtitle_cues(&attachment.id) {
        Ok(cues) => {
            state
                .playback
                .set_active_subtitle_cues_for_version(Some(&version_id), cues.clone());
            Ok(Some(AutoMatchedSubtitleDto { attachment, cues }))
        }
        Err(_) => Ok(None),
    }
}

#[tauri::command]
pub fn bind_subtitle_to_version(
    state: State<'_, AppState>,
    attachment_id: String,
    version_id: Option<String>,
    offset_ms: i64,
) -> AppResult<()> {
    state
        .library
        .bind_subtitle_to_version(&attachment_id, version_id.as_deref(), offset_ms)?;
    if let Some(ref vid) = version_id {
        if let Ok(cues) = state.library.load_subtitle_cues(&attachment_id) {
            state
                .playback
                .set_active_subtitle_cues_for_version(Some(vid), cues);
        }
    } else {
        state
            .playback
            .set_active_subtitle_cues_for_version(None, Vec::new());
    }
    Ok(())
}

#[tauri::command]
pub async fn fetch_online_subtitles_for_work(
    state: State<'_, AppState>,
    work_id: String,
    rj_or_id: Option<String>,
) -> AppResult<WorkDetailDto> {
    let _ = state
        .provider
        .fetch_online_subtitles_for_work(&work_id, rj_or_id.as_deref())
        .await?;
    if let Ok(snap) = state.playback.get_snapshot() {
        if snap.current_work_id.as_deref() == Some(&work_id) {
            if let Some(ref cur_vid) = snap.current_version_id {
                if let Ok(Some(att)) = state.library.find_best_subtitle_for_version(cur_vid) {
                    let _ = state.provider.ensure_online_subtitle_cached(&att.id).await;
                    if let Ok(cues) = state.library.load_subtitle_cues(&att.id) {
                        state
                            .playback
                            .set_active_subtitle_cues_for_version(Some(cur_vid), cues);
                    }
                }
            }
        }
    }
    state.db.get_work_detail(&work_id)
}

#[tauri::command]
pub fn analyze_track_acoustics(
    state: State<'_, AppState>,
    version_id: String,
) -> AppResult<TrackAcousticMetricsDto> {
    state.library.analyze_track_acoustics(&version_id)
}

#[tauri::command]
pub fn preview_download(
    state: State<'_, AppState>,
    work_id: String,
    source_ids: Option<Vec<String>>,
) -> AppResult<DownloadPreviewDto> {
    state.downloads.preview_download(&work_id, source_ids)
}

#[tauri::command]
pub async fn prepare_work_download(
    state: State<'_, AppState>,
    work_id: String,
    rj_or_id: Option<String>,
) -> AppResult<DownloadPreviewDto> {
    state
        .provider
        .ensure_work_online_tracks(&work_id, rj_or_id.as_deref())
        .await?;
    state.downloads.preview_download(&work_id, None)
}

#[tauri::command]
pub async fn delete_work_from_library(
    state: State<'_, AppState>,
    work_id: String,
    delete_downloaded_files: Option<bool>,
) -> AppResult<PlaybackSnapshot> {
    if delete_downloaded_files.unwrap_or(false) {
        let _ = state.downloads.delete_downloaded_files_for_work(&work_id);
    } else {
        let _ = state.downloads.cancel_all_tasks_for_work(&work_id);
    }
    state.playback.stop_if_playing_work(&work_id).await;
    state.db.delete_work(&work_id)?;
    state.playback.emit_snapshot();
    state.playback.get_snapshot()
}

#[tauri::command]
pub fn delete_work_downloaded_files(
    state: State<'_, AppState>,
    work_id: String,
) -> AppResult<WorkDetailDto> {
    state.downloads.delete_downloaded_files_for_work(&work_id)?;
    state.db.get_work_detail(&work_id)
}

#[tauri::command]
pub fn enqueue_downloads(
    state: State<'_, AppState>,
    work_id: String,
    source_ids: Option<Vec<String>>,
) -> AppResult<Vec<DownloadTaskDto>> {
    state.downloads.enqueue_downloads(&work_id, source_ids)
}

#[tauri::command]
pub fn list_download_tasks(state: State<'_, AppState>) -> AppResult<Vec<DownloadTaskDto>> {
    state.downloads.list_tasks()
}

#[tauri::command]
pub fn pause_download_task(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<DownloadTaskDto>> {
    state.downloads.pause_task(&task_id)
}

#[tauri::command]
pub fn resume_download_task(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<DownloadTaskDto>> {
    state.downloads.resume_task(&task_id)
}

#[tauri::command]
pub fn cancel_download_task(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<DownloadTaskDto>> {
    state.downloads.cancel_task(&task_id)
}

#[tauri::command]
pub fn associate_works(
    state: State<'_, AppState>,
    local_work_id: String,
    online_work_id: String,
) -> AppResult<String> {
    state.db.associate_works(&local_work_id, &online_work_id)
}

#[tauri::command]
pub fn associate_versions(
    state: State<'_, AppState>,
    target_version_id: String,
    source_version_id: String,
) -> AppResult<String> {
    state
        .db
        .associate_versions(&target_version_id, &source_version_id)
}

#[tauri::command]
pub fn undo_association(state: State<'_, AppState>, history_id: String) -> AppResult<()> {
    state.db.undo_association(&history_id)
}

#[tauri::command]
pub fn list_association_history(
    state: State<'_, AppState>,
) -> AppResult<Vec<AssociationHistoryDto>> {
    state.db.list_association_history()
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> AppResult<AppSettingsDto> {
    state.session.get_settings()
}

#[tauri::command]
pub async fn update_app_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    download_dir: String,
    api_base_url: String,
    proxy_url: Option<String>,
    close_behavior: String,
    hardware_acceleration: Option<bool>,
    auth_token: Option<String>,
    audio_exclusive: Option<bool>,
    smtc_mode: Option<String>,
    realtime_watch_enabled: Option<bool>,
    webdav_url: Option<String>,
    nsfw_enabled: Option<bool>,
    island_fullscreen_avoidance: Option<bool>,
    island_edge_snap: Option<bool>,
    island_always_on_top: Option<bool>,
    island_scroll_volume: Option<bool>,
    island_toast_enabled: Option<bool>,
    island_toast_duration_sec: Option<f64>,
    island_privacy_mode: Option<bool>,
) -> AppResult<AppSettingsDto> {
    state.session.update_settings(
        &download_dir,
        &api_base_url,
        proxy_url,
        &close_behavior,
        hardware_acceleration,
        auth_token,
        None,
        smtc_mode,
        realtime_watch_enabled,
        webdav_url,
        nsfw_enabled,
        island_fullscreen_avoidance,
        island_edge_snap,
        island_always_on_top,
        island_scroll_volume,
        island_toast_enabled,
        island_toast_duration_sec,
        island_privacy_mode,
    )?;
    if let Some(exclusive) = audio_exclusive {
        if let Err(error) = state.playback.set_audio_exclusive(exclusive).await {
            if let Ok(settings) = state.session.get_settings() { let _ = app.emit("settings:updated", &settings); }
            return Err(error);
        }
    }
    let settings = state.session.get_settings()?;
    let _ = app.emit("settings:updated", &settings);
    Ok(settings)
}

#[tauri::command]
pub fn set_nsfw_enabled(state: State<'_, AppState>, enabled: bool) -> AppResult<bool> {
    state.provider.set_nsfw_enabled(enabled)?;
    Ok(state.provider.nsfw_enabled())
}

#[tauri::command]
pub async fn sync_watched_roots_now(
    state: State<'_, AppState>,
) -> AppResult<Vec<ScanReportDto>> {
    let lib = state.library.clone();
    tokio::task::spawn_blocking(move || lib.poll_and_sync_watched_roots())
        .await
        .map_err(|e| AppError::new(ErrorCode::InternalError, format!("目录监听同步线程失败: {}", e)))?
}

#[tauri::command]
pub fn clear_playback_cache(state: State<'_, AppState>) -> AppResult<u64> {
    state.session.clear_playback_cache()
}

#[tauri::command]
pub fn get_diagnostic_logs(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    state.session.get_diagnostics()
}

#[tauri::command]
pub fn export_user_backup(
    state: State<'_, AppState>,
    target_path: Option<String>,
) -> AppResult<UserBackupBundleDto> {
    let bundle = state.db.export_user_backup()?;
    if let Some(ref p) = target_path {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            let json_bytes = serde_json::to_vec_pretty(&bundle).map_err(|e| {
                AppError::new(
                    ErrorCode::InternalError,
                    format!("序列化用户备份失败: {}", e),
                )
            })?;
            std::fs::write(trimmed, json_bytes)?;
        }
    }
    Ok(bundle)
}

#[tauri::command]
pub async fn import_user_backup(
    state: State<'_, AppState>,
    source_path: Option<String>,
    bundle_json: Option<String>,
) -> AppResult<BackupRestoreReportDto> {
    let raw_json = if let Some(ref p) = source_path.filter(|s| !s.trim().is_empty()) {
        std::fs::read_to_string(p.trim())?
    } else if let Some(raw) = bundle_json.filter(|s| !s.trim().is_empty()) {
        raw
    } else {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "请提供有效的备份文件路径或备份 JSON 内容",
        ));
    };

    let bundle: UserBackupBundleDto = serde_json::from_str(&raw_json).map_err(|e| {
        AppError::new(
            ErrorCode::InvalidInput,
            format!("解析用户备份 JSON 失败: {}", e),
        )
    })?;

    let report = state.db.import_user_backup(&bundle)?;
    if let Ok((global_cfg, global_preset)) = state.db.get_dsp_global_state() {
        let _ = state
            .playback
            .update_dsp_config(global_cfg, global_preset)
            .await;
    }
    state.playback.emit_snapshot();
    Ok(report)
}

#[tauri::command]
pub fn export_encrypted_sync_bundle(
    state: State<'_, AppState>,
    passphrase: String,
    target_path: Option<String>,
) -> AppResult<EncryptedSyncEnvelopeDto> {
    let bundle = state.db.export_user_backup()?;
    let env = encrypt_backup_bundle(&bundle, &passphrase)?;
    if let Some(ref p) = target_path {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            let json_bytes = serde_json::to_vec_pretty(&env).map_err(|e| {
                AppError::new(
                    ErrorCode::InternalError,
                    format!("序列化加密同步包失败: {}", e),
                )
            })?;
            std::fs::write(trimmed, json_bytes)?;
        }
    }
    Ok(env)
}

#[tauri::command]
pub async fn import_encrypted_sync_bundle(
    state: State<'_, AppState>,
    passphrase: String,
    source_path: Option<String>,
    envelope_json: Option<String>,
) -> AppResult<BackupRestoreReportDto> {
    let raw_json = if let Some(ref p) = source_path.filter(|s| !s.trim().is_empty()) {
        std::fs::read_to_string(p.trim())?
    } else if let Some(raw) = envelope_json.filter(|s| !s.trim().is_empty()) {
        raw
    } else {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "请提供加密同步文件路径或密文 JSON",
        ));
    };

    let env: EncryptedSyncEnvelopeDto = serde_json::from_str(&raw_json).map_err(|e| {
        AppError::new(
            ErrorCode::InvalidInput,
            format!("解析加密同步包 JSON 结构失败: {}", e),
        )
    })?;
    let bundle = decrypt_backup_bundle(&env, &passphrase)?;
    let report = state.db.import_user_backup(&bundle)?;
    if let Ok((global_cfg, global_preset)) = state.db.get_dsp_global_state() {
        let _ = state
            .playback
            .update_dsp_config(global_cfg, global_preset)
            .await;
    }
    state.playback.emit_snapshot();
    Ok(report)
}

#[tauri::command]
pub fn get_dsp_state(
    state: State<'_, AppState>,
    work_id: Option<String>,
) -> AppResult<DspStateDto> {
    state.playback.get_dsp_state(work_id)
}

#[tauri::command]
pub async fn update_dsp_config(
    state: State<'_, AppState>,
    config: DspConfigDto,
    preset_id: Option<String>,
) -> AppResult<DspStateDto> {
    state.playback.update_dsp_config(config, preset_id).await
}

#[tauri::command]
pub async fn apply_dsp_preset(
    state: State<'_, AppState>,
    preset_id: String,
) -> AppResult<DspStateDto> {
    state.playback.apply_dsp_preset(&preset_id).await
}

#[tauri::command]
pub async fn import_autoeq_text(
    state: State<'_, AppState>,
    text: String,
) -> AppResult<DspStateDto> {
    state.playback.import_autoeq_text(&text).await
}

#[tauri::command]
pub async fn clear_autoeq_bands(state: State<'_, AppState>) -> AppResult<DspStateDto> {
    state.playback.clear_autoeq_bands().await
}

#[tauri::command]
pub async fn save_custom_dsp_preset(
    state: State<'_, AppState>,
    name: String,
    description: String,
    config: DspConfigDto,
) -> AppResult<DspStateDto> {
    state
        .playback
        .save_custom_dsp_preset(&name, &description, config)
        .await
}

#[tauri::command]
pub async fn delete_custom_dsp_preset(
    state: State<'_, AppState>,
    preset_id: String,
) -> AppResult<DspStateDto> {
    state.playback.delete_custom_dsp_preset(&preset_id).await
}

#[tauri::command]
pub async fn set_work_dsp_override_enabled(
    state: State<'_, AppState>,
    work_id: String,
    enabled: bool,
) -> AppResult<DspStateDto> {
    state
        .playback
        .set_work_dsp_override_enabled(&work_id, enabled)
        .await
}

#[tauri::command]
pub async fn set_audio_exclusive(
    state: State<'_, AppState>,
    exclusive: bool,
) -> AppResult<PlaybackSnapshot> {
    state.playback.set_audio_exclusive(exclusive).await
}

#[tauri::command]
pub fn set_smtc_mode(
    state: State<'_, AppState>,
    mode: String,
) -> AppResult<PlaybackSnapshot> {
    state.playback.set_smtc_mode(&mode)
}

#[tauri::command]
pub fn get_smtc_display_state(state: State<'_, AppState>) -> AppResult<SmtcDisplayStateDto> {
    state.playback.get_smtc_display_state()
}

#[tauri::command]
pub async fn handle_smtc_action(
    state: State<'_, AppState>,
    action: String,
    seek_ms: Option<i64>,
) -> AppResult<PlaybackSnapshot> {
    state.playback.handle_smtc_action(&action, seek_ms).await
}

const WINDOW_PLACEMENT_SETTING_KEY: &str = "window_placement_json";

pub fn capture_and_save_window_placement(window: &tauri::WebviewWindow, db: &Database) {
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let is_maximized = window.is_maximized().unwrap_or(false);

    let prev_saved = db
        .get_setting(WINDOW_PLACEMENT_SETTING_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<WindowPlacementDto>(&raw).ok());

    let current_mon = window.current_monitor().ok().flatten().or_else(|| {
        let center_x = pos.x.saturating_add((size.width as i32) / 2);
        let center_y = pos.y.saturating_add((size.height as i32) / 2);
        window
            .available_monitors()
            .ok()
            .unwrap_or_default()
            .into_iter()
            .find(|m| {
                let mp = m.position();
                let ms = m.size();
                center_x >= mp.x
                    && center_x < mp.x.saturating_add(ms.width as i32)
                    && center_y >= mp.y
                    && center_y < mp.y.saturating_add(ms.height as i32)
            })
    });

    let (mon_name, mon_x, mon_y, mon_w, mon_h) = match current_mon {
        Some(ref m) => (
            m.name().cloned(),
            Some(m.position().x),
            Some(m.position().y),
            Some(m.size().width),
            Some(m.size().height),
        ),
        None => (
            prev_saved.as_ref().and_then(|p| p.monitor_name.clone()),
            prev_saved.as_ref().and_then(|p| p.monitor_x),
            prev_saved.as_ref().and_then(|p| p.monitor_y),
            prev_saved.as_ref().and_then(|p| p.monitor_width),
            prev_saved.as_ref().and_then(|p| p.monitor_height),
        ),
    };

    let placement = if is_maximized {
        let base_w = prev_saved
            .as_ref()
            .map(|p| p.width.max(960))
            .unwrap_or(1280);
        let base_h = prev_saved
            .as_ref()
            .map(|p| p.height.max(640))
            .unwrap_or(820);
        let anchor_x = mon_x
            .map(|mx| {
                let mw = mon_w.unwrap_or(1920).max(base_w);
                mx.saturating_add(((mw as i32 - base_w as i32) / 2).max(24))
            })
            .unwrap_or(pos.x.max(-16_000));
        let anchor_y = mon_y
            .map(|my| {
                let mh = mon_h.unwrap_or(1080).max(base_h);
                my.saturating_add(((mh as i32 - base_h as i32) / 2).max(24))
            })
            .unwrap_or(pos.y.max(-16_000));
        WindowPlacementDto {
            x: anchor_x,
            y: anchor_y,
            width: base_w,
            height: base_h,
            maximized: true,
            monitor_name: mon_name,
            monitor_x: mon_x,
            monitor_y: mon_y,
            monitor_width: mon_w,
            monitor_height: mon_h,
        }
    } else if is_valid_normal_window_bounds(pos.x, pos.y, size.width, size.height, false) {
        WindowPlacementDto {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            maximized: false,
            monitor_name: mon_name,
            monitor_x: mon_x,
            monitor_y: mon_y,
            monitor_width: mon_w,
            monitor_height: mon_h,
        }
    } else {
        let Some(mut prev) = prev_saved else {
            return;
        };
        if pos.x > -25_000 && pos.y > -25_000 {
            if let (Some(mx), Some(my)) = (mon_x, mon_y) {
                let mw = mon_w.unwrap_or(1920).max(prev.width);
                let mh = mon_h.unwrap_or(1080).max(prev.height);
                prev.x = mx.saturating_add(((mw as i32 - prev.width as i32) / 2).max(0));
                prev.y = my.saturating_add(((mh as i32 - prev.height as i32) / 2).max(0));
                prev.monitor_name = mon_name;
                prev.monitor_x = Some(mx);
                prev.monitor_y = Some(my);
                prev.monitor_width = mon_w;
                prev.monitor_height = mon_h;
            }
            if let Ok(raw) = serde_json::to_string(&prev) {
                let _ = db.set_setting(WINDOW_PLACEMENT_SETTING_KEY, &raw);
            }
        }
        return;
    };

    if let Ok(raw) = serde_json::to_string(&placement) {
        let _ = db.set_setting(WINDOW_PLACEMENT_SETTING_KEY, &raw);
    }
}

pub fn restore_window_placement_from_db(window: &tauri::WebviewWindow, db: &Database) {
    let Some(raw) = db.get_setting(WINDOW_PLACEMENT_SETTING_KEY).ok().flatten() else {
        return;
    };
    let Ok(saved) = serde_json::from_str::<WindowPlacementDto>(&raw) else {
        return;
    };

    let monitors: Vec<MonitorDescriptorDto> = window
        .available_monitors()
        .ok()
        .unwrap_or_default()
        .into_iter()
        .map(|m| MonitorDescriptorDto {
            name: m.name().cloned(),
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
            scale_factor: m.scale_factor(),
        })
        .collect();

    let restored = resolve_window_restore_placement(&saved, &monitors);

    let _ = window.unmaximize();
    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        restored.x,
        restored.y,
    )));
    let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
        restored.width,
        restored.height,
    )));
    if restored.maximized {
        let _ = window.maximize();
    }
}

#[tauri::command]
pub fn window_set_mini_mode(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    mini_mode: bool,
    expanded_height: Option<bool>,
) -> AppResult<bool> {
    use tauri::{LogicalSize, Size};
    if mini_mode {
        capture_and_save_window_placement(&window, &state.db);
        let _ = window.unmaximize();
        let h = if expanded_height.unwrap_or(false) {
            210.0
        } else {
            88.0
        };
        let _ = window.set_min_size(Some(Size::Logical(LogicalSize::new(320.0, 84.0))));
        let _ = window.set_size(Size::Logical(LogicalSize::new(340.0, h)));
        let _ = window.set_always_on_top(true);
    } else {
        let _ = window.set_min_size(Some(Size::Logical(LogicalSize::new(980.0, 660.0))));
        let _ = window.set_always_on_top(false);
        if state
            .db
            .get_setting(WINDOW_PLACEMENT_SETTING_KEY)
            .ok()
            .flatten()
            .is_some()
        {
            restore_window_placement_from_db(&window, &state.db);
        } else {
            let _ = window.set_size(Size::Logical(LogicalSize::new(1280.0, 820.0)));
        }
    }
    Ok(mini_mode)
}

#[tauri::command]
pub fn window_minimize(window: tauri::WebviewWindow) -> AppResult<()> {
    let _ = window.minimize();
    Ok(())
}

#[tauri::command]
pub fn window_toggle_maximize(window: tauri::WebviewWindow) -> AppResult<bool> {
    let is_max = window.is_maximized().unwrap_or(false);
    if is_max {
        let _ = window.unmaximize();
        Ok(false)
    } else {
        let _ = window.maximize();
        Ok(true)
    }
}

#[tauri::command]
pub fn window_set_always_on_top(
    window: tauri::WebviewWindow,
    always_on_top: bool,
) -> AppResult<bool> {
    let _ = window.set_always_on_top(always_on_top);
    Ok(always_on_top)
}

#[tauri::command]
pub async fn window_close(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<()> {
    capture_and_save_window_placement(&window, &state.db);
    let close_behavior = state
        .db
        .get_setting("close_behavior")
        .ok()
        .flatten()
        .unwrap_or_else(|| "minimize_to_tray".into());
    if close_behavior == "minimize_to_tray" {
        let _ = window.hide();
    } else {
        state.playback.shutdown().await;
        window.app_handle().exit(0);
    }
    Ok(())
}

const ISLAND_PLACEMENT_SETTING_KEY: &str = "island_placement_json";

#[tauri::command]
pub async fn window_toggle_island(
    state: State<'_, AppState>,
    open: Option<bool>,
) -> AppResult<bool> {
    state.island.show(open.unwrap_or(!state.island.is_open()))
}

#[tauri::command]
pub fn is_island_open(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(state.island.is_open())
}

#[tauri::command]
pub fn island_set_size_and_position(
    app: AppHandle,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
) -> AppResult<()> {
    use tauri::{LogicalPosition, LogicalSize, Position, Size};
    if let Some(win) = app.get_webview_window("island") {
        let _ = win.set_size(Size::Logical(LogicalSize::new(width, height)));
        if let (Some(px), Some(py)) = (x, y) {
            let _ = win.set_position(Position::Logical(LogicalPosition::new(px, py)));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn island_focus_main_window(app: AppHandle) -> AppResult<()> {
    if let Some(main_win) = app.get_webview_window("main") {
        if main_win.is_minimized().unwrap_or(false) {
            let _ = main_win.unminimize();
        }
        let _ = main_win.show();
        let _ = main_win.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub fn island_set_always_on_top(
    app: AppHandle,
    state: State<'_, AppState>,
    always_on_top: bool,
) -> AppResult<bool> {
    if let Some(win) = app.get_webview_window("island") {
        let _ = win.set_always_on_top(always_on_top);
    }
    let _ = state.db.set_setting(
        "island_always_on_top",
        if always_on_top { "true" } else { "false" },
    );
    let _ = app.emit("island:always_on_top_changed", always_on_top);
    if let Ok(settings) = state.session.get_settings() { let _ = app.emit("settings:updated", settings); }
    Ok(always_on_top)
}

#[tauri::command]
pub fn island_save_placement(state: State<'_, AppState>, x: i32, y: i32) -> AppResult<()> {
    let placement = IslandPlacementDto { x, y };
    if let Ok(raw) = serde_json::to_string(&placement) {
        let _ = state.db.set_setting(ISLAND_PLACEMENT_SETTING_KEY, &raw);
    }
    Ok(())
}

#[tauri::command]
pub fn island_get_placement(state: State<'_, AppState>) -> AppResult<Option<IslandPlacementDto>> {
    let saved = state
        .db
        .get_setting(ISLAND_PLACEMENT_SETTING_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<IslandPlacementDto>(&raw).ok());
    Ok(saved)
}

#[tauri::command]
pub fn island_set_privacy_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<bool> {
    let _ = state
        .db
        .set_setting("island_privacy_mode", if enabled { "true" } else { "false" });
    let _ = app.emit("island:privacy_changed", enabled);
    if let Ok(settings) = state.session.get_settings() { let _ = app.emit("settings:updated", settings); }
    Ok(enabled)
}

#[cfg(windows)]
pub fn is_foreground_window_fullscreen(app: &AppHandle) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
    };

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let desktop = GetDesktopWindow();
        let shell = GetShellWindow();
        if fg == desktop || fg == shell {
            return false;
        }

        let mut window_rect = RECT::default();
        if GetWindowRect(fg, &mut window_rect).is_err() {
            return false;
        }

        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
            return false;
        }

        let m_rect = mi.rcMonitor;
        if let Some(island_monitor) = app.get_webview_window("island").and_then(|w| w.current_monitor().ok().flatten()) {
            if island_monitor.position().x != m_rect.left || island_monitor.position().y != m_rect.top { return false; }
        }
        window_rect.left <= m_rect.left
            && window_rect.top <= m_rect.top
            && window_rect.right >= m_rect.right
            && window_rect.bottom >= m_rect.bottom
    }
}

#[cfg(not(windows))]
pub fn is_foreground_window_fullscreen(_app: &AppHandle) -> bool {
    false
}

#[tauri::command]
pub fn check_fullscreen_avoidance(app: AppHandle, state: State<'_, AppState>) -> AppResult<bool> {
    let avoidance_en = state
        .db
        .get_setting("island_fullscreen_avoidance")
        .ok()
        .flatten()
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    if avoidance_en && is_foreground_window_fullscreen(&app) {
        Ok(true)
    } else {
        Ok(false)
    }
}




