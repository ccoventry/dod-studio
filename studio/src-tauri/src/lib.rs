mod audit_manager;
mod batch_catch_up;
mod batch_close;
mod batch_eta;
mod blender_manager;
mod builtin_profiles;
mod capture_manager;
mod capture_room_manager;
mod combine_manager;
mod crash_maps_manager;
mod dem_open_with;
mod demo_cache_cmd;
mod demo_links_manager;
mod demo_preview;
mod demo_rename_manager;
mod demo_split_manager;
mod dir_browser;
mod download_progress;
mod drop_paths;
mod game_process_manager;
mod hd_manager;
mod hook_batch;
mod hook_dll_list;
mod installs_manager;
mod manifest_file;
mod map_manager;
mod match_group_cmd;
mod menu_scale_manager;
mod messages;
mod movie_copy_manager;
mod obs_manager;
mod obs_test_manager;
mod open_files;
mod overview_manager;
mod packet_limit_manager;
mod pick_paths;
mod players_tab;
mod preview_in_place;
mod project_convert;
mod render_manager;
mod review_library;
mod review_manager;
mod scan_manager;
mod settings_manager;
mod take_checks_manager;
mod take_cleanup_manager;
mod take_delete_manager;
mod take_preview_manager;
mod updater_manager;

/// Helpers this crate's own tests share. See `Scratch` on why a temporary
/// directory needs a guard rather than a trailing `remove_dir_all` (#253).
#[cfg(test)]
mod test_support;

use audit_manager::{AuditManager, AuditOutcome, DeleteOutcome};
use capture_manager::{CaptureManager, CapturePayload, read_cfg_commands};
use demo_preview::{
    delete_orphaned_previews, launch_demo_preview, scan_orphaned_previews,
    send_preview_to_running_game,
};
use game_process_manager::{
    check_engine_processes, kill_engine_processes, launch_standalone_game, start_steam, steam_state,
};
use obs_manager::launch_obs;
use render_manager::{
    RenderManager, cancel_render_batch, cancel_render_job, cancel_render_jobs,
    check_render_autosave, clear_render_queue, discard_render_autosave, get_export_pool_free_gb,
    get_render_jobs, get_render_required_estimate_gb, queue_render_batch, recover_render_batch,
    remove_non_rendering_render_jobs, remove_render_job, remove_render_jobs, reset_all_render_jobs,
    reset_render_job, reset_render_jobs, set_render_job_codec, start_queued_render,
};
use settings_manager::{AppSettings, SettingsManager};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

// ── ScanManager ────────────────────────────────────────────────────────────────

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

// ── Settings IPC Commands ──────────────────────────────────────────────────────

#[tauri::command]
async fn get_settings(state: tauri::State<'_, SettingsManager>) -> Result<AppSettings, String> {
    let inner_arc = Arc::clone(&state.inner);
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let guard = inner_arc.lock().unwrap_or_else(|p| p.into_inner());
        Ok(guard.clone())
    }))
    .await
}

#[tauri::command]
async fn save_settings(
    state: tauri::State<'_, SettingsManager>,
    settings: AppSettings,
) -> Result<(), String> {
    let inner_arc = Arc::clone(&state.inner);
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let mut settings = settings;
        // Refused before anything is written: a game folder that is a path
        // would put the launch and every file Studio writes somewhere other
        // than beside hl.exe (#402).
        native::shared::paths::validate_game_folder(&settings.game_folder)?;
        let mut guard = inner_arc.lock().unwrap_or_else(|p| p.into_inner());
        // The frontend doesn't carry this field; the loaded value stands, or a
        // migration would run again at every start.
        settings.settings_migrations = guard.settings_migrations.max(settings.settings_migrations);
        settings.save()?;
        settings_manager::apply_game_folder(&settings);
        players_tab::mirror_table(&settings);
        *guard = settings;
        Ok(())
    }))
    .await
}

/// The built-in command profiles (#911), which the Commands tab lists locked
/// beside the user's own. Never saved: every update replaces them.
#[tauri::command]
fn builtin_command_profiles() -> Vec<settings_manager::CommandProfile> {
    builtin_profiles::builtin_command_profiles()
}

// ── Project Session IPC Commands ───────────────────────────────────────────────
// File I/O for paths the user picked happens here in Rust (std::fs), same as
// `save_settings`/`get_settings` above. Tauri's fs plugin was dropped: its
// default scope covers only the app's own config/data dirs, so it could never
// reach a path a save/open dialog returns.

