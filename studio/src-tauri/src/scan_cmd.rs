//! The demo folder scan's commands and its state (#1174). The scan itself is
//! `scan_manager.rs`.

use crate::messages;
use crate::scan_manager;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct ScanManager {
    pub is_scanning: Arc<AtomicBool>,
    pub cancel_token: Arc<AtomicBool>,
}

impl Default for ScanManager {
    fn default() -> Self {
        Self {
            is_scanning: Arc::new(AtomicBool::new(false)),
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[tauri::command]
pub async fn scan_directory(
    app_handle: tauri::AppHandle,
    scan_state: tauri::State<'_, ScanManager>,
    paths: Vec<String>,
    known: Option<Vec<scan_manager::KnownDemo>>,
    workers: Option<usize>,
    include_subfolders: Option<bool>,
) -> Result<scan_manager::ScanOutcome, String> {
    scan_manager::scan_directory_impl(
        app_handle,
        Arc::clone(&scan_state.is_scanning),
        Arc::clone(&scan_state.cancel_token),
        paths,
        known.unwrap_or_default(),
        workers.unwrap_or(scan_manager::SCAN_CONCURRENCY),
        // Missing: today's walk into every subfolder (#531).
        include_subfolders.unwrap_or(true),
    )
    .await
}

/// What a rescan of `paths` has to do, for the rescan prompt's time line
/// (#937): stats each demo and reads its cache entries' first bytes, opening
/// no demo, so a few hundred take well under a second (longer on a slow or
/// cloud drive, hence off the main thread).
#[tauri::command]
pub async fn estimate_rescan(
    paths: Vec<String>,
) -> Result<analysis::cache::RescanEstimate, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(analysis::cache::RescanEstimate::new(
            &native::analyzer_cache_root(),
            &paths,
        ))
    }))
    .await
}

/// Total physical RAM in bytes, for the scan worker box's hint line (#246).
#[tauri::command]
pub async fn system_memory_bytes() -> Result<u64, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(|| {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        Ok(sys.total_memory())
    }))
    .await
}

#[tauri::command]
pub fn cancel_scan(scan_state: tauri::State<'_, ScanManager>) -> Result<(), String> {
    scan_state.cancel_token.store(true, Ordering::SeqCst);
    Ok(())
}
