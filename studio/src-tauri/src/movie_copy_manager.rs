//! Setup's "Make a movie copy" (#939): the IPC around
//! `native::shared::movie_copy`, which does the copy.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use native::shared::movie_copy::{self, CopyOutcome, CopySummary};
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub struct MovieCopyManager {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

/// Progress events at most this often (~30 fps), per the event-loop rule.
const PROGRESS_EVERY_MS: u32 = 33;

/// What the dialog shows before copying.
#[derive(serde::Serialize)]
pub struct MovieCopyPlan {
    #[serde(flatten)]
    pub summary: CopySummary,
    /// Free bytes on the drive holding hl.exe; `None` when unknown.
    pub free_bytes: Option<u64>,
    /// The bytes the drive keeps spare on top of the copy.
    pub headroom_bytes: u64,
    /// Every name already in the Half-Life folder, for the name check.
    pub existing: Vec<String>,
    pub default_name: &'static str,
}

fn game_root(game_path: &str) -> Result<PathBuf, String> {
    Path::new(game_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("{game_path} is not a path to hl.exe"))
}

/// The sizes and names the dialog needs, for the `dod` beside `game_path`.
#[tauri::command]
pub async fn movie_copy_plan(game_path: String) -> Result<MovieCopyPlan, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let root = game_root(&game_path)?;
        let source = root.join(movie_copy::SOURCE_FOLDER);
        let plan = movie_copy::plan_movie_copy(&source, movie_copy::DEFAULT_COPY_NAME)
            .map_err(|e| format!("Couldn't list {} to copy it: {e}", source.display()))?;
        Ok(MovieCopyPlan {
            summary: plan.summary(),
            free_bytes: native::sys::disk::get_available_bytes(&root),
            headroom_bytes: movie_copy::FREE_SPACE_HEADROOM_BYTES,
            existing: movie_copy::existing_names(&root),
            default_name: movie_copy::DEFAULT_COPY_NAME,
        })
    }))
    .await
}

/// Copies `dod` to `name` beside `game_path`, emitting `movie_copy_progress`
/// ([`movie_copy::CopyProgress`]) at most every [`PROGRESS_EVERY_MS`].
/// [`cancel_movie_copy`] stops it and the half-made folder goes.
#[tauri::command]
pub async fn start_movie_copy(
    app_handle: AppHandle,
    state: tauri::State<'_, MovieCopyManager>,
    game_path: String,
    name: String,
    include_demos: bool,
) -> Result<CopyOutcome, String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("A movie copy is already running".to_string());
    }
    let running = Arc::clone(&state.running);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);
    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let root = game_root(&game_path)?;
        let started = std::time::Instant::now();
        let last = AtomicU32::new(0);
        let mut progress = |p: &movie_copy::CopyProgress| {
            let now = started.elapsed().as_millis() as u32;
            let finished = p.done_files == p.total_files;
            if !finished && now.saturating_sub(last.load(Ordering::Relaxed)) < PROGRESS_EVERY_MS {
                return;
            }
            last.store(now, Ordering::Relaxed);
            let _ = app_handle.emit("movie_copy_progress", p);
        };
        movie_copy::copy_movie_folder(&root, &name, include_demos, &cancel, &mut progress)
    }))
    .await;
    running.store(false, Ordering::SeqCst);
    result
}

/// Stops the running movie copy at its next chunk.
#[tauri::command]
pub fn cancel_movie_copy(state: tauri::State<'_, MovieCopyManager>) -> Result<(), String> {
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}
