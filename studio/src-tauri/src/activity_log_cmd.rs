//! The activity log's two commands (#1174): a line from the frontend, and
//! where today's file is.

/// Writes one line to today's activity log from the frontend. Exists for
/// events that have no other Tauri command to piggyback logging on — the
/// Master Demo Queue's Clear Untracked/Selected/All and row-delete actions
/// are pure frontend array mutations with nothing else calling into Rust —
/// and every toast (#873). Off the window's thread: each line is flushed to
/// disk under the log's lock, which a capture's own logging can hold while
/// the disk is busy with frames. `logFrontendEvent` sends one line at a time,
/// so a burst (a rescan's carry report) still lands in order.
#[tauri::command(async)]
pub fn log_frontend_event(message: String) {
    native::log_markdown(&message);
}

/// Absolute path to today's activity log, for the top nav's "View Logs"
/// button — most users won't know to go looking under AppData on their own.
#[tauri::command]
pub fn get_activity_log_path() -> String {
    native::activity_log_path().to_string_lossy().to_string()
}
