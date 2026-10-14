#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod commands;
pub mod db;
pub mod domain;
pub mod player;
pub mod providers;
pub mod services;

#[cfg(test)]
mod regression_baseline;

use commands::AppState;
use db::Database;
use providers::AsmrOneProvider;
use services::download::DownloadService;
use services::library::LibraryService;
use services::playback::PlaybackService;
use services::session::SessionService;
use services::updater::UpdateService;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};

pub fn run() {
    // T911: Enable WebView2 hardware GPU rasterization & zero-copy compositing by default
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--enable-gpu-rasterization --enable-zero-copy --enable-features=CanvasOopRasterization",
        );
    }

    // T1417: the identifier also namespaces WebView data, SQLite and cache paths.
    // Keep the release identifier stable; debug builds never open release data.
    let mut context = tauri::generate_context!();
    if cfg!(debug_assertions) {
        context.config_mut().identifier.push_str(".debug");
    }
    #[cfg(windows)]
    let startup_gate = match services::instance::StartupGate::acquire(&context.config().identifier) {
        Ok(gate) => gate,
        Err(error) => { eprintln!("{error}"); return; }
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            // Keep the gate alive through window setup, then release it even on error.
            #[cfg(windows)]
            let _startup_gate = startup_gate;
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join(&app.config().identifier).join("data"));
            let cache_dir = app
                .path()
                .app_cache_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join(&app.config().identifier).join("cache"));

            let db = Database::open_and_migrate(&data_dir, &cache_dir)
                .expect("Failed to initialize SQLite database and migrations");

            let library = LibraryService::new(db.clone());
            let provider = AsmrOneProvider::new(db.clone());
            let playback = PlaybackService::new(db.clone(), provider.clone());
            let downloads = DownloadService::new(db.clone(), provider.clone());
            let session = SessionService::new(
                db.clone(),
                provider.clone(),
                playback.clone(),
                downloads.clone(),
            );

            playback.attach_app_handle(app.handle().clone());
            downloads.attach_app_handle(app.handle().clone());
            let updater = UpdateService::new(db.clone(), app.handle().clone())?;
            let bg_updater = updater.clone();
            let bg_backups = session.backups.clone();
            let island = services::island::IslandService::new(db.clone(), app.handle().clone());
            // WebViews can invoke commands as soon as they load. Register every
            // service before creating either window or doing any window initialization.
            app.manage(AppState {
                db: db.clone(), library: library.clone(), provider: provider.clone(),
                playback: playback.clone(), downloads: downloads.clone(), session,
                updater, island: island.clone(),
                desktop_lyrics: services::desktop_lyrics::DesktopLyricsService::new(db.clone(), app.handle().clone()),
            });
            for config in &app.config().app.windows {
                if app.get_webview_window(&config.label).is_none() {
                    tauri::WebviewWindowBuilder::from_config(app, config)?.visible(false).build()?;
                }
            }
            if let Some(main_win) = app.get_webview_window("main") {
                commands::restore_window_placement_from_db(&main_win, &db);
                if app.config().app.windows.iter().any(|config| config.label == "main" && config.visible) {
                    main_win.show()?;
                }
            }
            island.initialize();
            // Host-owned scheduling remains active in the tray. A wake-up creates
            // at most one overdue backup; failures wait five minutes before retry.
            tauri::async_runtime::spawn(async move {
                loop {
                    let service = bg_backups.clone();
                    let _ = tokio::task::spawn_blocking(move || service.tick(chrono::Utc::now())).await;
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            });
            let bg_island = island.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let _ = bg_updater.check(false).await;
            });

            // T909: Offload download startup reconciliation and mpv engine pre-warming to background
            // so the main window renders immediately on cold start without blocking on I/O or IPC setup.
            let bg_downloads = downloads.clone();
            let bg_playback = playback.clone();
            let bg_library = library.clone();
            let bg_db = db.clone();
            tauri::async_runtime::spawn(async move {
                if bg_downloads.reconcile_on_startup().is_ok() { bg_downloads.start_scheduler(); }
                let _ = bg_playback.prewarm_engine().await;
                // T1201: Background debounced directory watcher incremental sync loop
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    let watch_enabled = bg_db
                        .get_setting("realtime_watch_enabled")
                        .ok()
                        .flatten()
                        .map(|v| v != "false" && v != "0")
                        .unwrap_or(true);
                    if watch_enabled {
                        let lib_clone = bg_library.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            let _ = lib_clone.poll_and_sync_watched_roots();
                        })
                        .await;
                    }
                }
            });

            // T1307: Background loop for fullscreen exclusive avoidance detection
            let bg_app_fs = app.handle().clone();
            let bg_db_fs = db.clone();
            tauri::async_runtime::spawn(async move {
                let mut last_fs = false;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                    let _ = bg_island.refresh_displays();
                    let avoidance_enabled = bg_db_fs
                        .get_setting("island_fullscreen_avoidance")
                        .ok()
                        .flatten()
                        .map(|v| v != "false" && v != "0")
                        .unwrap_or(true);
                    {
                        let is_fs = avoidance_enabled && commands::is_foreground_window_fullscreen(&bg_app_fs);
                        if is_fs != last_fs {
                            last_fs = is_fs;
                            bg_island.avoid_fullscreen(is_fs);
                            let _ = bg_app_fs.emit("island:fullscreen", is_fs);
                        }
                    }
                }
            });

            // T605: System Tray menu (Show, Play/Pause, Quit)
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let toggle_item =
                MenuItem::with_id(app, "toggle_pause", "播放 / 暂停", true, None::<&str>)?;
            let quit_item =
                MenuItem::with_id(app, "quit", "退出 ASMR Player", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &toggle_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("ASMR Player")
                .on_menu_event(|app_handle, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "toggle_pause" => {
                        let pb = app_handle.state::<AppState>().playback.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = pb.toggle_pause().await;
                        });
                    }
                    "quit" => {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false) {
                                commands::capture_and_save_window_placement(
                                    &win,
                                    &app_handle.state::<AppState>().db,
                                );
                            }
                        }
                        let pb = app_handle.state::<AppState>().playback.clone();
                        let handle_clone = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            pb.shutdown().await;
                            handle_clone.exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app_handle = tray.app_handle();
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            let _ = tray_builder.build(app);

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if window.label() == "island" {
                    if let WindowEvent::Moved(position) = event {
                        if let Some(state) = window.app_handle().try_state::<AppState>() { state.island.moved(*position); }
                    }
                }
                if window.label() == "main" && window.is_visible().unwrap_or(false) {
                    let app_handle = window.app_handle();
                    if let Some(webview_win) = app_handle.get_webview_window("main") {
                        let state = app_handle.state::<AppState>();
                        commands::capture_and_save_window_placement(&webview_win, &state.db);
                    }
                }
            }
            WindowEvent::Focused(false) if window.label() == "desktop-lyrics-menu" => {
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == "desktop-lyrics-menu" => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == "desktop-lyrics" => {
                api.prevent_close();
                let svc=window.app_handle().state::<AppState>().desktop_lyrics.clone();
                tauri::async_runtime::spawn_blocking(move || { let _=svc.update(services::desktop_lyrics::LyricsPatch { enabled:Some(false), ..Default::default() }); });
            }
            WindowEvent::CloseRequested { api, .. } => {
                let app_handle = window.app_handle();
                let state = app_handle.state::<AppState>();
                if let Some(webview_win) = app_handle.get_webview_window("main") {
                    commands::capture_and_save_window_placement(&webview_win, &state.db);
                }
                let close_behavior = state
                    .db
                    .get_setting("close_behavior")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "minimize_to_tray".into());

                if close_behavior == "minimize_to_tray" {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    api.prevent_close();
                    let pb = state.playback.clone();
                    let handle_clone = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        pb.shutdown().await;
                        handle_clone.exit(0);
                    });
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_desktop_lyrics_settings,
            commands::update_desktop_lyrics_settings,
            commands::desktop_lyrics_ready,
            commands::desktop_lyrics_reset_position,
            commands::desktop_lyrics_save_position,
            commands::desktop_lyrics_open_menu,
            commands::desktop_lyrics_close_menu,
            commands::get_active_subtitles,
            commands::get_island_preferences,
            commands::set_island_preferences,
            commands::island_set_form,
            commands::island_settle_layout,
            commands::island_layout_ready,
            commands::island_save_current_position,
            commands::island_reset_position,
            commands::get_release_history,
            commands::refresh_release_history,
            commands::claim_upgrade_notice,
            commands::get_update_snapshot,
            commands::check_for_updates,
            commands::set_update_auto_check,
            commands::download_update,
            commands::cancel_update_download,
            commands::dismiss_update,
            commands::install_update,
            commands::open_release_page,
            commands::open_update_link,
            commands::list_works,
            commands::preview_library_batch,
            commands::execute_library_batch,
            commands::undo_library_batch,
            commands::latest_library_batch,
            commands::list_smart_collections,
            commands::save_smart_collection,
            commands::remove_smart_collection,
            commands::query_smart_collection,
            commands::get_work_detail,
            commands::toggle_favorite,
            commands::get_media_affinity,
            commands::toggle_version_favorite,
            commands::toggle_collected,
            commands::set_work_override,
            commands::reset_work_override,
            commands::list_library_roots,
            commands::preview_import,
            commands::preview_smart_import,
            commands::scan_and_import,
            commands::cancel_scan,
            commands::relocate_library_root,
            commands::sync_watched_roots_now,
            commands::open_online_work,
            commands::sync_work_from_asmr_one,
            commands::browse_online_works,
            commands::search_online_works,
            commands::list_online_tags,
            commands::get_playback_snapshot,
            commands::play_version,
            commands::play_directory_group,
            commands::play_work,
            commands::play_favorites_queue,
            commands::toggle_pause,
            commands::seek_ms,
            commands::set_volume,
            commands::set_loop_mode,
            commands::step_queue,
            commands::append_to_queue,
            commands::remove_from_queue,
            commands::move_in_queue,
            commands::clear_queue,
            commands::list_queue_plans,
            commands::save_queue_plan,
            commands::rename_queue_plan,
            commands::preview_queue_plan,
            commands::apply_queue_plan,
            commands::restart_player_engine,
            commands::set_sleep_timer,
            commands::set_ab_loop,
            commands::clear_ab_loop,
            commands::list_ab_loops,
            commands::delete_ab_loop,
            commands::list_bookmarks,
            commands::search_bookmarks,
            commands::play_bookmark,
            commands::export_bookmark_notes,
            commands::restore_bookmark_notes,
            commands::add_bookmark,
            commands::update_bookmark,
            commands::delete_bookmark,
            commands::load_subtitle_cues,
            commands::auto_match_subtitle_for_version,
            commands::bind_subtitle_to_version,
            commands::fetch_online_subtitles_for_work,
            commands::analyze_track_acoustics,
            commands::preview_download,
            commands::prepare_work_download,
            commands::delete_work_from_library,
            commands::delete_work_downloaded_files,
            commands::enqueue_downloads,
            commands::list_download_tasks,
            commands::get_download_policy,
            commands::set_download_policy,
            commands::set_download_priority,
            commands::pause_download_task,
            commands::resume_download_task,
            commands::cancel_download_task,
            commands::associate_works,
            commands::associate_versions,
            commands::undo_association,
            commands::list_association_history,
            commands::get_app_settings,
            commands::get_shortcut_bindings,
            commands::save_shortcut_bindings,
            commands::update_app_settings,
            commands::set_nsfw_enabled,
            commands::get_storage_overview,
            commands::preview_storage_cleanup,
            commands::execute_storage_cleanup,
            commands::get_diagnostic_logs,
            commands::preview_diagnostic_bundle,
            commands::export_diagnostic_bundle,
            commands::export_user_backup,
            commands::get_backup_status,
            commands::set_backup_policy,
            commands::create_user_backup,
            commands::preview_user_backup,
            commands::apply_user_backup,
            commands::export_encrypted_sync_bundle,
            commands::get_dsp_state,
            commands::update_dsp_config,
            commands::apply_dsp_preset,
            commands::import_autoeq_text,
            commands::clear_autoeq_bands,
            commands::save_custom_dsp_preset,
            commands::delete_custom_dsp_preset,
            commands::set_work_dsp_override_enabled,
            commands::set_audio_exclusive,
            commands::get_audio_output,
            commands::set_audio_device,
            commands::set_smtc_mode,
            commands::get_smtc_display_state,
            commands::handle_smtc_action,
            commands::window_set_mini_mode,
            commands::window_minimize,
            commands::window_toggle_maximize,
            commands::window_set_always_on_top,
            commands::window_close,
            commands::window_toggle_island,
            commands::is_island_open,
            commands::island_set_size_and_position,
            commands::island_focus_main_window,
            commands::island_set_always_on_top,
            commands::island_save_placement,
            commands::island_get_placement,
            commands::island_set_privacy_mode,
            commands::check_fullscreen_avoidance,
        ])
        .run(context)
        .expect("error while running ASMR Player tauri application");
}

