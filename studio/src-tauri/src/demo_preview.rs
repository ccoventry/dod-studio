// demo_preview.rs
// Launch Preview: a demo patched with bookmarks at its highlights, opened in
// the game (or sent to a game already running), and the Clear Previews sweep
// of the preview files left behind.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::capture_manager::SerializedStreak;
use crate::game_process_manager::{
    LaunchRequest, launch_config, running_game_pids, saved_settings, watch_for_error_dialogs,
};
use native::log_markdown;
use native::patch::launch_settings::RunningGameCheck;
use native::patch::{
    CaptureStreak, PatchJob, PatcherConfig, StreamPatcher, build_preview_patch_jobs,
};
use serde::{Deserialize, Serialize};
use tauri::Manager;

// ── Bookmark Previews (.dodstudio_preview) ─────────────────────────────────────
//
// `build_preview_patch_jobs` (native/src/patch/builder.rs) groups a flat list
// of streaks by `source_demo` and, per demo, injects one `svc_director`
// STUFFTEXT "bookmark" event per selected highlight (plus MATCH_START/DEMO_END)
// into a copy of the original saved as `<stem>_preview.dem` — the events show
// up as named markers when the file is loaded through GoldSrc's `viewdemo` VCR
// UI. Each output file is marked with a hidden `.dodstudio_preview` sidecar so
// it's never mistaken for a real recorded demo.
//
// `launch_demo_preview` uses it for a single demo when the game can't play
// the original in place (#434), then launches HLAE via `+viewdemo` against
// the patched preview once it's on disk.

fn write_hidden_sidecar(path: &Path) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00000002); // FILE_ATTRIBUTE_HIDDEN
    }
    options.open(path)?;
    Ok(())
}

/// Validates the HLAE/hl.exe paths, ensures `<hl_parent>/dod` exists, and
/// builds a minimal `PatcherConfig` carrying just the fields
/// `build_hlae_process` reads (hlae_path/game_path/resolution/
/// goldsrc_hooks_dll_path). `goldsrc_hooks_dll_path` matters only to callers
/// that launch HLAE (`launch_demo_preview`); one that never spawns a process
/// passes `None`.
fn resolve_preview_env(
    hlae_path: &str,
    game_path: &str,
    goldsrc_hooks_dll_path: Option<String>,
) -> Result<(PatcherConfig, PathBuf), String> {
    if hlae_path.trim().is_empty() || game_path.trim().is_empty() {
        return Err(crate::messages::configure_paths_before("previewing"));
    }
    let hlae_p = Path::new(hlae_path);
    let hl_p = Path::new(game_path);
    if !hlae_p.is_file() {
        return Err(crate::messages::HLAE_NOT_FOUND_AT_CONFIGURED_PATH.to_string());
    }
    if !hl_p.is_file() {
        return Err(crate::messages::HL_NOT_FOUND_AT_CONFIGURED_PATH.to_string());
    }

    let dod_dir = native::shared::paths::mod_dir_for_exe(hl_p)
        .ok_or_else(|| crate::messages::COULD_NOT_RESOLVE_DOD_DIRECTORY.to_string())?;
    std::fs::create_dir_all(&dod_dir).map_err(crate::messages::failed_to_create_dod_directory)?;

    let patcher_config = PatcherConfig {
        hlae_path: hlae_path.to_string(),
        game_path: game_path.to_string(),
        goldsrc_hooks_dll_path: crate::updater_manager::allowed_hook_dll(goldsrc_hooks_dll_path),
        ..PatcherConfig::default()
    };
    Ok((patcher_config, dod_dir))
}

