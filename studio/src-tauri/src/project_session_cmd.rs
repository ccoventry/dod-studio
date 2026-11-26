//! The project session commands (#1174). File I/O for paths the user picked
//! happens here in Rust (std::fs), same as the settings commands. Tauri's fs
//! plugin was dropped: its default scope covers only the app's own
//! config/data dirs, so it could never reach a path a save/open dialog
//! returns.

use crate::file_replace;
use crate::messages;
use crate::scan_manager;

#[tauri::command]
pub async fn save_project_session(path: String, contents: String) -> Result<(), String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        file_replace::replace_file(std::path::Path::new(&path), &contents)
    }))
    .await
}

/// `Documents\dod-studio\projects`, made if it's missing: where Save and
/// Load Project start (#354), so project files don't land wherever the last
/// file dialog happened to be.
#[tauri::command]
pub async fn default_projects_dir() -> Result<String, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(|| {
        let dir = dirs::document_dir()
            .ok_or_else(|| messages::NO_DOCUMENTS_FOLDER.to_string())?
            .join("dod-studio")
            .join("projects");
        std::fs::create_dir_all(&dir)
            .map_err(|e| messages::failed_to_write_file(&dir.to_string_lossy(), e))?;
        Ok(dir.to_string_lossy().to_string())
    }))
    .await
}

/// Which of a loaded project's demos are missing, and where each one moved,
/// if a file with the same key turns up in `search_dirs` (#21). Emits
/// `missing_demos_search` once, `{ missing }`, when the search for them
/// starts (#958).
#[tauri::command]
pub async fn locate_missing_demos(
    app_handle: tauri::AppHandle,
    demos: Vec<scan_manager::KnownDemo>,
    search_dirs: Vec<String>,
) -> Result<Vec<scan_manager::MissingDemo>, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use tauri::Emitter;
        let dirs: Vec<std::path::PathBuf> =
            search_dirs.iter().map(std::path::PathBuf::from).collect();
        Ok(scan_manager::locate_missing_demos(
            &demos,
            &dirs,
            &mut |missing| {
                let _ = app_handle.emit(
                    "missing_demos_search",
                    serde_json::json!({ "missing": missing }),
                );
            },
        ))
    }))
    .await
}

/// Which demos are no longer the file they were scanned from (#21). Emits
/// `changed_demos_progress` ({ done, total }, at most every ~33 ms and on
/// the last demo) for Start Capture's line (#953).
#[tauri::command]
pub async fn changed_demos(
    app_handle: tauri::AppHandle,
    demos: Vec<scan_manager::KnownDemo>,
) -> Result<Vec<String>, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use tauri::Emitter;
        let started = std::time::Instant::now();
        let last_emit = std::sync::atomic::AtomicU32::new(u32::MAX);
        Ok(scan_manager::changed_demos_with_progress(
            &demos,
            |done, total| {
                let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
                if scan_manager::changed_progress_due(&last_emit, now_ms, done, total) {
                    let _ = app_handle.emit(
                        "changed_demos_progress",
                        serde_json::json!({ "done": done, "total": total }),
                    );
                }
            },
        ))
    }))
    .await
}

#[tauri::command]
pub async fn load_project_session(path: String) -> Result<String, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        std::fs::read_to_string(&path).map_err(|e| messages::failed_to_read_file(&path, e))
    }))
    .await
}

/// One `true` per path that is a file that's there: File → Open Recent dims
/// the rest (#935). Off the main thread, since a path on a sleeping network
/// or cloud drive can take seconds to answer (#789).
#[tauri::command]
pub async fn files_exist(paths: Vec<String>) -> Result<Vec<bool>, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(paths
            .iter()
            .map(|p| std::path::Path::new(p).is_file())
            .collect())
    }))
    .await
}
