//! The Demo Auditor's Consolidate (#872): Tauri commands over
//! `native::demo_consolidate`. Every command runs off the main thread; a
//! move between drives copies 100+ GB, with byte progress and Cancel.

use native::demo_consolidate::{self, Outcome, Plan, Progress, Request, UndoableBatch};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub struct ConsolidateManager {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

fn log_dir() -> PathBuf {
    native::shared::paths::get_appdata_dir().join(demo_consolidate::LOG_FOLDER)
}

/// `consolidate_progress`, at most every ~33 ms (CLAUDE.md), plus the last
/// report of each step so the bar ends full.
fn emitter(app_handle: AppHandle) -> impl FnMut(&Progress) {
    let started = Instant::now();
    let last_emit = AtomicU32::new(u32::MAX);
    move |p: &Progress| {
        let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
        let step_done = p.files_done == p.files_total;
        if step_done || crate::capture_manager::progress_emit_due(&last_emit, now_ms) {
            let _ = app_handle.emit("consolidate_progress", p.clone());
        }
    }
}

/// Runs `job` with the cancel token cleared, refusing a second job while
/// one runs.
async fn exclusive<T: Send + 'static>(
    state: &ConsolidateManager,
    job: impl FnOnce(&AtomicBool) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(crate::messages::CONSOLIDATE_ALREADY_RUNNING.to_string());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = Arc::clone(&state.cancel);
    let result =
        crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || job(&cancel)))
            .await;
    state.running.store(false, Ordering::SeqCst);
    result
}

/// What Consolidate would do, for the preview. Reads the disk only.
#[tauri::command]
pub async fn consolidate_plan(request: Request) -> Result<Plan, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        demo_consolidate::plan_on_disk(&request)
    }))
    .await
}

/// Moves the demos and recycles the copies, logging the moves for Undo.
#[tauri::command]
pub async fn consolidate_run(
    app_handle: AppHandle,
    state: tauri::State<'_, ConsolidateManager>,
    request: Request,
) -> Result<Outcome, String> {
    exclusive(state.inner(), move |cancel| {
        let mut emit = emitter(app_handle);
        demo_consolidate::run(&request, &log_dir(), cancel, &mut emit)
    })
    .await
}

/// Puts the newest batch's demos back where they were.
#[tauri::command]
pub async fn consolidate_undo(
    app_handle: AppHandle,
    state: tauri::State<'_, ConsolidateManager>,
) -> Result<Outcome, String> {
    exclusive(state.inner(), move |cancel| {
        let mut emit = emitter(app_handle);
        Ok(demo_consolidate::undo_last(&log_dir(), cancel, &mut emit))
    })
    .await
}

/// The batch Undo would put back, if any. It reads every log.
#[tauri::command]
pub async fn consolidate_undoable() -> Result<Option<UndoableBatch>, String> {
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        demo_consolidate::undoable(&log_dir())
    }))
    .await
}

#[tauri::command]
pub fn consolidate_cancel(state: tauri::State<'_, ConsolidateManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}