/// Builds one bookmark-preview `PatchJob` per source demo present in
/// `streaks` and patches it — unless a previous run already left a valid
/// `<stem>_preview.dem` + `.dodstudio_preview` sidecar in place, in which case
/// that existing file is reused as-is rather than regenerated. Returns the
/// full job list (every source demo, patched or reused — callers need the
/// resolved output path either way) alongside how many were freshly patched
/// this call, for a caller that wants to report "N generated" accurately.
fn patch_bookmark_previews(
    streaks: Vec<SerializedStreak>,
    dod_dir: &Path,
    patcher_config: &PatcherConfig,
) -> Result<(Vec<PatchJob>, usize), String> {
    if streaks.is_empty() {
        return Err(crate::messages::NO_HIGHLIGHTS_TO_PREVIEW.to_string());
    }
    let capture_streaks: Vec<CaptureStreak> =
        streaks.into_iter().map(CaptureStreak::from).collect();
    // Same check as a capture batch (#196): a preview patched from a
    // different file crashes the game the same way. Unwrapped: the toast says
    // "Preview failed" (`STRINGS.IPC.previewFailed`).
    native::patch::check_sources_unchanged(&capture_streaks)?;
    let jobs = build_preview_patch_jobs(capture_streaks, Some(dod_dir));
    if jobs.is_empty() {
        return Err(crate::messages::FAILED_TO_BUILD_PREVIEW_JOBS.to_string());
    }

    let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut generated = 0usize;
    for job in &jobs {
        let sidecar_path = job
            .output_demo
            .with_extension(native::shared::paths::PREVIEW_SIDECAR_EXT);
        if job.output_demo.is_file() && sidecar_path.is_file() {
            // Already previewed in an earlier session — the bookmark set is
            // derived purely from this demo's own highlights, which don't
            // change between scans, so there's nothing to regenerate.
            continue;
        }

        StreamPatcher::new(&job.source_demo, &job.output_demo)
            .patch(job, patcher_config, &cancel_token)
            .map_err(|e| crate::messages::failed_to_patch_preview_demo(&job.source_demo, e))?;

        write_hidden_sidecar(&sidecar_path)
            .map_err(crate::messages::failed_to_write_preview_sidecar)?;
        generated += 1;
    }
    Ok((jobs, generated))
}

/// Patches the given demo's highlights into a single bookmarked
/// `<stem>_preview.dem` — reusing one already on disk from an earlier run
/// instead of regenerating it — and immediately launches HLAE against it via
/// `+viewdemo <stem>_preview`.
#[tauri::command]
pub async fn launch_demo_preview(
    app: tauri::AppHandle,
    hlae_path: String,
    game_path: String,
    streaks: Vec<SerializedStreak>,
    goldsrc_hooks_dll_path: Option<String>,
) -> Result<(), String> {
    // The saved resolution, as Launch Game uses it (#358): the preview
    // config otherwise keeps PatcherConfig's 1280x720 default.
    let resolution = {
        let settings_state = app.state::<crate::settings_manager::SettingsManager>();
        let guard = settings_state
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        (guard.resolution_width, guard.resolution_height)
    };
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let (mut patcher_config, dod_dir) =
            resolve_preview_env(&hlae_path, &game_path, goldsrc_hooks_dll_path)?;
        (
            patcher_config.resolution_width,
            patcher_config.resolution_height,
        ) = resolution;
        // The original demo, untouched, when the game can play it in place (#434).
        if let Some(arg) = crate::preview_in_place::plan(&patcher_config, &dod_dir, &streaks) {
            return crate::preview_in_place::launch(app, &patcher_config, &arg);
        }
        let (jobs, _generated) = patch_bookmark_previews(streaks, &dod_dir, &patcher_config)?;
        let job = jobs
            .first()
            .ok_or_else(|| crate::messages::FAILED_TO_BUILD_PREVIEW_PATCH_JOB.to_string())?;

        let preview_stem = job
            .output_demo
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| crate::messages::COULD_NOT_RESOLVE_PREVIEW_FILE_STEM.to_string())?;

        // With the hook DLL, the demo goes over its pipe once the game is up:
        // on the launch line its map loads before config.cfg/movie.cfg set
        // brightness and gamma, so the world is built darker than the models
        // are lit (#662, the same as #705 for batches). Without the DLL there
        // is no pipe, so the launch line keeps it.
        let line = format!("viewdemo {preview_stem}");
        let over_pipe = patcher_config.goldsrc_hooks_dll().is_some();
        let before = running_game_pids();
        let extra = if over_pipe {
            String::new()
        } else {
            format!("+{line}")
        };
        let mut cmd = patcher_config.build_hlae_process(&extra);
        let launcher = native::sys::launcher::Launcher::spawn(&mut cmd)
            .map_err(crate::messages::failed_to_launch_hlae_for_preview)?;
        watch_for_error_dialogs(app, launcher);
        if over_pipe {
            std::thread::spawn(move || {
                crate::preview_in_place::send_when_ready(&before, &[line]);
            });
        }

        Ok(())
    }))
    .await
}

