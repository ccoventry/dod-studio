//! The HD Textures page's backend (#372): what is built, finding (or
//! fetching) the upscaler and a Python, running the build, the user's own
//! styles, and the misses the game logged.
//! The work itself is in `native::hd`; this is the Tauri surface.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use native::hd::build::{BuildOutcome, BuildRequest};
use native::hd::my_styles::{self, StyleDef};
use native::hd::{self, HdStatus, misses, python, setup::SetupOutcome, upscaler};
use tauri::{AppHandle, Emitter, Manager, State};

/// One download or build at a time, and a way to stop it. They share the
/// flag: a build must not start while its Python is still being fetched.
#[derive(Default)]
pub struct HdManager {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

/// The `hl.exe` path from the Configuration page, or the "set it first"
/// error.
fn game_exe(game_path: &str) -> Result<PathBuf, String> {
    let game_path = game_path.trim();
    if game_path.is_empty() {
        return Err(crate::messages::HD_NEEDS_GAME_PATH.to_string());
    }
    Ok(PathBuf::from(game_path))
}

/// The build scripts: the copy bundled with the app, else (in a dev build)
/// the repo's.
fn scripts_dir(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("hd-scripts"));
    hd::build::scripts_dir(bundled.as_deref())
}

/// What is built under `<game>\dod\dodstudio_hd`, which upscaler folder and
/// Python a build would use, and what each is missing. `game_path` is the
/// `hl.exe` the Configuration page holds.
#[tauri::command]
pub async fn hd_status(app: AppHandle, game_path: String) -> Result<HdStatus, String> {
    let root = hd::hd_root(&game_exe(&game_path)?)
        .ok_or_else(|| crate::messages::HD_NEEDS_GAME_PATH.to_string())?;
    let scripts = scripts_dir(&app);
    // A big HD folder is tens of thousands of files, and finding Python runs
    // a few processes: neither belongs on the async runtime's own threads.
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        let hd_tools = hd::setup::hd_tools_dir();
        let (source, realesrgan) = upscaler::resolve_or_app(&hd_tools, scripts.as_deref());
        let mut status = hd::scan(&root, &realesrgan);
        status.tools.source = source;
        status.tools.chosen =
            upscaler::chosen(&hd_tools).map(|dir| dir.to_string_lossy().to_string());
        status.python = Some(python::resolve(&hd_tools));
        status.my_styles = Some(my_styles::read(&root, scripts.as_deref()));
        status.scripts = scripts.map(|dir| dir.to_string_lossy().to_string());
        status
    }))
    .await
}

/// Marks a download or build as running, or says one already is.
fn start(state: &HdManager) -> Result<(Arc<AtomicBool>, Arc<AtomicBool>), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(crate::messages::HD_ALREADY_RUNNING.to_string());
    }
    state.cancel.store(false, Ordering::SeqCst);
    Ok((Arc::clone(&state.running), Arc::clone(&state.cancel)))
}

/// Downloads the upscaler and style models into the app's own folder when
/// no complete upscaler folder is found elsewhere, plus the app's own Python
/// when the PC has none the build can use, skipping what is already there.
/// Emits `hd_setup_progress` at most ~30 times a second.
#[tauri::command]
pub async fn hd_setup_tools(
    app: AppHandle,
    state: State<'_, HdManager>,
) -> Result<SetupOutcome, String> {
    let scripts = scripts_dir(&app);
    let (running, cancel) = start(&state)?;

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let hd_tools = hd::setup::hd_tools_dir();
        let need_upscaler = !upscaler::resolve(&hd_tools, scripts.as_deref())
            .is_some_and(|(_, dir)| upscaler::complete(&dir));
        let need_python = python::resolve(&hd_tools).using.is_none();
        // Throttled to ~30fps per CLAUDE.md's telemetry-throttling guardrail:
        // `run` reports every 64 KiB chunk, thousands of times a download.
        let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let mut last_item = String::new();
        hd::setup::run(
            &hd_tools,
            need_upscaler,
            need_python,
            &cancel,
            &mut |progress| {
                let now = std::time::Instant::now();
                // Always pass on a new file or the unpack step, so the line never
                // shows a finished file while the next one is under way.
                let changed = progress.item != last_item || progress.unpacking;
                if changed || now.duration_since(last_emit) >= std::time::Duration::from_millis(33)
                {
                    last_emit = now;
                    last_item.clone_from(&progress.item);
                    let _ = app.emit("hd_setup_progress", progress);
                }
            },
        )
    }))
    .await;

    running.store(false, Ordering::SeqCst);
    result
}

