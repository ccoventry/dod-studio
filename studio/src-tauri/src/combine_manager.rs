//! Combine Clips (#107): Tauri commands over `native::hlcr::combine`, which
//! joins rendered clips into one video.

use native::hlcr::combine::{self, CombinePlan};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub struct CombineManager {
    cancel: Arc<AtomicBool>,
}

fn paths(clips: &[String]) -> Vec<PathBuf> {
    clips.iter().map(PathBuf::from).collect()
}

/// How `clips` would be joined: without re-encoding, or not.
#[tauri::command]
pub async fn combine_plan(
    clips: Vec<String>,
    ffmpeg_path: Option<String>,
) -> Result<CombinePlan, String> {
    let ffmpeg = crate::render_manager::resolve_ffmpeg(ffmpeg_path.as_ref());
    combine::plan(&ffmpeg, &paths(&clips)).await
}

/// Joins `clips`, in order, into `output`. Emits `combine_progress`.
#[tauri::command]
pub async fn combine_clips(
    app_handle: AppHandle,
    state: tauri::State<'_, CombineManager>,
    clips: Vec<String>,
    output: String,
    ffmpeg_path: Option<String>,
) -> Result<(), String> {
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);
    let ffmpeg = crate::render_manager::resolve_ffmpeg(ffmpeg_path.as_ref());
    let plan = combine::plan(&ffmpeg, &paths(&clips)).await?;
    combine::combine(&ffmpeg, &plan, &PathBuf::from(output), cancel, |fraction| {
        let _ = app_handle.emit(
            "combine_progress",
            serde_json::json!({ "fraction": fraction }),
        );
    })
    .await
}

#[tauri::command]
pub fn combine_cancel(state: tauri::State<'_, CombineManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}