#[tauri::command]
async fn save_project_session(path: String, contents: String) -> Result<(), String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        std::fs::write(&path, contents).map_err(|e| messages::failed_to_write_file(&path, e))
    }))
    .await
}

/// `Documents\dod-studio\projects`, made if it's missing: where Save and
/// Load Project start (#354), so project files don't land wherever the last
/// file dialog happened to be.
#[tauri::command]
async fn default_projects_dir() -> Result<String, String> {
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
async fn locate_missing_demos(
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
async fn changed_demos(
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
async fn load_project_session(path: String) -> Result<String, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        std::fs::read_to_string(&path).map_err(|e| messages::failed_to_read_file(&path, e))
    }))
    .await
}

/// One `true` per path that is a file that's there: File → Open Recent dims
/// the rest (#935). Off the main thread, since a path on a sleeping network
/// or cloud drive can take seconds to answer (#789).
#[tauri::command]
async fn files_exist(paths: Vec<String>) -> Result<Vec<bool>, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(paths
            .iter()
            .map(|p| std::path::Path::new(p).is_file())
            .collect())
    }))
    .await
}

// ── Auditor IPC Commands ───────────────────────────────────────────────────────

/// `paths` plus every fixed drive when `all_drives`, in one key pass (#844).
/// `queued` is the project's queue: copies there, or in the install's game
/// folder, are kept first.
#[tauri::command]
async fn run_demo_audit(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AuditManager>,
    settings: tauri::State<'_, SettingsManager>,
    paths: Vec<String>,
    all_drives: bool,
    queued: Vec<String>,
    look_in_zips: Option<bool>,
) -> Result<AuditOutcome, String> {
    let hl_path = settings
        .inner
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .hl_path
        .clone();
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
async fn list_fixed_drives() -> Result<Vec<String>, String> {
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(
        audit_manager::fixed_drive_roots,
    ))
    .await
}

/// To the Recycle Bin, or for good when `permanent` (#844), each copy only
/// once it matches its group's kept copy byte for byte.
#[tauri::command]
async fn delete_audit_files(
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
fn cancel_audit(state: tauri::State<'_, AuditManager>) -> Result<(), String> {
    state.cancel_token.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command(async)]
fn reveal_in_explorer(path: String) -> Result<(), String> {
    audit_manager::reveal_in_explorer_impl(path)
}

/// Opens Windows' Recycle Bin (the Demo Auditor's deletes go there).
#[tauri::command(async)]
fn open_recycle_bin() -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg("shell:RecycleBinFolder")
            .spawn()
            .map_err(crate::messages::failed_to_open_explorer)?;
    }
    Ok(())
}

// ── Tauri commands ─────────────────────────────────────────────────────────────

/// Triggers a batch capture run from the Vite frontend.
///
/// Async so the Tauri dispatcher never blocks the main thread.
/// The heavy lifting is delegated inside `start_capture_batch_impl`
/// via `tokio::task::spawn_blocking`.
#[tauri::command]
async fn start_capture_batch(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, CaptureManager>,
    payload: CapturePayload,
) -> Result<(), String> {
    capture_manager::start_capture_batch_impl(app_handle, &state, payload).await
}

/// Cancel a running capture batch.
#[tauri::command]
async fn cancel_capture_batch(state: tauri::State<'_, CaptureManager>) -> Result<(), String> {
    native::log_markdown(&format!(
        "[capture] Cancel requested (is_running={})",
        state.is_running()
    ));
    state
        .cancel_token
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Writes one line to today's activity log from the frontend. Exists for
/// events that have no other Tauri command to piggyback logging on — the
/// Master Demo Queue's Clear Untracked/Selected/All and row-delete actions
/// are pure frontend array mutations with nothing else calling into Rust —
/// and every toast (#873). Off the window's thread: each line is flushed to
/// disk under the log's lock, which a capture's own logging can hold while
/// the disk is busy with frames. `logFrontendEvent` sends one line at a time,
/// so a burst (a rescan's carry report) still lands in order.
#[tauri::command(async)]
fn log_frontend_event(message: String) {
    native::log_markdown(&message);
}

/// Absolute path to today's activity log, for the top nav's "View Logs"
/// button — most users won't know to go looking under AppData on their own.
#[tauri::command]
fn get_activity_log_path() -> String {
    native::activity_log_path().to_string_lossy().to_string()
}

#[tauri::command]
async fn scan_directory(
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
async fn estimate_rescan(paths: Vec<String>) -> Result<analysis::cache::RescanEstimate, String> {
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
async fn system_memory_bytes() -> Result<u64, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(|| {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        Ok(sys.total_memory())
    }))
    .await
}

#[tauri::command]
fn cancel_scan(scan_state: tauri::State<'_, ScanManager>) -> Result<(), String> {
    scan_state.cancel_token.store(true, Ordering::SeqCst);
    Ok(())
}

/// Free space across the capture folders, each drive counted once: two
/// folders on one drive share its space. On the blocking pool (#853): a
/// cache miss re-lists every volume, which can wait on a sleeping or network
/// drive, and start-up asks for this alongside the other disk queries.
#[tauri::command]
async fn calculate_export_pool_space(paths: Vec<String>) -> Result<u64, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        export_pool_space(paths)
    }))
    .await
}

