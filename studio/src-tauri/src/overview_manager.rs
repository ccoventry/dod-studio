//! The Overviews page's backend (#371): installs and maps, the scene for a
//! map, the page's saved edits, and writing the finished overview. The work
//! is in `native::overview`; this is the Tauri surface.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use tauri::Emitter;

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

/// Which `overview_scene` call is the newest: an older one still building
/// gives up, so clicking down the map list builds only the map clicked last.
static LATEST_SCENE: AtomicU64 = AtomicU64::new(0);

/// Reads the map and works out what to draw. A tenth of a second to a
/// second, off the async runtime's threads, sending `overview_progress`
/// (`{ map, fraction }`, at most every 33 ms) as it goes. Fails with
/// `native::overview::reach::CANCELLED` once another map is asked for.
#[tauri::command]
pub async fn overview_scene(
    app: tauri::AppHandle,
    install: String,
    map: String,
) -> Result<Scene, String> {
    let mine = LATEST_SCENE.fetch_add(1, Ordering::SeqCst) + 1;
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let last_sent = AtomicU32::new(0);
        let progress = |fraction: f32| {
            let now = started.elapsed().as_millis() as u32;
            if now.saturating_sub(last_sent.load(Ordering::Relaxed)) >= 33 {
                last_sent.store(now, Ordering::Relaxed);
                let _ = app.emit(
                    "overview_progress",
                    serde_json::json!({ "map": map, "fraction": fraction }),
                );
            }
            LATEST_SCENE.load(Ordering::SeqCst) != mine
        };
        native::overview::scene_for_until(Path::new(&install), &map, &progress)
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
