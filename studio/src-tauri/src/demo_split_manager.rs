//! The Demo Auditor's Split tab (#624): find the demos in a folder that
//! recorded more than one map, list each one's maps, and write the chosen
//! maps out as demos of their own. The work is `native::demo_split` and
//! `native::demo_maps_scan`; this is the IPC around it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use native::demo_maps_scan::{DemoMaps, find_multi_map_demos};
use native::demo_split::{MapSegment, Written, demo_segments, split_file};
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

#[tauri::command]
pub async fn split_demo_maps(
    settings: tauri::State<'_, SettingsManager>,
    path: String,
    keep: Vec<usize>,
) -> Result<Vec<Written>, String> {
    // The game's own maps folder, for a map whose BSP the demo's folder lacks.
    let hl_path = settings
        .inner
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .hl_path
        .clone();
    let map_dirs: Vec<PathBuf> = Path::new(&hl_path)
        .parent()
        .map(|game| {
            ["dod", "dod_downloads", "dod_addon"]
                .iter()
                .map(|d| game.join(d).join("maps"))
                .collect()
        })
        .unwrap_or_default();
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        split_file(Path::new(&path), &keep, &map_dirs)
    }))
    .await
}
