//! The Demo Auditor's Split tab (#624): find the demos in a folder that
//! recorded more than one map, list each one's maps, and write the chosen
//! maps out as demos of their own. The work is `native::demo_split` and
//! `native::demo_maps_scan`; this is the IPC around it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use native::demo_maps_scan::{DemoMaps, find_multi_map_demos};
use native::demo_split::{
    Keep, MapFailure, MapSegment, SplitProgress, Written, quick_demo_segments, split_file_report,
};
use tauri::{AppHandle, Emitter};

use crate::settings_manager::SettingsManager;

#[derive(Default)]
pub struct DemoSplitManager {
    pub cancel: Arc<AtomicBool>,
    /// Each running Split Checked Maps' cancel flag, by the demo's path.
    pub splits: std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
}

/// What Split Checked Maps wrote, the maps that failed (the others still
/// run, #876), and whether Cancel stopped it first.
#[derive(serde::Serialize)]
pub struct SplitOutcome {
    pub written: Vec<Written>,
    pub failed: Vec<MapFailure>,
    pub cancelled: bool,
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
        // Frame headers only; a full parse when the walk can't read a frame.
        quick_demo_segments(Path::new(&path))
    }))
    .await
}

/// Writes the ticked maps (`keep`) of the demo at `path` as demos of their
/// own; [`cancel_demo_split`] stops it (the maps already saved stay).
#[tauri::command]
pub async fn split_demo_maps(
    app_handle: AppHandle,
    settings: tauri::State<'_, SettingsManager>,
    state: tauri::State<'_, DemoSplitManager>,
    path: String,
    keep: Vec<usize>,
) -> Result<SplitOutcome, String> {
    let map_dirs = game_map_dirs(&settings);
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .splits
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(path.clone(), Arc::clone(&cancel));
    let key = path.clone();
    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let stop = || cancel.load(Ordering::Relaxed);
        split_with_events(&app_handle, &path, Keep::These(&keep), &map_dirs, &stop).map(|r| {
            SplitOutcome {
                written: r.written,
                failed: r.failed,
                cancelled: r.cancelled,
            }
        })
    }))
    .await;
    state
        .splits
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&key);
    result
}

/// Stops the running split of the demo at `path`, at its next step.
#[tauri::command]
pub fn cancel_demo_split(
    state: tauri::State<'_, DemoSplitManager>,
    path: String,
) -> Result<(), String> {
    if let Some(flag) = state
        .splits
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&path)
    {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
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
        split_with_events(
            &app_handle,
            &path,
            Keep::AtLeast(min_seconds),
            &map_dirs,
            &|| false,
        )
        .and_then(|r| r.into_written())
        .map(|(written, _)| written)
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
            use native::shared::paths::{addon_dir, downloads_dir, mod_dir};
            [mod_dir(game), downloads_dir(game), addon_dir(game)]
                .iter()
                .map(|d| d.join("maps"))
                .collect()
        })
        .unwrap_or_default()
}

/// Runs a split that `cancel` can stop, emitting `split_progress` (the
/// demo's path plus [`SplitProgress`]) at most every [`PROGRESS_EVERY_MS`],
/// and always when a map moves to a new stage, so a map's Finished or Failed
/// is never dropped while others are checked beside it (#876).
fn split_with_events(
    app_handle: &AppHandle,
    path: &str,
    keep: Keep,
    map_dirs: &[PathBuf],
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<native::demo_split::SplitReport, String> {
    let started = std::time::Instant::now();
    let last = AtomicU32::new(0);
    let mut stages = StageChanges::default();
    split_file_report(
        Path::new(path),
        keep,
        map_dirs,
        &mut |p: SplitProgress| {
            let now = started.elapsed().as_millis() as u32;
            let changed = stages.changed(p.part, p.stage);
            if !changed && now.saturating_sub(last.load(Ordering::Relaxed)) < PROGRESS_EVERY_MS {
                return;
            }
            last.store(now, Ordering::Relaxed);
            let _ = app_handle.emit(
                "split_progress",
                serde_json::json!({ "path": path, "progress": p }),
            );
        },
        cancel,
    )
}

/// The stage each map (by its `part`; 0 is the read) was last sent at.
#[derive(Default)]
struct StageChanges(std::collections::HashMap<usize, &'static str>);

impl StageChanges {
    /// Whether map `part` is at a stage it wasn't last sent at; notes it.
    fn changed(&mut self, part: usize, stage: &'static str) -> bool {
        self.0.insert(part, stage) != Some(stage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_maps_stage_change_is_sent_even_between_others() {
        let mut s = StageChanges::default();
        assert!(s.changed(0, "reading"));
        assert!(!s.changed(0, "reading"));
        assert!(s.changed(1, "checking"));
        assert!(s.changed(2, "checking"));
        // Map 1's check goes on beside map 2's: only the clock lets it through.
        assert!(!s.changed(1, "checking"));
        assert!(s.changed(1, "finished"));
        assert!(!s.changed(2, "checking"));
        assert!(s.changed(2, "failed"));
    }
}