fn export_pool_space(paths: Vec<String>) -> Result<u64, String> {
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
    Ok(native::sys::disk::pool_free_bytes(&paths))
}

#[derive(serde::Serialize)]
struct CaptureOutputDiagnostic {
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
async fn diagnose_capture_output_paths(
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
            let usable = native::sys::disk::get_available_bytes(&p) != u64::MAX;
            CaptureOutputDiagnostic {
                path: path_str,
                status,
                usable,
            }
        })
        .collect()
}

#[tauri::command]
async fn validate_paths(hlae_path: String, hl_path: String) -> Result<bool, String> {
    let hlae_p = std::path::Path::new(&hlae_path);
    let hl_p = std::path::Path::new(&hl_path);

    if !hlae_p.exists() || !hlae_p.is_file() {
        return Err(messages::HLAE_EXECUTABLE_NOT_FOUND.into());
    }
    if !hl_p.exists() || !hl_p.is_file() {
        return Err(messages::HL_EXECUTABLE_NOT_FOUND.into());
    }
    Ok(true)
}

/// Whether HLAE can reach an FFmpeg of its own, which is a different question
/// from whether Render Studio can.
///
/// `mirv_movie_ffmpeg` makes HLAE spawn FFmpeg itself and it does not consult
/// the app's resolution chain, so this has to be reported before a batch rather
/// than discovered after one — the failure is a capture that runs to completion
/// and produces no video. See `native::shared::hlae_ffmpeg`.
///
/// On the blocking pool (#853): it searches PATH and runs `ffmpeg -version`,
/// polling for up to 5 s, and start-up calls it twice.
#[tauri::command]
async fn check_hlae_ffmpeg(
    hlae_path: String,
    ffmpeg_path: Option<String>,
) -> Result<serde_json::Value, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        hlae_ffmpeg_report(hlae_path, ffmpeg_path)
    }))
    .await
}

fn hlae_ffmpeg_report(
    hlae_path: String,
    ffmpeg_path: Option<String>,
) -> Result<serde_json::Value, String> {
    use native::shared::hlae_ffmpeg as hf;

    let state = hf::detect(std::path::Path::new(&hlae_path));

    // "HLAE is pointed somewhere" and "HLAE is pointed at the same FFmpeg
    // Render Studio uses" are different questions, and only the second keeps
    // both halves of the pipeline encoding with the same build — which was the
    // stated reason for writing an ini instead of copying the binary. Nothing
    // was checking it stayed true, so changing the app's FFmpeg silently left
    // HLAE on the old one.
    let app_ffmpeg = hf::resolve_absolute(ffmpeg_path.as_deref().unwrap_or("ffmpeg"));
    let agrees_with_app = match (&state, &app_ffmpeg) {
        (hf::HlaeFfmpeg::Linked { target, .. }, Some(app)) => Some(hf::same_file(target, app)),
        // A bundled binary is HLAE's own and is meant to differ; with nothing
        // linked there is nothing to disagree with.
        _ => None,
    };

    // "It exists" was the only test the picker applied, and ffplay.exe and
    // ffprobe.exe sit in the same folder as ffmpeg.exe. Checking here as well as
    // at link time means the row says so before the button is pressed, instead
    // of reporting a disagreement between two paths one of which cannot record.
    //
    // Chaining this off `app_ffmpeg` was a hole: a path that does not resolve
    // produces `None`, so the check never ran and a typo'd override said
    // nothing at all. Failing to resolve is itself the problem worth reporting.
    let configured = ffmpeg_path.as_deref().unwrap_or("").trim().to_string();
    // The frontend sends the bare default `ffmpeg` when no override is set, so
    // that is "no override" too, not a file called ffmpeg that went missing.
    let no_override = configured.is_empty() || configured.eq_ignore_ascii_case("ffmpeg");
    let app_ffmpeg_problem = match &app_ffmpeg {
        Some(p) => hf::verify_is_ffmpeg(p).err(),
        None if no_override => {
            Some("no ffmpeg.exe was found on PATH, and no override is set".to_string())
        }
        None => Some(format!("there is no file at \"{}\"", configured)),
    };

    // Whether the HLAE Executable above is a working HLAE install, answered by
    // the file the pipeline actually consumes rather than by what the exe calls
    // itself: `build_hlae_process` passes this DLL as `-hookDllPath`, so its
    // absence means a capture cannot work whatever the exe is named. Advisory —
    // reported, never enforced.
    let missing_hook_dll = hf::missing_hook_dll(std::path::Path::new(&hlae_path))
        .map(|p| p.to_string_lossy().into_owned());

    Ok(serde_json::json!({
        "state": state,
        "usable": state.is_usable(),
        "can_link": state.can_link(),
        "app_ffmpeg": app_ffmpeg.map(|p| p.to_string_lossy().into_owned()),
        "agrees_with_app": agrees_with_app,
        "app_ffmpeg_problem": app_ffmpeg_problem,
        "missing_hook_dll": missing_hook_dll,
    }))
}