/// Launch Preview for a game that is already running (#413): patches the
/// same `<stem>_preview.dem` `launch_demo_preview` would (reusing one already
/// on disk), then, instead of launching a second game, sends
/// `viewdemo <stem>_preview` to the running one over its hook DLL's pipe.
///
/// Resolves to the command sent, or `None` when no running game takes
/// commands -- one Studio didn't start, with its hooks off, or started with
/// other launch settings (#666) -- so the caller can fall back to the
/// "already running" prompt.
#[tauri::command]
pub async fn send_preview_to_running_game(
    app: tauri::AppHandle,
    hlae_path: String,
    game_path: String,
    streaks: Vec<SerializedStreak>,
    goldsrc_hooks_dll_path: Option<String>,
) -> Result<Option<String>, String> {
    let request = LaunchRequest {
        hlae_path: Some(hlae_path.clone()),
        game_path: Some(game_path.clone()),
        goldsrc_hooks_dll_path: goldsrc_hooks_dll_path.clone(),
        ..LaunchRequest::default()
    };
    let launch = launch_config(&saved_settings(&app), &request);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        // Only a game started with the settings a launch would use now: a
        // preview sent to any other plays at the old resolution or install.
        let pids =
            match native::patch::launch_settings::check_running_game(&launch.launch_settings()) {
                RunningGameCheck::Match { pid } => vec![pid],
                RunningGameCheck::None | RunningGameCheck::Mismatch { .. } => return Ok(None),
            };
        let (patcher_config, dod_dir) =
            resolve_preview_env(&hlae_path, &game_path, goldsrc_hooks_dll_path)?;
        if let Some(arg) = crate::preview_in_place::plan(&patcher_config, &dod_dir, &streaks) {
            return crate::preview_in_place::send_to_running(&pids, &arg);
        }
        let (jobs, _generated) = patch_bookmark_previews(streaks, &dod_dir, &patcher_config)?;
        let job = jobs
            .first()
            .ok_or_else(|| crate::messages::FAILED_TO_BUILD_PREVIEW_PATCH_JOB.to_string())?;
        let preview_stem = job
            .output_demo
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| crate::messages::COULD_NOT_RESOLVE_PREVIEW_FILE_STEM.to_string())?;

        let command = format!("viewdemo {preview_stem}");
        for pid in pids {
            match native::sys::game_remote::send_console_commands(
                pid,
                std::slice::from_ref(&command),
            )
            .map_err(crate::messages::failed_to_send_to_running_game)?
            {
                native::sys::game_remote::Sent::Delivered => {
                    log_markdown(&format!(
                        "[preview] sent \"{command}\" to the running game (pid {pid})"
                    ));
                    return Ok(Some(command));
                }
                native::sys::game_remote::Sent::NotListening => continue,
            }
        }
        Ok(None)
    }))
    .await
}

