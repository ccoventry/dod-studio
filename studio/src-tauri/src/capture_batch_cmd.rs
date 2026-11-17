//! The commands that start and cancel a capture batch (#1174). The batch
//! itself is `capture_manager.rs`.

use crate::capture_manager::{self, CaptureManager, CapturePayload};

/// Triggers a batch capture run from the Vite frontend.
///
/// Async so the Tauri dispatcher never blocks the main thread.
/// The heavy lifting is delegated inside `start_capture_batch_impl`
/// via `tokio::task::spawn_blocking`.
#[tauri::command]
pub async fn start_capture_batch(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, CaptureManager>,
    payload: CapturePayload,
) -> Result<(), String> {
    capture_manager::start_capture_batch_impl(app_handle, &state, payload).await
}

/// Cancel a running capture batch.
#[tauri::command]
pub async fn cancel_capture_batch(state: tauri::State<'_, CaptureManager>) -> Result<(), String> {
    native::log_markdown(&format!(
        "[capture] Cancel requested (is_running={})",
        state.is_running()
    ));
    state
        .cancel_token
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}
