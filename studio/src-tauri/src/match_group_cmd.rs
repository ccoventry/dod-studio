//! The Master Queue's match grouping (#685): a Tauri command over
//! `native::match_groups`. Reads each queued demo's analysis, from the
//! analyzer cache when it has one and by parsing it when asked to, on a few
//! threads, with progress and Cancel.

use native::match_groups::{self, Grouping};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::{AppHandle, Emitter};

/// Progress events at most this often (CLAUDE.md's ~30fps).
const PROGRESS_INTERVAL_MS: u32 = 33;
/// Reads at once. Each holds a whole demo's analysis while it is read.
const MAX_WORKERS: usize = 4;

#[derive(Default)]
pub struct MatchGroupManager {
    cancel: Arc<AtomicBool>,
}

/// Which match and half each of `paths` is. A demo the analyzer cache
/// doesn't have is parsed when `parse_missing`, else left out (the
/// frontend shows it as unknown). Cancelled, the demos read so far are
/// grouped.
#[tauri::command]
pub async fn queue_match_groups(
    app_handle: AppHandle,
    state: tauri::State<'_, MatchGroupManager>,
    paths: Vec<String>,
    parse_missing: bool,
) -> Result<Grouping, String> {
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let total = paths.len();
        let started = std::time::Instant::now();
        let last_emit = AtomicU32::new(u32::MAX);
        let emit = |done: usize| {
            let now = started.elapsed().as_millis() as u32;
            let last = last_emit.load(Ordering::Relaxed);
            if done == total || last == u32::MAX || now.saturating_sub(last) >= PROGRESS_INTERVAL_MS
            {
                last_emit.store(now, Ordering::Relaxed);
                let _ = app_handle.emit(
                    "match_group_progress",
                    serde_json::json!({ "done": done, "total": total }),
                );
            }
        };
        emit(0);
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, MAX_WORKERS);
        Ok(match_groups::group_paths(
            &paths,
            parse_missing,
            workers,
            &cancel,
            &emit,
        ))
    }))
    .await
}

#[tauri::command]
pub fn queue_match_groups_cancel(state: tauri::State<'_, MatchGroupManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}
