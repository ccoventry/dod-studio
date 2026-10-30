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

/// The page's edits for `map`: this PC's own, else the ones saved beside
/// `install`'s overview (`dod_addon/overviews/<map>.dodstudio.json`).
/// Each flag's icon as the game draws it on its map, read from `install`
/// (`native::overview::flag_icons`), for the page to preview.
#[tauri::command]
pub async fn overview_flag_icons(
    install: String,
    map: String,
) -> Result<Vec<native::overview::flag_icons::FlagIcon>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let install = Path::new(&install);
        let bsp = native::overview::files::bsp_path(install, &map);
        let level = native::overview::level::Level::from_file(&bsp)?;
        Ok(native::overview::flag_icons::flag_icons(install, &level))
    }))
    .await
}

/// The screen height Half-Life last ran at (`ScreenHeight` under
/// `HKCU\Software\Valve\Half-Life\Settings`), for sizing the flag icons
/// the page previews; None when it can't be read.
#[tauri::command]
pub async fn overview_screen_height() -> Option<u32> {
    tokio::task::spawn_blocking(screen_height)
        .await
        .ok()
        .flatten()
}

#[cfg(windows)]
fn screen_height() -> Option<u32> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Valve\Half-Life\Settings",
            "/v",
            "ScreenHeight",
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let value = text
        .split_whitespace()
        .skip_while(|w| *w != "REG_DWORD")
        .nth(1)?;
    u32::from_str_radix(value.trim_start_matches("0x"), 16).ok()
}

#[cfg(not(windows))]
fn screen_height() -> Option<u32> {
    None
}

#[tauri::command(async)]
pub fn overview_load_edits(map: String, install: Option<String>) -> Option<serde_json::Value> {
    files::load_edits_for(install.as_deref().map(Path::new), &map)
}

#[tauri::command(async)]
pub fn overview_save_edits(map: String, edits: serde_json::Value) -> Result<(), String> {
    files::save_edits(&map, &edits)
        .inspect(|_| native::log_markdown(&format!("[overview] saved edits for {map}")))
}

#[tauri::command(async)]
pub fn overview_reset_edits(map: String) -> Result<(), String> {
    files::remove_edits(&map)
        .inspect(|_| native::log_markdown(&format!("[overview] reset edits for {map}")))
}

/// Writes the high-quality copy (`<map>_hd.tga`). The pixels come as the raw
/// request body -- 48 MB at 4096x3072, too big to send as JSON -- and where
/// to put them in the `x-overview` header, URI-encoded JSON:
/// `{install, map, width, height}`. Always to `dod_addon` (`files::save_hd`).
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
        width: u32,
        height: u32,
    }
    let meta: Meta = serde_json::from_str(&decoded).map_err(|e| e.to_string())?;
    let rgba = rgba.clone();
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        files::save_hd(
            Path::new(&meta.install),
            &meta.map,
            meta.width,
            meta.height,
            &rgba,
        )
        .inspect(|path| {
            native::log_markdown(&format!(
                "[overview] saved the high-quality {} ({}x{}): {}",
                meta.map,
                meta.width,
                meta.height,
                Path::new(path).display()
            ));
        })
    }))
    .await
}

/// Encodes the page's drawing and writes it beside a `.txt`, backing up a
/// user's own overview first.
#[tauri::command]
pub async fn overview_export(request: Export) -> Result<Saved, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        files::export(&request)
            .inspect(|saved| native::log_markdown(&export_log(&request.map, saved)))
    }))
    .await
}

/// The activity log's line for an exported overview.
fn export_log(map: &str, saved: &Saved) -> String {
    let mut line = format!("[overview] exported {map}: {} file(s)", saved.written.len());
    if !saved.backed_up.is_empty() {
        line.push_str(&format!(
            ", {} of your own backed up",
            saved.backed_up.len()
        ));
    }
    if !saved.removed.is_empty() {
        line.push_str(&format!(", {} older one(s) removed", saved.removed.len()));
    }
    line
}

#[cfg(test)]
mod log_tests {
    use super::*;

    #[test]
    fn the_export_line_counts_files_backups_and_removals() {
        let saved = Saved {
            written: vec!["a.tga".into(), "a.txt".into()],
            backed_up: vec!["old.tga".into()],
            removed: vec![],
        };
        assert_eq!(
            export_log("dod_anzio", &saved),
            "[overview] exported dod_anzio: 2 file(s), 1 of your own backed up"
        );
    }
}