// ── Clear Previews audit (orphaned *_preview.dem sweep) ────────────────────────
//
// Bookmark previews (see the block comment above `patch_bookmark_previews`)
// pile up in `<hl>/dod` across capture sessions when `auto_clear_previews`
// is off. These two commands back the "Clear Previews" audit modal: scan for
// leftovers and let the user purge them on confirmation. A file only counts
// as an orphaned preview if it still carries its hidden `.dodstudio_preview`
// sidecar — the same marker `patch_bookmark_previews` stamps on every file
// it generates — so a real demo that happens to end in `_preview.dem` is
// never swept up.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewFileSummary {
    pub demo_path: String,
    pub sidecar_path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub modified_unix_secs: f64,
}

/// Resolves the game folder to sweep (`dod` unless the Game folder setting
/// says otherwise, #402), accepting either an `hl.exe` path or that folder
/// itself (mirrors `resolve_preview_env`'s hl.exe-relative resolution, but
/// tolerates being pointed at the folder directly since the frontend may pass
/// either).
fn resolve_dod_dir_for_sweep(game_dir: &str) -> Result<PathBuf, String> {
    let p = Path::new(game_dir);
    if p.is_file() {
        return native::shared::paths::mod_dir_for_exe(p)
            .ok_or_else(|| crate::messages::COULD_NOT_RESOLVE_DOD_DIRECTORY.to_string());
    }
    if p.is_dir() {
        let game_folder = native::shared::paths::game_folder();
        let is_dod_dir = p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&game_folder));
        if is_dod_dir {
            return Ok(p.to_path_buf());
        }
        return Ok(native::shared::paths::mod_dir(p));
    }
    Err(crate::messages::game_directory_not_found(game_dir))
}

/// Sweeps `<hl>/dod` for orphaned bookmark-preview demos and reports them
/// (with combined demo + sidecar size) for the audit modal.
#[tauri::command]
pub async fn scan_orphaned_previews(game_dir: String) -> Result<Vec<PreviewFileSummary>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let dod_dir = resolve_dod_dir_for_sweep(&game_dir)?;
        if !dod_dir.is_dir() {
            return Ok(Vec::new());
        }

        let entries =
            std::fs::read_dir(&dod_dir).map_err(crate::messages::failed_to_read_dod_directory)?;

        let mut results = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !file_name.to_lowercase().ends_with("_preview.dem") {
                continue;
            }

            let Some(sidecar_path) = native::shared::paths::preview_sidecar(&path) else {
                continue;
            };

            let demo_meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let sidecar_size = std::fs::metadata(&sidecar_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let modified_unix_secs = demo_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            results.push(PreviewFileSummary {
                demo_path: path.to_string_lossy().to_string(),
                sidecar_path: sidecar_path.to_string_lossy().to_string(),
                file_name,
                size_bytes: demo_meta.len() + sidecar_size,
                modified_unix_secs,
            });
        }

        results.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        Ok(results)
    }))
    .await
}

/// Deletes the given orphaned preview demos and their `.dodstudio_preview`
/// sidecars. `file_paths` must be `demo_path` values as reported by
/// `scan_orphaned_previews` — anything not ending in `_preview.dem` is
/// skipped rather than deleted. Returns the count of demo files removed; a
/// missing sidecar does not fail the entry.
#[tauri::command]
pub async fn delete_orphaned_previews(file_paths: Vec<String>) -> Result<u32, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let mut deleted: u32 = 0;
        for demo_path in file_paths {
            let path = PathBuf::from(&demo_path);
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !file_name.to_lowercase().ends_with("_preview.dem") {
                continue;
            }

            let sidecar_path = native::shared::paths::preview_sidecar(&path);
            let demo_removed = std::fs::remove_file(&path).is_ok();
            if let Some(sidecar_path) = sidecar_path {
                let _ = std::fs::remove_file(&sidecar_path);
            }

            if demo_removed {
                deleted += 1;
            }
        }
        Ok(deleted)
    }))
    .await
}