/// Whether each configured executable path actually points at a file.
///
/// The Path Routing fields accepted anything: `validate_paths` only ran at
/// capture launch, so a typo sat there looking fine until a batch failed
/// minutes later. Returns a state per path rather than a message, so the
/// wording stays in `strings.js` with every other user-facing string.
#[tauri::command]
async fn diagnose_executable_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    Ok(paths
        .iter()
        .map(|raw| {
            let path = raw.trim();
            if path.is_empty() {
                // Not a complaint: these fields are legitimately blank before
                // they are filled in, and the FFmpeg override is optional.
                return "empty";
            }
            let p = std::path::Path::new(path);
            if p.is_file() {
                "ok"
            } else if p.is_dir() {
                // The classic mistake these fields invite — the folder rather
                // than the executable inside it.
                "not_a_file"
            } else {
                "not_found"
            }
        })
        .map(str::to_string)
        .collect())
}

/// Points HLAE at an FFmpeg by writing `ffmpeg.ini`, on request only.
///
/// Never overwrites an existing ini: HLAE installs are shared with Source work
/// and other projects, so silently repointing one would break somebody else's
/// setup to fix ours. `link` refuses in that case and the error says so.
/// `elevated` retries the same write through a UAC prompt. The HLAE installer
/// puts the target under `Program Files`, so an unelevated write fails for most
/// people — the frontend asks first, then calls back with this set.
#[tauri::command]
async fn link_hlae_ffmpeg(
    hlae_path: String,
    ffmpeg_path: String,
    elevated: Option<bool>,
) -> Result<serde_json::Value, String> {
    use native::shared::hlae_ffmpeg::{self, LinkError};

    let ffmpeg = hlae_ffmpeg::resolve_absolute(&ffmpeg_path)
        .ok_or_else(|| messages::ffmpeg_could_not_be_resolved(&ffmpeg_path))?;

    let hlae = std::path::Path::new(&hlae_path);
    let result = if elevated.unwrap_or(false) {
        hlae_ffmpeg::link_elevated(hlae, &ffmpeg)
    } else {
        hlae_ffmpeg::link(hlae, &ffmpeg)
    };

    match result {
        Ok(ini) => {
            native::log_markdown(&format!(
                "[hlae-ffmpeg] wrote {} pointing at {} — HLAE can now spawn FFmpeg for \
                 `mirv_movie_ffmpeg`.",
                ini.display(),
                ffmpeg.display()
            ));
            Ok(serde_json::json!({ "ini": ini.to_string_lossy() }))
        }
        // Reported as data, not an error string: the frontend has to tell this
        // apart from a real failure so it can offer the prompt rather than show
        // somebody "Access is denied. (os error 5)" and leave them there.
        Err(LinkError::NeedsElevation { ini }) => Ok(serde_json::json!({
            "needs_elevation": true,
            "ini": ini.to_string_lossy(),
        })),
        Err(e) => Err(e.to_string()),
    }
}

