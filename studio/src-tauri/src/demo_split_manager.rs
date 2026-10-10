//! The Demo Auditor's Split tab (#624): find the demos in a folder that
//! recorded more than one map, list each one's maps, and write the chosen
//! maps out as demos of their own. The work is `native::demo_split` and
//! `native::demo_maps_scan`; this is the IPC around it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use native::demo_maps_scan::{DemoMaps, find_multi_map_demos};
use native::demo_split::{
    Keep, MapSegment, SplitProgress, Written, demo_segments, split_file_with_progress,
};
use tauri::{AppHandle, Emitter};

use crate::settings_manager::SettingsManager;

#[derive(Default)]
pub struct DemoSplitManager {
    pub cancel: Arc<AtomicBool>,
}

/// Progress events at most this often (~30 fps), per the event-loop rule.
const PROGRESS_EVERY_MS: u32 = 33;

#[tauri::command]
pub async fn find_multi_map_demos_cmd(
    app_handle: AppHandle,
    state: tauri::State<'_, DemoSplitManager>,
    folder: String,
    recursive: bool,
) -> Result<Vec<DemoMaps>, String> {
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let folder = PathBuf::from(&folder);
        if !folder.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        let started = std::time::Instant::now();
        let last = AtomicU32::new(0);
        let mut progress = |done: usize, total: usize, demo: &Path| {
            let now = started.elapsed().as_millis() as u32;
            if done < total && now.saturating_sub(last.load(Ordering::Relaxed)) < PROGRESS_EVERY_MS
            {
                return;
            }
            last.store(now, Ordering::Relaxed);
            let _ = app_handle.emit(
                "split_scan_progress",
                serde_json::json!({
                    "done": done,
                    "total": total,
                    "demo": demo.file_name().map(|n| n.to_string_lossy().to_string()),
                }),
            );
        };
        Ok(find_multi_map_demos(
            &folder,
            recursive,
            &native::analyzer_cache_root(),
            &cancel,
            &mut progress,
        ))
    }))
    .await
}

#[tauri::command]
pub fn cancel_multi_map_scan(state: tauri::State<'_, DemoSplitManager>) -> Result<(), String> {
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn demo_map_segments(path: String) -> Result<Vec<MapSegment>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        demo_segments(Path::new(&path))
    }))
    .await
}

/// Writes the ticked maps (`keep`) of the demo at `path` as demos of their own.
#[tauri::command]
pub async fn split_demo_maps(
    app_handle: AppHandle,
    settings: tauri::State<'_, SettingsManager>,
    path: String,
    keep: Vec<usize>,
) -> Result<Vec<Written>, String> {
    let map_dirs = game_map_dirs(&settings);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        split_with_events(&app_handle, &path, Keep::These(&keep), &map_dirs)
    }))
    .await
}

/// Split now (#217): every map at least `min_seconds` long, with one parse of
/// the demo (the maps' lengths come from it too).
#[tauri::command]
pub async fn split_demo_auto(
    app_handle: AppHandle,
    settings: tauri::State<'_, SettingsManager>,
    path: String,
    min_seconds: f32,
) -> Result<Vec<Written>, String> {
    let map_dirs = game_map_dirs(&settings);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        split_with_events(&app_handle, &path, Keep::AtLeast(min_seconds), &map_dirs)
    }))
    .await
}

/// The game's own maps folders, for a map whose BSP the demo's folder lacks.
fn game_map_dirs(settings: &SettingsManager) -> Vec<PathBuf> {
    let hl_path = settings
        .inner
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .hl_path
        .clone();
    Path::new(&hl_path)
        .parent()
        .map(|game| {
            ["dod", "dod_downloads", "dod_addon"]
                .iter()
                .map(|d| game.join(d).join("maps"))
                .collect()
        })
        .unwrap_or_default()
}

/// Runs a split, emitting `split_progress` (the demo's path plus
/// [`SplitProgress`]) at most every [`PROGRESS_EVERY_MS`], and always at a new
/// stage or map, so a stage change is never dropped.
fn split_with_events(
    app_handle: &AppHandle,
    path: &str,
    keep: Keep,
    map_dirs: &[PathBuf],
) -> Result<Vec<Written>, String> {
    let started = std::time::Instant::now();
    let last = AtomicU32::new(0);
    let mut last_step = (String::new(), 0usize);
    split_file_with_progress(Path::new(path), keep, map_dirs, &mut |p: SplitProgress| {
        let now = started.elapsed().as_millis() as u32;
        let step = (p.stage.to_string(), p.part);
        if step == last_step && now.saturating_sub(last.load(Ordering::Relaxed)) < PROGRESS_EVERY_MS
        {
            return;
        }
        last_step = step;
        last.store(now, Ordering::Relaxed);
        let _ = app_handle.emit(
            "split_progress",
            serde_json::json!({ "path": path, "progress": p }),
        );
    })
}
