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

/// Writes the high-quality copy (`<map>_hd.tga`). The pixels come as the raw
/// request body -- 48 MB at 4096x3072, too big to send as JSON -- and where
/// to put them in the `x-overview` header, URI-encoded JSON:
/// `{install, map, target, width, height}`.
#[tauri::command]
pub async fn overview_export_hd(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(rgba) = request.body() else {
        return Err("the high-quality image did not arrive as raw bytes".to_string());
    };
    let header = request
        .headers()
        .get("x-overview")
        .and_then(|v| v.to_str().ok())
        .ok_or("the high-quality image arrived without its details")?;
    let decoded: String = url::form_urlencoded::parse(format!("m={header}").as_bytes())
        .find(|(k, _)| k == "m")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    #[derive(serde::Deserialize)]
    struct Meta {
        install: String,
        map: String,
        target: files::Target,
        width: u32,
        height: u32,
    }
    let meta: Meta = serde_json::from_str(&decoded).map_err(|e| e.to_string())?;
    let rgba = rgba.clone();
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        files::save_hd(
            Path::new(&meta.install),
            meta.target,
            &meta.map,
            meta.width,
            meta.height,
            &rgba,
        )
    }))
    .await
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
