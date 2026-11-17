//! Free space and path checks for the capture folders (#1174): how much room
//! the Render and Capture tabs have, and which of the folders listed is
//! usable.

use crate::messages;

/// Free space across the capture folders, each drive counted once: two
/// folders on one drive share its space. On the blocking pool (#853): a
/// cache miss re-lists every volume, which can wait on a sleeping or network
/// drive, and start-up asks for this alongside the other disk queries.
#[tauri::command]
pub async fn calculate_export_pool_space(paths: Vec<String>) -> Result<u64, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        export_pool_space(paths)
    }))
    .await
}

fn export_pool_space(paths: Vec<String>) -> Result<u64, String> {
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
    Ok(native::sys::disk::pool_free_bytes(&paths))
}

/// The capture folders' free space and the part captures may use, which
/// leaves out what "Keep free on the Windows drive" keeps there (#1430), so
/// Start Capture Batch's check can name the setting when it's what's short.
/// On the blocking pool for the same reason as `calculate_export_pool_space`.
#[tauri::command]
pub async fn capture_pool_room(paths: Vec<String>) -> Result<native::sys::disk::PoolRoom, String> {
    messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
        native::sys::disk::pool_room(&paths)
    }))
    .await
}

#[derive(serde::Serialize)]
pub struct CaptureOutputDiagnostic {
    path: String,
    status: &'static str, // "ok" | "not_absolute" | "malformed" | "not_found" | "not_a_directory"
    // Whether get_available_bytes (the same function backing the aggregate
    // sum/footer) would actually count this path — a "not_found" path whose
    // drive is real and mounted still passes (many output folders are
    // auto-created at write time), while one on an unmounted/nonexistent
    // drive doesn't. Reusing that exact check here, rather than deriving a
    // second opinion from `status` alone, is what keeps this list and the
    // footer's byte total from disagreeing about the same path.
    usable: bool,
}

/// On the blocking pool for the same reason as `calculate_export_pool_space`.
#[tauri::command]
pub async fn diagnose_capture_output_paths(
    paths: Vec<String>,
) -> Result<Vec<CaptureOutputDiagnostic>, String> {
    messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        capture_output_diagnostics(paths)
    }))
    .await
}

fn capture_output_diagnostics(paths: Vec<String>) -> Vec<CaptureOutputDiagnostic> {
    paths
        .into_iter()
        .map(|path_str| {
            let p = std::path::PathBuf::from(&path_str);
            let status = match native::sys::disk::diagnose_path(&p) {
                native::sys::disk::PathStatus::Ok => "ok",
                native::sys::disk::PathStatus::NotAbsolute => "not_absolute",
                native::sys::disk::PathStatus::Malformed => "malformed",
                native::sys::disk::PathStatus::NotFound => "not_found",
                native::sys::disk::PathStatus::NotADirectory => "not_a_directory",
            };
            let usable = native::sys::disk::get_available_bytes(&p).is_some();
            CaptureOutputDiagnostic {
                path: path_str,
                status,
                usable,
            }
        })
        .collect()
}