// ── Full-fidelity analysis payload for the standalone Demo Analyzer tab ─────────
// Passes the typed `analysis::DemoInfo`/`AnalyzerState` straight through so the frontend can
// reconstruct every report sub-view (Summary/Scoreboard/Player Details/Team
// Details/Timeline/Rounds/Chat) with full fidelity, matching the data the
// egui report views on `dev` were built from.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AnalyzerReportPayload {
    pub file_name: String,
    pub file_path: String,
    pub file_dir: String,
    pub file_size_mb: f64,
    pub file_created_unix_secs: f64,
    pub demo_info: analysis::DemoInfo,
    pub state: analysis::AnalyzerState,
}

/// Which request `index_demo_players` is serving, per lane; a newer one stops
/// an older one between demos (the Demo Analyzer moved to another folder).
/// One lane each, so the Master Queue's and Configuration > Players' lookups
/// never cancel the Analyzer's or each other's.
static PLAYER_INDEX_ANALYZER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PLAYER_INDEX_QUEUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PLAYER_INDEX_PLAYERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Who is in each demo (#437, #174), one `demo_players` event per demo as it
/// is read: from the small player index when it is current (milliseconds), or
/// from the analyzer cache or a full parse (about a second) when it is not.
/// Resolves with how many demos were read.
#[tauri::command]
async fn index_demo_players(
    app_handle: tauri::AppHandle,
    paths: Vec<String>,
    request_id: u64,
    lane: String,
) -> Result<usize, String> {
    let current: &'static std::sync::atomic::AtomicU64 = match lane.as_str() {
        "queue" => &PLAYER_INDEX_QUEUE,
        "players" => &PLAYER_INDEX_PLAYERS,
        _ => &PLAYER_INDEX_ANALYZER,
    };
    current.store(request_id, Ordering::SeqCst);
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use tauri::Emitter;
        let mut done = 0;
        for path in paths {
            if current.load(Ordering::SeqCst) != request_id {
                break;
            }
            let result = native::player_index::demo_players(std::path::Path::new(&path));
            let payload = match result {
                Ok((demo, _)) => serde_json::json!({
                    "lane": lane,
                    "requestId": request_id,
                    "path": path,
                    "demoType": demo.demo_type,
                    "players": demo.players,
                }),
                Err(e) => serde_json::json!({
                    "lane": lane,
                    "requestId": request_id,
                    "path": path,
                    "error": e,
                }),
            };
            let _ = app_handle.emit("demo_players", payload);
            done += 1;
        }
        Ok(done)
    }))
    .await
}

/// Which of `paths` already have their players (answered here, all at once),
/// which still need a read, and which sit on a cloud drive with no cache
/// entry (#992). Opens no demo, so it never downloads one.
#[tauri::command]
async fn lookup_demo_players(
    paths: Vec<String>,
) -> Result<native::player_index::PlayerLookup, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(native::player_index::lookup_players(&paths))
    }))
    .await
}

/// One `analyzer_progress` event. The path lets the page ignore an earlier
/// click's analysis still running beside this one; `cached` says the demo
/// is read from the analyzer cache, with no frames to count.
fn analyzer_progress_payload(
    path: &str,
    processed: usize,
    total: usize,
    cached: bool,
) -> serde_json::Value {
    serde_json::json!({ "processed": processed, "total": total, "path": path, "cached": cached })
}

#[tauri::command]
async fn analyze_demo_full(
    app_handle: tauri::AppHandle,
    demo_path: String,
) -> Result<AnalyzerReportPayload, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use tauri::Emitter;

        let path = std::path::PathBuf::from(&demo_path);

        if !path.exists() || !path.is_file() {
            return Err(messages::demo_file_not_found(&demo_path));
        }

        // A demo the analyzer cache has is read, not parsed: the page says so
        // instead of a % that never comes (#958).
        if analysis::cache::is_fresh(&native::analyzer_cache_root(), &path) {
            let _ = app_handle.emit(
                "analyzer_progress",
                analyzer_progress_payload(&demo_path, 0, 0, true),
            );
        }

        // Throttled to ~30fps per CLAUDE.md's telemetry-throttling guardrail —
        // `try_from_bytes_with_progress` calls back every ~500 frames, which
        // can be 1000+ times for a large demo.
        let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let progress_cb = |processed: usize, total: usize| {
            let now = std::time::Instant::now();
            if now.duration_since(last_emit) >= std::time::Duration::from_millis(33)
                || processed == total
            {
                last_emit = now;
                let _ = app_handle.emit(
                    "analyzer_progress",
                    analyzer_progress_payload(&demo_path, processed, total, false),
                );
            }
        };

        match native::run_analyzer_cached(&path, progress_cb) {
            Ok((file_info, analysis, _from_cache)) => {
                // Independent metadata lookup (not `file_info.created_at`) to avoid
                // depending on `web_time::SystemTime`'s exact type identity here.
                let created_unix_secs = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);

                let file_dir = std::path::Path::new(&file_info.path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                Ok(AnalyzerReportPayload {
                    file_name: file_info.name,
                    file_path: file_info.path,
                    file_dir,
                    file_size_mb: file_info.size_bytes as f64 / 1_048_576.0,
                    file_created_unix_secs: created_unix_secs,
                    demo_info: analysis.demo_info,
                    state: analysis.state,
                })
            }
            Err(e) => Err(messages::analyzer_error(e)),
        }
    }))
    .await
}