/// Stops a running download or build: a download at its next chunk (what was
/// complete stays), a build by ending its processes (what was built stays).
#[tauri::command]
pub fn hd_cancel(state: State<'_, HdManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Builds the chosen styles and asset types with `build_all.py`, skipping
/// files that already exist. Emits `hd_build_progress` at each step and log
/// line.
#[tauri::command]
pub async fn hd_build(
    app: AppHandle,
    state: State<'_, HdManager>,
    game_path: String,
    request: BuildRequest,
) -> Result<BuildOutcome, String> {
    let game = game_exe(&game_path)?;
    let scripts = scripts_dir(&app).ok_or_else(|| crate::messages::HD_NO_SCRIPTS.to_string())?;
    let (running, cancel) = start(&state)?;

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let hd_tools = hd::setup::hd_tools_dir();
        let using = python::resolve(&hd_tools)
            .using
            .ok_or_else(|| crate::messages::HD_NO_PYTHON.to_string())?;
        let (_, realesrgan) = upscaler::resolve_or_app(&hd_tools, Some(&scripts));
        hd::build::run(
            &request,
            &using,
            &hd_tools,
            &realesrgan,
            &scripts,
            &game,
            &cancel,
            &mut |progress| {
                let _ = app.emit("hd_build_progress", progress);
            },
        )
    }))
    .await;

    running.store(false, Ordering::SeqCst);
    result
}

/// Uses `path` (a `python.exe` the user picked) for builds from now on, or
/// goes back to finding one with `None`. A file that doesn't run as Python is
/// refused; one missing a package is kept, and the page says what's missing.
#[tauri::command]
pub async fn hd_set_python(path: Option<String>) -> Result<(), String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let hd_tools = hd::setup::hd_tools_dir();
        match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(path) => {
                if python::probe(Path::new(path), &[]).is_none() {
                    return Err(crate::messages::hd_not_python(path));
                }
                python::set_chosen(&hd_tools, Some(Path::new(path)))
            }
            None => python::set_chosen(&hd_tools, None),
        }
    }))
    .await
}

/// Uses `path` (a folder with `realesrgan-ncnn-vulkan.exe` in it) for builds
/// from now on, or goes back to finding one with `None`.
#[tauri::command]
pub async fn hd_set_upscaler(path: Option<String>) -> Result<(), String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let hd_tools = hd::setup::hd_tools_dir();
        let dir = path.as_deref().map(str::trim).filter(|p| !p.is_empty());
        upscaler::set_chosen(&hd_tools, dir.map(Path::new))
    }))
    .await
}

/// The newest list `dodstudio_debug_hd_misses` wrote to the hook log (none
/// when no log has one yet), and the command itself.
#[tauri::command]
pub async fn hd_misses() -> Result<misses::MissesView, String> {
    // A day's hook log can be tens of thousands of lines.
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(|| {
        misses::view(&native::activity_log_dir())
    }))
    .await
}

/// Adds `name` to the install's `my_styles.txt` as `def`, or changes it if
/// the file has it already.
#[tauri::command]
pub async fn hd_save_style(
    app: AppHandle,
    game_path: String,
    name: String,
    def: StyleDef,
) -> Result<(), String> {
    let root = hd::hd_root(&game_exe(&game_path)?)
        .ok_or_else(|| crate::messages::HD_NEEDS_GAME_PATH.to_string())?;
    let scripts = scripts_dir(&app);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        my_styles::save(&root, scripts.as_deref(), &name, &def)
    }))
    .await
}

/// Takes `name` out of the install's `my_styles.txt`. What it built stays.
#[tauri::command]
pub async fn hd_remove_style(
    app: AppHandle,
    game_path: String,
    name: String,
) -> Result<(), String> {
    let root = hd::hd_root(&game_exe(&game_path)?)
        .ok_or_else(|| crate::messages::HD_NEEDS_GAME_PATH.to_string())?;
    let scripts = scripts_dir(&app);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        my_styles::remove(&root, scripts.as_deref(), &name)
    }))
    .await
}
