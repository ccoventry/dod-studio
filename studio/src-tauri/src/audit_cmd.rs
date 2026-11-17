//! The Demo Auditor's commands (#1174): running an audit, deleting the copies
//! it finds, and the two Explorer shortcuts beside its results. The audit
//! itself is `audit_manager.rs`.

use crate::audit_manager::{self, AuditManager, AuditOutcome, DeleteOutcome};
use crate::settings_manager::SettingsManager;
use lock_recover::lock_or_recover;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// `paths` plus every fixed drive when `all_drives`, in one key pass (#844).
/// `queued` is the project's queue: copies there, or in the install's game
/// folder, are kept first.
#[tauri::command]
pub async fn run_demo_audit(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AuditManager>,
    settings: tauri::State<'_, SettingsManager>,
    paths: Vec<String>,
    all_drives: bool,
    queued: Vec<String>,
    look_in_zips: Option<bool>,
) -> Result<AuditOutcome, String> {
    let hl_path = lock_or_recover(&settings.inner).hl_path.clone();
    let game_folders: Vec<PathBuf> = native::shared::paths::mod_dir_for_exe(Path::new(&hl_path))
        .filter(|_| !hl_path.trim().is_empty())
        .into_iter()
        .collect();
    audit_manager::run_demo_audit_impl(
        app_handle,
        Arc::clone(&state.is_running),
        Arc::clone(&state.cancel_token),
        paths,
        all_drives,
        look_in_zips.unwrap_or(false),
        (game_folders, queued),
    )
    .await
}

/// The roots "All drives" walks, for its label.
#[tauri::command]
pub async fn list_fixed_drives() -> Result<Vec<String>, String> {
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(
        audit_manager::fixed_drive_roots,
    ))
    .await
}

/// To the Recycle Bin, or for good when `permanent` (#844), each copy only
/// once it matches its group's kept copy byte for byte.
#[tauri::command]
pub async fn delete_audit_files(
    app_handle: tauri::AppHandle,
    copies: Vec<audit_manager::DeleteCopy>,
    permanent: bool,
) -> Result<DeleteOutcome, String> {
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        audit_manager::delete_audit_files_impl(&app_handle, copies, permanent)
    }))
    .await
}

#[tauri::command]
pub fn cancel_audit(state: tauri::State<'_, AuditManager>) -> Result<(), String> {
    state.cancel_token.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command(async)]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    audit_manager::reveal_in_explorer_impl(path)
}

/// Opens Windows' Recycle Bin (the Demo Auditor's deletes go there).
#[tauri::command(async)]
pub fn open_recycle_bin() -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg("shell:RecycleBinFolder")
            .spawn()
            .map_err(crate::messages::failed_to_open_explorer)?;
    }
    Ok(())
}
