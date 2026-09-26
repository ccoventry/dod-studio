//! The Blender page's backend (#403): finding Blender, and running the
//! `blender/` scripts on a recorded `.agr` one step at a time. The work
//! itself is in `native::blender`; this is the Tauri surface.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use native::blender::{self, BlenderStatus, JobRequest, Outcome};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// One Blender job at a time, and a way to stop it.
#[derive(Default)]
pub struct BlenderManager {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

/// The scripts: the copy bundled with the app, else (in a dev build) the
/// repo's.
fn scripts_dir(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("blender-scripts"));
    blender::scripts_dir(bundled.as_deref())
}

/// The `hl.exe` path from the Configuration page, or the "set it first"
/// error.
fn game_exe(game_path: &str) -> Result<PathBuf, String> {
    let game_path = game_path.trim();
    if game_path.is_empty() {
        return Err(crate::messages::BLENDER_NEEDS_GAME_PATH.to_string());
    }
    Ok(PathBuf::from(game_path))
}

/// What the page shows before anything runs.
#[derive(Debug, Clone, Serialize)]
pub struct BlenderPageStatus {
    pub blender: BlenderStatus,
    /// Maps in the game's `dod\maps` and `dod_downloads\maps`.
    pub maps: Vec<String>,
    /// HD styles built for models.
    pub styles: Vec<String>,
}

/// Which Blender a job would use and what it lacks, plus the maps and HD
/// styles to pick from. `game_path` is the `hl.exe` the Configuration page
/// holds.
#[tauri::command]
pub async fn blender_status(
    app: AppHandle,
    game_path: String,
) -> Result<BlenderPageStatus, String> {
    let game = game_exe(&game_path)?;
    let scripts = scripts_dir(&app);
    // Asks Blender for its version: a process, so not on the async runtime.
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || BlenderPageStatus {
        blender: blender::resolve(&blender::tools_dir(), scripts.as_deref()),
        maps: blender::maps(&game),
        styles: blender::hd_styles(&game),
    }))
    .await
}

/// Uses `path` (a `blender.exe` the user picked) from now on, or goes back to
/// finding one with `None`.
#[tauri::command]
pub async fn blender_set_exe(path: Option<String>) -> Result<(), String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let tools = blender::tools_dir();
        match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(path) => {
                let exe = Path::new(path);
                let named_blender = exe
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("blender.exe"));
                if !exe.is_file() || !named_blender {
                    return Err(crate::messages::blender_not_blender(path));
                }
                blender::set_chosen(&tools, Some(exe))
            }
            None => blender::set_chosen(&tools, None),
        }
    }))
    .await
}

/// Runs one step of the pipeline on `request.agr`. Emits `blender_progress`
/// at most ~30 times a second, and always when a rendered-frame count changes.
#[tauri::command]
pub async fn blender_run(
    app: AppHandle,
    state: State<'_, BlenderManager>,
    game_path: String,
    request: JobRequest,
) -> Result<Outcome, String> {
    let game = game_exe(&game_path)?;
    let scripts =
        scripts_dir(&app).ok_or_else(|| crate::messages::BLENDER_NO_SCRIPTS.to_string())?;
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(crate::messages::BLENDER_ALREADY_RUNNING.to_string());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let (running, cancel) = (Arc::clone(&state.running), Arc::clone(&state.cancel));

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let status = blender::resolve(&blender::tools_dir(), Some(&scripts));
        let exe = status
            .exe
            .map(PathBuf::from)
            .ok_or_else(|| crate::messages::BLENDER_NOT_FOUND.to_string())?;
        // Throttled to ~30fps per CLAUDE.md's telemetry-throttling guardrail:
        // the import step prints thousands of lines.
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let mut last_done = None;
        blender::run(&request, &exe, &scripts, &game, &cancel, &mut |progress| {
            let now = Instant::now();
            if progress.done != last_done
                || now.duration_since(last_emit) >= Duration::from_millis(33)
            {
                last_emit = now;
                last_done = progress.done;
                let _ = app.emit("blender_progress", progress);
            }
        })
    }))
    .await;

    running.store(false, Ordering::SeqCst);
    result
}

/// Stops the running step by ending Blender. A render keeps the frames it
/// finished, and running it again carries on from there.
#[tauri::command]
pub fn blender_cancel(state: State<'_, BlenderManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}
