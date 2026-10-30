//! Configuration › Paths, "Size the Game's Menus Like the Pre-Anniversary
//! Engine" (#985): `native::menu_scale` for the selected install and game
//! folder. The box shows what is on disk, not a saved setting.

use std::path::{Path, PathBuf};

use native::menu_scale::MenuScaleStatus;

fn game_dir(game_path: &str, game_folder: &str) -> Result<PathBuf, String> {
    let folder = if game_folder.trim().is_empty() {
        native::shared::paths::DEFAULT_GAME_FOLDER
    } else {
        game_folder.trim()
    };
    native::shared::paths::validate_game_folder(folder)
        .map_err(crate::messages::failed_to_check_game_folder)?;
    Path::new(game_path)
        .parent()
        .filter(|_| !game_path.trim().is_empty())
        .map(|root| root.join(folder))
        .ok_or_else(|| "set the Half-Life executable first".to_string())
}

/// The setting's state for `game_path` (hl.exe) and `game_folder`; brings
/// our copy up to date when it is on.
#[tauri::command]
pub async fn menu_scale_status(
    game_path: String,
    game_folder: String,
) -> Result<MenuScaleStatus, String> {
    let dir = game_dir(&game_path, &game_folder)?;
    tokio::task::spawn_blocking(move || native::menu_scale::status(Path::new(&game_path), &dir))
        .await
        .map_err(|e| format!("menu size check stopped: {e}"))
}

/// Writes (`on`) or removes Studio's scheme copy in `<game>_addon\resource`.
#[tauri::command]
pub async fn set_menu_scale(
    game_path: String,
    game_folder: String,
    on: bool,
) -> Result<MenuScaleStatus, String> {
    let dir = game_dir(&game_path, &game_folder)?;
    tokio::task::spawn_blocking(move || native::menu_scale::set(Path::new(&game_path), &dir, on))
        .await
        .map_err(|e| format!("menu size change stopped: {e}"))?
        .map_err(crate::messages::failed_to_set_menu_size)
}