/// Every `Weapon` variant's resolved display name, keyed by its raw JSON tag
/// (e.g. `"ScopedK98"` -> "Scoped Kar98k") — the same names
/// `native::patch::scanner` bakes into a kill streak's timeline text, so the
/// frontend's weapon tables (analyzer_pane.js) can show identical text
/// instead of independently re-deriving a name from the raw enum tag.
///
/// On the blocking pool (#853): the first call at every boot loads the
/// localization files from disk.
#[tauri::command]
async fn get_weapon_display_names() -> Result<std::collections::HashMap<String, String>, String> {
    messages::spawn_blocking_result(tokio::task::spawn_blocking(
        analysis::all_weapon_display_names,
    ))
    .await
}

/// The overview image for `map_name` and how it lies over the world, for the
/// Demo Analyzer's kill map (#448). `None` when no `overviews/` folder near
/// the game or the demo has one the app can show -- not an error, since
/// custom maps often ship without.
#[tauri::command]
async fn load_map_overview(
    game_path: Option<String>,
    demo_path: String,
    map_name: String,
) -> Result<Option<native::map_overview::MapOverview>, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let game = game_path
            .filter(|p| !p.trim().is_empty())
            .map(std::path::PathBuf::from);
        let dirs = native::map_overview::overview_search_dirs(
            game.as_deref(),
            std::path::Path::new(&demo_path),
        );
        Ok(native::map_overview::find_map_overview(&dirs, &map_name))
    }))
    .await
}