fn run_smoke_test(work_dir: &str, data_dir_str: &str) -> Result<(), String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let base_dir = std::path::PathBuf::from(data_dir_str);
        let data_dir = base_dir.join("data");
        let cache_dir = base_dir.join("cache");

        let mpv_bin = player::resolve_mpv_binary().map_err(|e| e.message)?;

        // Phase 1: Initialize DB, import local work, play via bundled mpv, seek, and shutdown
        let saved_version_id = {
            let db = db::Database::open_and_migrate(&data_dir, &cache_dir).map_err(|e| e.message)?;
            let library = LibraryService::new(db.clone());
            let provider = AsmrOneProvider::new(db.clone());
            let playback = PlaybackService::new(db.clone(), provider);

            let report = library
                .scan_and_import(work_dir, "single_work", None)
                .map_err(|e| e.message)?;
            if report.works_added < 1 {
                return Err("Smoke test expected at least 1 imported work".into());
            }

            let works = db.list_works("all", None).map_err(|e| e.message)?;
            let work_id = works.first().ok_or("No work found after import")?.id.clone();
            let detail = db.get_work_detail(&work_id).map_err(|e| e.message)?;
            let ver_id = detail
                .tracks
                .first()
                .and_then(|t| t.versions.first())
                .ok_or("No track version found after import")?
                .id
                .clone();

            playback.play(&ver_id, None).await.map_err(|e| e.message)?;
            playback.seek_ms(5_200).await.map_err(|e| e.message)?;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            playback.shutdown().await;
            ver_id
        };

        // Phase 2: Reopen on the same DB to verify session restore and position persistence
        let restored_pos = {
            let db2 =
                db::Database::open_and_migrate(&data_dir, &cache_dir).map_err(|e| e.message)?;
            let provider2 = AsmrOneProvider::new(db2.clone());
            let playback2 = PlaybackService::new(db2, provider2);
            let snap = playback2.get_snapshot().map_err(|e| e.message)?;
            if snap.current_version_id.as_deref() != Some(saved_version_id.as_str()) {
                return Err(format!(
                    "Expected restored version_id {}, got {:?}",
                    saved_version_id, snap.current_version_id
                ));
            }
            if snap.state != "paused" {
                return Err(format!("Expected restored state 'paused', got {}", snap.state));
            }
            if (snap.position_ms - 5_200).abs() > 2_000 {
                return Err(format!(
                    "Restored position {} out of 2s tolerance from 5200ms",
                    snap.position_ms
                ));
            }
            playback2.shutdown().await;
            snap.position_ms
        };

        let result_json = serde_json::json!({
            "status": "ok",
            "mpvBinary": mpv_bin.to_string_lossy(),
            "versionId": saved_version_id,
            "restoredPositionMs": restored_pos,
        });
        let _ = std::fs::write(
            base_dir.join("smoke_test_result.json"),
            serde_json::to_string_pretty(&result_json).unwrap_or_default(),
        );
        Ok(())
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--smoke-test" {
        match run_smoke_test(&args[2], &args[3]) {
            Ok(()) => std::process::exit(0),
            Err(err) => {
                let err_path = std::path::PathBuf::from(&args[3]).join("smoke_test_error.txt");
                let _ = std::fs::write(err_path, &err);
                eprintln!("Smoke test failed: {}", err);
                std::process::exit(1);
            }
        }
    }
    run();
}
