//! Configuration > Players (#428): the backend of the tab. Mirrors the table
//! for the hook DLL, lists the PNGs and sprites the dropdowns offer, decodes
//! a sprite for the tile picker, and tells running games the "Use in the
//! game" box (`dodstudio_player_overrides`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use native::player_overrides::{self, SpriteImage, cvar_line};
use native::sys::game_remote::{Sent, send_console_commands};

use crate::settings_manager::AppSettings;

/// How long a game Studio just started gets to open its command pipe before
/// the box's value is sent (the batch's own `GameHandoff` waits as long).
const PIPE_WAIT: Duration = Duration::from_secs(120);
/// Between two looks for the pipe: only watching a process come up.
const PIPE_POLL: Duration = Duration::from_millis(500);

/// Writes the Players table where the hook DLL reads it. A failure is said
/// in the activity log rather than failing the save: the rest of the
/// settings are already written, and the hook keeps its last copy.
pub fn mirror_table(settings: &AppSettings) {
    if let Err(e) = player_overrides::write_table(&settings.players) {
        native::log_markdown(&format!(
            "⚠️ **Players** — {e}. The game keeps the table it last read."
        ));
    }
}

/// `<hl.exe's folder>\<game folder>`, or `None` when either can't be used.
fn game_dir(game_path: &str, game_folder: &str) -> Option<PathBuf> {
    let folder = if game_folder.trim().is_empty() {
        native::shared::paths::DEFAULT_GAME_FOLDER
    } else {
        game_folder.trim()
    };
    native::shared::paths::validate_game_folder(folder).ok()?;
    Some(Path::new(game_path).parent()?.join(folder))
}

/// The PNGs in the game install's `logos` folder, for the spray column.
/// Empty with no `hl.exe` set or no such folder.
#[tauri::command(async)]
pub fn list_logo_pngs(game_path: String, game_folder: String) -> Vec<String> {
    game_dir(&game_path, &game_folder)
        .map(|dir| player_overrides::list_logo_pngs(&dir))
        .unwrap_or_default()
}

/// The sprites in the game install's `sprites` folders (the addon folder's
/// and the game folder's), for the crosshair column.
#[tauri::command(async)]
pub fn list_crosshair_sprites(game_path: String, game_folder: String) -> Vec<String> {
    game_dir(&game_path, &game_folder)
        .map(|dir| player_overrides::list_sprites(&dir))
        .unwrap_or_default()
}

/// One sprite's first frame, decoded for the tile picker's preview.
#[tauri::command(async)]
pub fn crosshair_sprite_image(
    game_path: String,
    game_folder: String,
    sprite: String,
) -> Result<SpriteImage, String> {
    if !player_overrides::is_sprite_name(&sprite) {
        return Err(format!("{sprite:?} is not a sprite file name"));
    }
    let dir = game_dir(&game_path, &game_folder)
        .ok_or_else(|| "set the Half-Life executable and game folder first".to_string())?;
    player_overrides::sprite_image(&player_overrides::sprite_path(&dir, sprite.trim()))
}

/// "Add a crosshair file…" (#842): copies the sprite in `source` (a `.spr`, or
/// the `.zip` the crosshair creator downloads) into the install's
/// `<game>_addon\sprites\` as `customxhair-<name>.spr`, and returns that file
/// name for the picker.
#[tauri::command(async)]
pub fn add_crosshair_file(
    game_path: String,
    game_folder: String,
    source: String,
    name: String,
) -> Result<String, String> {
    let dir = game_dir(&game_path, &game_folder)
        .ok_or_else(|| "set the Half-Life executable and game folder first".to_string())?;
    native::crosshair_import::import_crosshair(&dir, Path::new(&source), &name)
}

/// Sends the box's value to every running game that takes commands, and
/// says how many did. A game not started by Studio has no pipe and is left
/// alone; the next launch, review or batch sends it again.
#[tauri::command(async)]
pub fn set_player_overrides(on: bool) -> usize {
    let line = [cvar_line(on)];
    native::sys::process::game_pids()
        .into_iter()
        .filter(|&pid| matches!(send_console_commands(pid, &line), Ok(Sent::Delivered)))
        .count()
}

/// After Launch Game: sends the box's value once the new game opens its
/// pipe, on a thread of its own so the launch returns at once. Gives up
/// after [`PIPE_WAIT`] (a game without the hook DLL never opens one).
pub fn send_when_listening(on: bool) {
    std::thread::spawn(move || {
        let line = [cvar_line(on)];
        let started = Instant::now();
        while started.elapsed() < PIPE_WAIT {
            for pid in native::sys::process::game_pids() {
                if matches!(send_console_commands(pid, &line), Ok(Sent::Delivered)) {
                    return;
                }
            }
            std::thread::sleep(PIPE_POLL);
        }
    });
}
