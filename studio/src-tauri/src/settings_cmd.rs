//! The settings commands (#1174): what the frontend reads and saves of
//! `settings.json`, and the built-in command profiles listed beside the
//! user's own. The manager and the file format live in `settings_manager.rs`.

use crate::builtin_profiles;
use crate::messages;
use crate::players_tab;
use crate::render_manager;
use crate::settings_manager::{self, AppSettings, SettingsManager};
use lock_recover::lock_or_recover;
use std::sync::Arc;

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, SettingsManager>) -> Result<AppSettings, String> {
    let inner_arc = Arc::clone(&state.inner);
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let guard = lock_or_recover(&inner_arc);
        Ok(guard.clone())
    }))
    .await
}

#[tauri::command]
pub async fn save_settings(
    state: tauri::State<'_, SettingsManager>,
    render: tauri::State<'_, render_manager::RenderManager>,
    settings: AppSettings,
) -> Result<(), String> {
    let inner_arc = Arc::clone(&state.inner);
    // A running render batch takes the new export folders for the renders it
    // starts next (#1430). Handed over under the settings lock, so two saves
    // close together can't leave it the older list.
    let export_dirs = render.export_dirs();
    // "Resumable renders" reaches a running batch's next renders the same way
    // (#1451).
    let resumable = render.resumable();
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let mut settings = settings;
        // Refused before anything is written: a game folder that is a path
        // would put the launch and every file Studio writes somewhere other
        // than beside hl.exe (#402).
        native::shared::paths::validate_game_folder(&settings.game_folder)?;
        let mut guard = lock_or_recover(&inner_arc);
        // The frontend doesn't carry this field; the loaded value stands, or a
        // migration would run again at every start.
        settings.settings_migrations = guard.settings_migrations.max(settings.settings_migrations);
        settings.pov_own_spray_notice = guard.pov_own_spray_notice;
        settings.pinned_folders_reviewed |= settings_manager::pinned_review_done_on_disk();
        settings.save()?;
        settings_manager::apply_game_folder(&settings);
        settings_manager::apply_windows_drive_keep_free(&settings);
        export_dirs.saved(&settings.render_export_dirs);
        if resumable.saved(settings.render_resumable) {
            native::log_markdown(&messages::render::resumable_turned(
                settings.render_resumable,
            ));
        }
        players_tab::mirror_table(&settings);
        *guard = settings;
        native::log_markdown("[settings] saved");
        Ok(())
    }))
    .await
}

/// The built-in command profiles (#911), which the Commands tab lists locked
/// beside the user's own. Never saved: every update replaces them.
#[tauri::command]
pub fn builtin_command_profiles() -> Vec<settings_manager::CommandProfile> {
    builtin_profiles::builtin_command_profiles()
}
