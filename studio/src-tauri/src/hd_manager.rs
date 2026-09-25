//! The HD Textures page's backend (#372): what is built, and fetching the
//! upscaler. The work itself is in `native::hd`; this is the Tauri surface.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use native::hd::{self, HdStatus, setup::SetupOutcome};
use tauri::{AppHandle, Emitter, State};

/// One setup run at a time, and a way to stop it.
#[derive(Default)]
pub struct HdManager {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

/// What is built under `<game>\dod\dodstudio_hd`, and whether the upscaler
/// and its models are downloaded. `game_path` is the `hl.exe` the
/// Configuration page holds.
#[tauri::command]
pub async fn hd_status(game_path: String) -> Result<HdStatus, String> {
    let game_path = game_path.trim().to_string();
    if game_path.is_empty() {
        return Err(crate::messages::HD_NEEDS_GAME_PATH.to_string());
    }
    let root = hd::hd_root(Path::new(&game_path))
        .ok_or_else(|| crate::messages::HD_NEEDS_GAME_PATH.to_string())?;
    // A big HD folder is tens of thousands of files; walking it is not
    // something to do on the async runtime's own threads.
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        hd::scan(&root, &hd::setup::tools_dir())
    }))
    .await
}

/// Downloads the upscaler and style models into the app's own folder,
/// skipping what is already there. Emits `hd_setup_progress` at most ~30
/// times a second.
#[tauri::command]
pub async fn hd_setup_tools(
    app: AppHandle,
    state: State<'_, HdManager>,
) -> Result<SetupOutcome, String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(crate::messages::HD_SETUP_ALREADY_RUNNING.to_string());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let running = Arc::clone(&state.running);
    let cancel = Arc::clone(&state.cancel);

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        // Throttled to ~30fps per CLAUDE.md's telemetry-throttling guardrail:
        // `run` reports every 64 KiB chunk, thousands of times a download.
        let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let mut last_item = String::new();
        hd::setup::run(&hd::setup::tools_dir(), &cancel, &mut |progress| {
            let now = std::time::Instant::now();
            // Always pass on a new file or the unpack step, so the line never
            // shows a finished file while the next one is under way.
            let changed = progress.item != last_item || progress.unpacking;
            if changed || now.duration_since(last_emit) >= std::time::Duration::from_millis(33) {
                last_emit = now;
                last_item.clone_from(&progress.item);
                let _ = app.emit("hd_setup_progress", progress);
            }
        })
    }))
    .await;

    running.store(false, Ordering::SeqCst);
    result
}

/// Stops a running [`hd_setup_tools`] at its next chunk; what was complete
/// stays, the file in progress is removed.
#[tauri::command]
pub fn hd_setup_cancel(state: State<'_, HdManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}
