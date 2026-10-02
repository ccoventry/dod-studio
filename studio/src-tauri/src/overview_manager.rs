//! The Overviews page's backend (#371): installs and maps, the scene for a
//! map, the page's saved edits, and writing the finished overview. The work
//! is in `native::overview`; this is the Tauri surface.

use std::path::{Path, PathBuf};

use native::overview::files::{self, Export, Install, MapEntry, Saved};
use native::overview::scene::Scene;

/// Every Half-Life install with Day of Defeat on this PC. `game_path` is the
/// Configuration page's `hl.exe`, so a library Steam's own list misses is
/// still found.
#[tauri::command]
pub async fn overview_installs(game_path: String) -> Result<Vec<Install>, String> {
    let hint = (!game_path.trim().is_empty()).then(|| PathBuf::from(game_path.trim()));
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        files::installs(hint.as_deref())
    }))
    .await
}

#[tauri::command]
pub async fn overview_maps(install: String) -> Result<Vec<MapEntry>, String> {
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        files::maps(Path::new(&install))
    }))
    .await
}

/// Reads the map and works out what to draw. A tenth of a second to a
/// second, off the async runtime's threads.
#[tauri::command]
pub async fn overview_scene(install: String, map: String) -> Result<Scene, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        native::overview::scene_for(Path::new(&install), &map)
    }))
    .await
}

#[tauri::command]
pub fn overview_load_edits(map: String) -> Option<serde_json::Value> {
    files::load_edits(&map)
}

#[tauri::command]
pub fn overview_save_edits(map: String, edits: serde_json::Value) -> Result<(), String> {
    files::save_edits(&map, &edits)
}

#[tauri::command]
pub fn overview_reset_edits(map: String) -> Result<(), String> {
    files::remove_edits(&map)
}

/// Encodes the page's drawing and writes it beside a `.txt`, backing up a
/// user's own overview first.
#[tauri::command]
pub async fn overview_export(request: Export) -> Result<Saved, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        files::export(&request)
    }))
    .await
}
