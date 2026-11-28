mod activity_log_cmd;
mod analyzer_cmd;
mod audit_cmd;
mod audit_manager;
mod batch_catch_up;
mod batch_close;
mod batch_eta;
mod blender_manager;
mod builtin_profiles;
mod capture_batch_cmd;
mod capture_guards;
mod capture_history_manager;
mod capture_manager;
mod capture_room_manager;
mod catch_up_takes;
mod clip_relink_manager;
mod clip_verdicts;
mod combine_manager;
mod config_checks_cmd;
mod crash_maps_manager;
mod damaged_demo_notices;
mod dem_open_with;
mod demo_cache_cmd;
mod demo_preview;
mod demo_rename_manager;
mod demo_repair_manager;
mod demo_split_manager;
mod dir_browser;
mod download_progress;
mod drop_paths;
mod executable_paths_cmd;
mod export_space_cmd;
mod file_replace;
mod game_process_manager;
mod hd_manager;
mod hook_batch;
mod hook_dll_list;
mod installs_manager;
mod manifest_file;
mod map_builds_manager;
mod map_manager;
mod match_group_cmd;
mod menu_scale_manager;
mod messages;
mod movie_copy_manager;
mod obs_manager;
mod obs_test_manager;
mod open_files;
mod overview_manager;
mod packet_limit_manager;
mod pick_paths;
mod player_index_cmd;
mod players_tab;
mod preview_in_place;
mod project_convert;
mod project_session_cmd;
mod protocol48_demos;
mod render_export_dirs;
mod render_live_resumable;
mod render_manager;
mod review_library;
mod review_manager;
mod scan_cmd;
mod scan_manager;
mod settings_cmd;
mod settings_manager;
mod settings_migrations;
mod take_checks_manager;
mod take_cleanup_manager;
mod take_delete_manager;
mod take_preview_manager;
mod trimmed_demos;
mod unreached_clips;
mod updater_manager;
mod vegas_manager;

/// Helpers this crate's own tests share. See `Scratch` on why a temporary
/// directory needs a guard rather than a trailing `remove_dir_all` (#253).
#[cfg(test)]
mod test_support;

use audit_manager::AuditManager;
use capture_manager::{CaptureManager, read_cfg_commands};
use demo_preview::{
    delete_orphaned_previews, launch_demo_preview, scan_orphaned_previews,
    send_preview_to_running_game,
};
use game_process_manager::{
    check_engine_processes, kill_engine_processes, launch_standalone_game, start_steam, steam_state,
};
use obs_manager::launch_obs;
use render_manager::{
    RenderManager, cancel_queued_render_jobs, cancel_render_batch, cancel_render_job,
    cancel_render_jobs, check_render_autosave, clear_render_queue, discard_render_autosave,
    get_export_pool_free_gb, get_render_jobs, get_render_required_estimate_gb, queue_render_batch,
    recover_render_batch, remove_non_rendering_render_jobs, remove_render_job, remove_render_jobs,
    reset_all_render_jobs, reset_render_job, reset_render_jobs, set_render_job_codec,
    start_queued_render,
};
use scan_cmd::ScanManager;
use settings_manager::SettingsManager;

