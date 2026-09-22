use hl_demo_auditor::{find_duplicates, scan_dir, AuditProgress};
use serde::{Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use tauri::{AppHandle, Emitter};

pub struct AuditManager {
    pub is_running: Arc<AtomicBool>,
    pub cancel_token: Arc<AtomicBool>,
}

impl Default for AuditManager {
    fn default() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Serialize)]
pub struct SerializedDuplicateGroup {
    pub size: u64,
    pub header_hash: u64,
    pub files: Vec<String>,
}

pub async fn run_demo_audit_impl(
    app_handle: AppHandle,
    is_running: Arc<AtomicBool>,
    cancel_token: Arc<AtomicBool>,
    paths: Vec<String>,
) -> Result<Vec<SerializedDuplicateGroup>, String> {
    is_running.store(true, Ordering::SeqCst);
    cancel_token.store(false, Ordering::SeqCst);
    let is_running_end = Arc::clone(&is_running);

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let (tx, rx) = mpsc::channel();
        
        // Spawn a thread to forward progress events to Tauri frontend
        let ah = app_handle.clone();
        let cancel_clone = cancel_token.clone();
        let mut files_checked = 0;
        
        let progress_handle = std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                if cancel_clone.load(Ordering::Relaxed) { break; }
                match msg {
                    AuditProgress::Scanning(p) => {
                        let _ = ah.emit("audit_progress", serde_json::json!({
                            "files_checked": files_checked,
                            "duplicates_found": 0,
                            "wasted_bytes": 0,
                            "status": format!("Scanning folder: {}", p)
                        }));
                    }
                    AuditProgress::Hashing(p) => {
                        files_checked += 1;
                        let _ = ah.emit("audit_progress", serde_json::json!({
                            "files_checked": files_checked,
                            "duplicates_found": 0,
                            "wasted_bytes": 0,
                            "status": format!("Checking: {}", p)
                        }));
                    }
                    _ => {}
                }
            }
        });

        let mut files = vec![];
        for path_str in paths {
            let path_buf = PathBuf::from(path_str);
            if path_buf.exists() {
                scan_dir(&path_buf, &mut files, &cancel_token, &Some(tx.clone()));
            }
        }

        let (_, duplicate_groups, dup_count, space_wasted_bytes) = 
            find_duplicates(files, &cancel_token, &Some(tx));

        // Signal progress thread to exit and wait for it
        let _ = progress_handle.join();

        let _ = app_handle.emit("audit_progress", serde_json::json!({
            "files_checked": files_checked,
            "duplicates_found": dup_count,
            "wasted_bytes": space_wasted_bytes,
            "status": if cancel_token.load(Ordering::Relaxed) { "Cancelled" } else { "Complete" }
        }));

        let serialized: Vec<SerializedDuplicateGroup> = duplicate_groups
            .into_iter()
            .map(|g| SerializedDuplicateGroup {
                size: g.key.size,
                header_hash: g.key.header_hash,
                files: g.files.into_iter().map(|f| f.to_string_lossy().to_string()).collect(),
            })
            .collect();

        Ok(serialized)
    }))
    .await;

    is_running_end.store(false, Ordering::SeqCst);
    result
}

pub fn delete_audit_files_impl(paths: Vec<String>) -> Result<(), String> {
    for path in paths {
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!("Failed to delete audit file {}: {}", path, e);
            return Err(crate::messages::failed_to_delete_audit_file(&path, e));
        }
    }
    Ok(())
}

/// Reveals a file in the OS file explorer, selecting it if the platform
/// supports that. Mirrors dev's `views/auditor.rs` cross-platform match.
pub fn reveal_in_explorer_impl(path: String) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Err(crate::messages::path_no_longer_exists(&path));
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path_buf)
            .spawn()
            .map_err(crate::messages::failed_to_open_explorer)?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path_buf)
            .spawn()
            .map_err(crate::messages::failed_to_open_finder)?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux fallback: no universal "select this file" behavior, so just
        // open the parent directory via the desktop's default file manager.
        let parent = path_buf.parent().ok_or(crate::messages::NO_PARENT_DIRECTORY_FOR_PATH)?;
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(crate::messages::failed_to_open_folder)?;
    }

    Ok(())
}