// ── App entry point ────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // A project or demo Windows asked this copy to open (#528). When this
    // exe is already running, that copy opens it instead and this one exits
    // before anything else starts.
    let launch_files = open_files::openable_paths(
        std::env::args_os(),
        &std::env::current_dir().unwrap_or_default(),
    );
    #[cfg(windows)]
    let open_pipe = open_files::own_pipe_name();
    #[cfg(windows)]
    if let Some(pipe) = &open_pipe
        && !launch_files.is_empty()
        && open_files::forward_to_running(pipe, &launch_files)
    {
        return;
    }

    // Before the builder, not in `.setup()`: `SettingsManager::new()` below is
    // a `.manage(..)` argument, so it is constructed -- and reads settings.json
    // -- before any setup hook runs. Migrating after that would read the new,
    // empty location and hand the user a default config.
    let migration = native::shared::paths::migrate_legacy_appdata_dir();
    if migration.did_something() {
        native::log_markdown(&format!(
            "📁 **Settings** — moved {} item(s) from %APPDATA%\\{} to %APPDATA%\\{}{}{} (#257)",
            migration.moved.len(),
            native::shared::paths::LEGACY_APPDATA_DIR_NAME,
            native::shared::paths::APPDATA_DIR_NAME,
            if migration.skipped.is_empty() {
                String::new()
            } else {
                format!(
                    "; left {} already present at the destination",
                    migration.skipped.len()
                )
            },
            if migration.failed.is_empty() {
                String::new()
            } else {
                format!("; FAILED to move {:?}", migration.failed)
            },
        ));
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(CaptureManager::new())
        .manage(RenderManager::new())
        .manage(take_preview_manager::TakePreviewManager::default())
        .manage(take_checks_manager::TakeChecksManager::default())
        .manage(ScanManager::default())
        .manage(SettingsManager::new())
        .manage(AuditManager::default())
        .manage(demo_split_manager::DemoSplitManager::default())
        .manage(movie_copy_manager::MovieCopyManager::default())
        .manage(combine_manager::CombineManager::default())
        .manage(obs_test_manager::ObsTestManager::default())
        .manage(demo_rename_manager::DemoRenameManager::default())
        .manage(match_group_cmd::MatchGroupManager::default())
        .manage(hd_manager::HdManager::default())
        .manage(blender_manager::BlenderManager::default())
        .manage(updater_manager::UpdaterState::default())
        .manage(open_files::PendingOpenFiles::new(&launch_files))
        .setup(move |app| {
            // Dev/debug builds find the repo-root `localizations/` folder via
            // analysis::translate_key's own walk-up-from-exe search, since the
            // exe runs from inside the source tree. A packaged install runs from
            // e.g. Program Files, nowhere near that folder — its `resources`
            // directory (populated by tauri.conf.json's `bundle.resources`) is
            // the only place weapon-name strings can come from there.
            use tauri::Manager;
            if let Ok(resource_dir) = app.path().resource_dir() {
                analysis::add_localization_search_path(resource_dir.join("localizations"));
            }
            // The game's Highlights tab asks Studio to analyse a demo too big
            // for the game's own memory (#565).
            #[cfg(windows)]
            native::sys::analysis_server::start();
            // Files a later double-click hands this copy (#528): queued, then
            // the page is told and the window comes to the front.
            #[cfg(windows)]
            if let Some(pipe) = open_pipe.clone() {
                use tauri::Emitter;
                let handle = app.handle().clone();
                open_files::serve(pipe, move |files| {
                    handle.state::<open_files::PendingOpenFiles>().push(&files);
                    let _ = handle.emit(open_files::OPEN_FILES_EVENT, ());
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            demo_cache_cmd::cache_demos,
            drop_paths::sort_dropped_paths,
            demo_cache_cmd::cancel_demo_cache,
            log_frontend_event,
            get_activity_log_path,
            validate_paths,
            check_hlae_ffmpeg,
            diagnose_executable_paths,
            settings_manager::list_game_folders,
            settings_manager::game_folder_language_file,
            movie_copy_manager::movie_copy_plan,
            movie_copy_manager::start_movie_copy,
            movie_copy_manager::cancel_movie_copy,
            players_tab::list_logo_pngs,
            players_tab::list_crosshair_sprites,
            players_tab::crosshair_sprite_image,
            players_tab::add_crosshair_file,
            players_tab::set_player_overrides,
            link_hlae_ffmpeg,
            analyze_demo_full,
            get_weapon_display_names,
            load_map_overview,
            start_capture_batch,
            launch_demo_preview,
            obs_manager::obs_test_connection,
            obs_manager::obs_check_orphan,
            obs_manager::obs_recover_orphan,
            obs_test_manager::obs_test_clip,
            obs_test_manager::obs_test_watch_start,
            obs_test_manager::obs_test_analyze,
            obs_test_manager::obs_test_cancel,
            obs_test_manager::obs_test_delete_take,
            launch_obs,
            launch_standalone_game,
            read_cfg_commands,
            check_engine_processes,
            batch_catch_up::catch_up_unwatched_batches,
            batch_close::wait_for_batch_start,
            kill_engine_processes,
            game_process_manager::check_running_game,
            game_process_manager::running_game_for_batch,
            game_process_manager::close_running_game,
            steam_state,
            start_steam,
            send_preview_to_running_game,
            review_manager::start_highlight_review,
            review_manager::stop_highlight_review,
            review_library::review_library_get,
            review_library::review_library_update,
            scan_orphaned_previews,
            delete_orphaned_previews,
            cancel_capture_batch,
            scan_directory,
            estimate_rescan,
            cancel_scan,
            calculate_export_pool_space,
            diagnose_capture_output_paths,
            queue_render_batch,
            start_queued_render,
            cancel_render_batch,
            cancel_render_job,
            reset_render_job,
            reset_all_render_jobs,
            remove_render_job,
            remove_non_rendering_render_jobs,
            cancel_render_jobs,
            reset_render_jobs,
            remove_render_jobs,
            clear_render_queue,
            take_delete_manager::measure_take_folders,
            take_delete_manager::delete_take_folders,
            take_preview_manager::take_preview_thumb,
            take_preview_manager::take_preview_sheet,
            take_preview_manager::cancel_take_previews,
            take_checks_manager::take_checks,
            take_checks_manager::cancel_take_checks,
            set_render_job_codec,
            get_export_pool_free_gb,
            get_render_required_estimate_gb,
            check_render_autosave,
            discard_render_autosave,
            recover_render_batch,
            get_render_jobs,
            get_settings,
            save_settings,
            builtin_command_profiles,
            save_project_session,
            load_project_session,
            files_exist,
            index_demo_players,
            lookup_demo_players,
            default_projects_dir,
            locate_missing_demos,
            changed_demos,
            system_memory_bytes,
            run_demo_audit,
            demo_split_manager::find_multi_map_demos_cmd,
            demo_split_manager::cancel_multi_map_scan,
            demo_split_manager::demo_map_segments,
            demo_split_manager::split_demo_maps,
            demo_split_manager::cancel_demo_split,
            demo_split_manager::split_demo_auto,
            delete_audit_files,
            list_fixed_drives,
            cancel_audit,
            crash_maps_manager::crash_map_warnings,
            crash_maps_manager::forget_crash_map,
            demo_links_manager::list_demo_links,
            demo_links_manager::add_demo_link,
            demo_links_manager::remove_demo_link,
            packet_limit_manager::engine_packet_entity_limit,
            menu_scale_manager::menu_scale_status,
            menu_scale_manager::set_menu_scale,
            installs_manager::find_installs,
            combine_manager::combine_plan,
            combine_manager::combine_clips,
            combine_manager::combine_cancel,
            demo_rename_manager::demo_rename_list,
            demo_rename_manager::demo_rename_cancel,
            demo_rename_manager::demo_rename_apply,
            demo_rename_manager::demo_rename_undo,
            demo_rename_manager::demo_rename_undoable,
            match_group_cmd::queue_match_groups,
            match_group_cmd::queue_match_groups_cancel,
            reveal_in_explorer,
            open_recycle_bin,
            dir_browser::browse_directory,
            dir_browser::read_demo_heads,
            dir_browser::default_browse_dir,
            pick_paths::pick_path_states,
            pick_paths::pick_user_folders,
            pick_paths::file_dialog_shown,
            dir_browser::count_demo_files_in_folder,
            dir_browser::scan_demo_folders,
            map_manager::check_demo_maps,
            map_manager::download_map,
            map_manager::scan_game_configs,
            map_manager::roll_floors,
            hd_manager::hd_status,
            hd_manager::hd_setup_tools,
            hd_manager::hd_cancel,
            hd_manager::hd_build,
            hd_manager::hd_set_python,
            hd_manager::hd_set_upscaler,
            overview_manager::overview_installs,
            overview_manager::overview_maps,
            overview_manager::overview_scene,
            overview_manager::overview_load_edits,
            overview_manager::overview_save_edits,
            overview_manager::overview_reset_edits,
            overview_manager::overview_export,
            overview_manager::overview_export_hd,
            overview_manager::overview_flag_icons,
            overview_manager::overview_screen_height,
            hd_manager::hd_misses,
            hd_manager::hd_save_style,
            hd_manager::hd_remove_style,
            hd_manager::hd_save_map_list,
            hd_manager::hd_world_coverage,
            hd_manager::hd_preview,
            blender_manager::blender_status,
            blender_manager::blender_set_exe,
            blender_manager::blender_run,
            blender_manager::blender_cancel,
            updater_manager::check_for_update,
            updater_manager::download_and_install_update,
            updater_manager::restart_app,
            updater_manager::release_history,
            updater_manager::is_debug_build,
            updater_manager::local_git_branch,
            updater_manager::hook_dll_override_allowed,
            hook_dll_list::list_hook_dlls,
            open_files::take_open_files,
            dem_open_with::dem_open_with_status,
            dem_open_with::set_dem_open_with,
            project_convert::project_convert_offer,
            project_convert::convert_project_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod analyzer_progress_tests {
    use super::analyzer_progress_payload;

    /// The fields analyzer_pane.js's `analyzer_progress` listener reads.
    #[test]
    fn a_cached_demo_is_said_to_be_cached() {
        let cached = analyzer_progress_payload("D:/demos/a.dem", 0, 0, true);
        assert_eq!(cached["path"], "D:/demos/a.dem");
        assert_eq!(cached["cached"], true);
        let parsing = analyzer_progress_payload("D:/demos/a.dem", 30, 100, false);
        assert_eq!(parsing["processed"], 30);
        assert_eq!(parsing["total"], 100);
        assert_eq!(parsing["cached"], false);
    }
}