// ── App entry point ────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // A project or demo Windows asked this copy to open (#528). When this
    // exe is already running, that copy opens it instead and this one exits
    // before anything else starts.
    let launch_files = open_files::openable_paths(
        std::env::args_os(),
        &std::env::current_dir().unwrap_or_default(),
    );
    #[cfg(windows)]
    let open_pipe = open_files::own_pipe_name();
    #[cfg(windows)]
    if let Some(pipe) = &open_pipe
        && !launch_files.is_empty()
        && open_files::forward_to_running(pipe, &launch_files)
    {
        return;
    }

    // Before the builder, not in `.setup()`: `SettingsManager::new()` below is
    // a `.manage(..)` argument, so it is constructed -- and reads settings.json
    // -- before any setup hook runs. Migrating after that would read the new,
    // empty location and hand the user a default config.
    let migration = native::shared::paths::migrate_legacy_appdata_dir();
    if migration.did_something() {
        native::log_markdown(&format!(
            "📁 **Settings** — moved {} item(s) from %APPDATA%\\{} to %APPDATA%\\{}{}{} (#257)",
            migration.moved.len(),
            native::shared::paths::LEGACY_APPDATA_DIR_NAME,
            native::shared::paths::APPDATA_DIR_NAME,
            if migration.skipped.is_empty() {
                String::new()
            } else {
                format!(
                    "; left {} already present at the destination",
                    migration.skipped.len()
                )
            },
            if migration.failed.is_empty() {
                String::new()
            } else {
                format!("; FAILED to move {:?}", migration.failed)
            },
        ));
    }

    // The render manager starts from the export folders settings.json has,
    // so the first save of some other setting isn't taken for a change of
    // them by a running batch (#1430).
    let settings_manager = SettingsManager::new();
    let render_manager = RenderManager::new();
    {
        let loaded = lock_recover::lock_or_recover(&settings_manager.inner);
        render_manager
            .export_dirs()
            .saved(&loaded.render_export_dirs);
        // As loaded, not a change: nothing to log.
        render_manager.resumable().saved(loaded.render_resumable);
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(CaptureManager::new())
        .manage(render_manager)
        .manage(take_preview_manager::TakePreviewManager::default())
        .manage(take_checks_manager::TakeChecksManager::default())
        .manage(ScanManager::default())
        .manage(settings_manager)
        .manage(AuditManager::default())
        .manage(demo_split_manager::DemoSplitManager::default())
        .manage(demo_repair_manager::DemoRepairManager::default())
        .manage(movie_copy_manager::MovieCopyManager::default())
        .manage(combine_manager::CombineManager::default())
        .manage(obs_test_manager::ObsTestManager::default())
        .manage(demo_rename_manager::DemoRenameManager::default())
        .manage(match_group_cmd::MatchGroupManager::default())
        .manage(hd_manager::HdManager::default())
        .manage(blender_manager::BlenderManager::default())
        .manage(updater_manager::UpdaterState::default())
        .manage(open_files::PendingOpenFiles::new(&launch_files))
        .setup(move |app| {
            // Dev/debug builds find the repo-root `localizations/` folder via
            // analysis::translate_key's own walk-up-from-exe search, since the
            // exe runs from inside the source tree. A packaged install runs from
            // e.g. Program Files, nowhere near that folder — its `resources`
            // directory (populated by tauri.conf.json's `bundle.resources`) is
            // the only place weapon-name strings can come from there.
            use tauri::Manager;
            if let Ok(resource_dir) = app.path().resource_dir() {
                analysis::add_localization_search_path(resource_dir.join("localizations"));
            }
            // The game's Highlights tab asks Studio to analyse a demo too big
            // for the game's own memory (#565).
            #[cfg(windows)]
            native::sys::analysis_server::start();
            // Hook DLL copies an earlier run left, which nothing else would
            // remove until the next game launch (#1146).
            #[cfg(windows)]
            {
                let settings = app.state::<SettingsManager>().inner.clone();
                std::thread::spawn(move || {
                    let configured = settings
                        .lock()
                        .ok()
                        .and_then(|s| s.goldsrc_hooks_dll_path.clone())
                        .map(std::path::PathBuf::from);
                    native::sys::hook_copy::clean_at_start(configured.as_deref());
                });
            }
            // Map builds an earlier run left in place, once no game runs
            // (#1547).
            {
                let settings = app.state::<SettingsManager>().inner.clone();
                std::thread::spawn(move || {
                    let game_path = settings.lock().ok().map(|s| s.hl_path.clone());
                    map_builds_manager::clean_leftovers_at_start(game_path);
                });
            }
            // ...and its Review tab to review the demo it has loaded (#1142).
            review_manager::serve_game_requests(app.handle().clone());
            // ...and to say a demo it plays is damaged, for a toast with
            // Repair (#1373).
            damaged_demo_notices::serve_damage_notices(app.handle().clone());
            // Files a later double-click hands this copy (#528): queued, then
            // the page is told and the window comes to the front.
            #[cfg(windows)]
            if let Some(pipe) = open_pipe.clone() {
                use tauri::Emitter;
                let handle = app.handle().clone();
                open_files::serve(pipe, move |files| {
                    handle.state::<open_files::PendingOpenFiles>().push(&files);
                    let _ = handle.emit(open_files::OPEN_FILES_EVENT, ());
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            demo_cache_cmd::cache_demos,
            drop_paths::sort_dropped_paths,
            demo_cache_cmd::cancel_demo_cache,
            activity_log_cmd::log_frontend_event,
            activity_log_cmd::get_activity_log_path,
            executable_paths_cmd::validate_paths,
            executable_paths_cmd::check_hlae_ffmpeg,
            executable_paths_cmd::diagnose_executable_paths,
            settings_manager::list_game_folders,
            settings_manager::game_folder_language_file,
            movie_copy_manager::movie_copy_plan,
            movie_copy_manager::start_movie_copy,
            movie_copy_manager::cancel_movie_copy,
            players_tab::list_logo_pngs,
            players_tab::list_crosshair_sprites,
            players_tab::crosshair_sprite_image,
            players_tab::add_crosshair_file,
            players_tab::set_player_overrides,
            players_tab::take_pov_own_spray_notice,
            executable_paths_cmd::link_hlae_ffmpeg,
            analyzer_cmd::analyze_demo_full,
            analyzer_cmd::get_weapon_display_names,
            analyzer_cmd::load_map_overview,
            capture_batch_cmd::start_capture_batch,
            launch_demo_preview,
            obs_manager::obs_test_connection,
            obs_manager::obs_check_orphan,
            obs_manager::obs_recover_orphan,
            obs_test_manager::obs_test_clip,
            obs_test_manager::obs_test_watch_start,
            obs_test_manager::obs_test_analyze,
            obs_test_manager::obs_test_cancel,
            obs_test_manager::obs_test_delete_take,
            launch_obs,
            launch_standalone_game,
            read_cfg_commands,
            check_engine_processes,
            batch_catch_up::catch_up_unwatched_batches,
            batch_close::wait_for_batch_start,
            kill_engine_processes,
            game_process_manager::check_running_game,
            game_process_manager::running_game_for_batch,
            game_process_manager::close_running_game,
            steam_state,
            start_steam,
            send_preview_to_running_game,
            review_manager::start_highlight_review,
            review_manager::stop_highlight_review,
            review_manager::game_review_progress,
            review_manager::game_review_finished,
            review_library::review_library_get,
            review_library::review_library_update,
            scan_orphaned_previews,
            delete_orphaned_previews,
            capture_batch_cmd::cancel_capture_batch,
            scan_cmd::scan_directory,
            scan_cmd::estimate_rescan,
            scan_cmd::cancel_scan,
            export_space_cmd::calculate_export_pool_space,
            export_space_cmd::capture_pool_room,
            export_space_cmd::diagnose_capture_output_paths,
            queue_render_batch,
            start_queued_render,
            cancel_render_batch,
            cancel_render_job,
            reset_render_job,
            reset_all_render_jobs,
            remove_render_job,
            remove_non_rendering_render_jobs,
            cancel_render_jobs,
            cancel_queued_render_jobs,
            reset_render_jobs,
            remove_render_jobs,
            clear_render_queue,
            capture_history_manager::take_capture_states,
            capture_history_manager::locate_rendered_videos,
            clip_relink_manager::relink_rendered_clips,
            clip_relink_manager::cancel_relink_rendered_clips,
            take_delete_manager::measure_take_folders,
            take_delete_manager::delete_take_folders,
            take_preview_manager::take_preview_thumb,
            take_preview_manager::take_preview_sheet,
            take_preview_manager::cancel_take_previews,
            take_checks_manager::take_checks,
            take_checks_manager::cancel_take_checks,
            set_render_job_codec,
            get_export_pool_free_gb,
            get_render_required_estimate_gb,
            check_render_autosave,
            discard_render_autosave,
            recover_render_batch,
            get_render_jobs,
            settings_cmd::get_settings,
            settings_cmd::save_settings,
            settings_cmd::builtin_command_profiles,
            project_session_cmd::save_project_session,
            project_session_cmd::load_project_session,
            project_session_cmd::files_exist,
            player_index_cmd::index_demo_players,
            player_index_cmd::lookup_demo_players,
            project_session_cmd::default_projects_dir,
            project_session_cmd::locate_missing_demos,
            project_session_cmd::changed_demos,
            scan_cmd::system_memory_bytes,
            audit_cmd::run_demo_audit,
            demo_split_manager::find_multi_map_demos_cmd,
            demo_split_manager::cancel_multi_map_scan,
            demo_split_manager::demo_map_segments,
            demo_split_manager::split_demo_maps,
            demo_split_manager::cancel_demo_split,
            demo_split_manager::split_demo_auto,
            demo_repair_manager::find_damaged_demos_cmd,
            demo_repair_manager::cancel_damaged_scan,
            demo_repair_manager::repair_demo_cmd,
            demo_repair_manager::cancel_demo_repair,
            demo_repair_manager::demo_repairable,
            demo_split_manager::demo_split_status,
            audit_cmd::delete_audit_files,
            audit_cmd::list_fixed_drives,
            audit_cmd::cancel_audit,
            crash_maps_manager::crash_map_warnings,
            crash_maps_manager::forget_crash_map,
            packet_limit_manager::engine_packet_entity_limit,
            menu_scale_manager::menu_scale_status,
            menu_scale_manager::set_menu_scale,
            installs_manager::find_installs,
            combine_manager::combine_plan,
            combine_manager::combine_clips,
            combine_manager::combine_cancel,
            demo_rename_manager::demo_rename_list,
            demo_rename_manager::demo_rename_cancel,
            demo_rename_manager::demo_rename_apply,
            demo_rename_manager::demo_rename_undo,
            demo_rename_manager::demo_rename_undoable,
            match_group_cmd::queue_match_groups,
            match_group_cmd::queue_match_groups_cancel,
            audit_cmd::reveal_in_explorer,
            audit_cmd::open_recycle_bin,
            dir_browser::browse_directory,
            dir_browser::read_demo_heads,
            dir_browser::default_browse_dir,
            pick_paths::pick_path_states,
            pick_paths::pick_user_folders,
            pick_paths::file_dialog_shown,
            dir_browser::count_demo_files_in_folder,
            dir_browser::scan_demo_folders,
            map_manager::check_demo_maps,
            map_manager::download_map,
            map_builds_manager::add_map_build,
            map_builds_manager::plan_map_builds,
            map_builds_manager::open_game_map_warnings,
            config_checks_cmd::scan_game_configs,
            config_checks_cmd::roll_floors,
            hd_manager::hd_status,
            hd_manager::hd_setup_tools,
            hd_manager::hd_cancel,
            hd_manager::hd_build,
            hd_manager::hd_set_python,
            hd_manager::hd_set_upscaler,
            overview_manager::overview_installs,
            overview_manager::overview_maps,
            overview_manager::overview_scene,
            overview_manager::overview_load_edits,
            overview_manager::overview_save_edits,
            overview_manager::overview_reset_edits,
            overview_manager::overview_export,
            overview_manager::overview_export_hd,
            overview_manager::overview_flag_icons,
            overview_manager::overview_screen_height,
            hd_manager::hd_misses,
            hd_manager::hd_save_style,
            hd_manager::hd_remove_style,
            hd_manager::hd_save_map_list,
            hd_manager::hd_world_coverage,
            hd_manager::hd_preview,
            blender_manager::blender_status,
            blender_manager::blender_set_exe,
            blender_manager::blender_run,
            blender_manager::blender_cancel,
            vegas_manager::vegas_install_script,
            updater_manager::check_for_update,
            updater_manager::download_and_install_update,
            updater_manager::restart_app,
            updater_manager::release_history,
            updater_manager::is_debug_build,
            updater_manager::local_git_branch,
            updater_manager::hook_dll_override_allowed,
            hook_dll_list::list_hook_dlls,
            open_files::take_open_files,
            dem_open_with::dem_open_with_status,
            dem_open_with::set_dem_open_with,
            project_convert::project_convert_offer,
            project_convert::convert_project_file,
        ])
        .build(tauri::generate_context!());
    // A release build has no console, so a panic here would leave no trace:
    // the activity log is the one place a user can see why Studio closed.
    // This covers `Builder::build` failing (event loop, plugin, menu, tray);
    // the windows are created later, inside `app.run()`.
    let app = match app {
        Ok(app) => app,
        Err(err) => {
            native::log_markdown(&messages::studio_start_failed(&err));
            std::process::exit(1);
        }
    };
    app.run(|_app, event| {
        // Every FFmpeg Studio starts (renders, combines, take checks, the
        // OBS benchmark) ends with Studio's process anyway (their job
        // object, #1496); this ends them first and removes the unfinished
        // renders and combines they were writing (#1439).
        if let tauri::RunEvent::Exit = event {
            native::hlcr::renderer::stop_renders_for_exit();
            // Map builds in place for a game that has closed come out now;
            // with a game running they stay for the next start (#1547).
            native::maps::build_placement::release_at_exit();
        }
    });
}
