//! The Demo Auditor's renamer (#469): Tauri commands over
//! `native::demo_rename`. Listing a folder reads each demo's analysis, from
//! the analyzer cache when it has one and by parsing it (about a second)
//! when it doesn't, on a few threads, with progress and Cancel.

use native::demo_rename::{self, DemoFacts, RenameOutcome, RenamePair, UndoableBatch};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use tauri::{AppHandle, Emitter};

/// Progress events at most this often (CLAUDE.md's ~30fps).
const PROGRESS_INTERVAL_MS: u32 = 33;
/// Parses at once. Each holds a whole demo's analysis.
const MAX_WORKERS: usize = 4;

#[derive(Default)]
pub struct DemoRenameManager {
    cancel: Arc<AtomicBool>,
}

fn log_dir() -> PathBuf {
    native::shared::paths::get_appdata_dir().join(demo_rename::LOG_FOLDER)
}

/// Every demo under `folder` with what its new name can be built from.
/// Cancelled, it returns the demos read so far.
#[tauri::command]
pub async fn demo_rename_list(
    app_handle: AppHandle,
    state: tauri::State<'_, DemoRenameManager>,
    folder: String,
) -> Result<Vec<DemoFacts>, String> {
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(&folder);
        if !root.is_dir() {
            return Err(crate::messages::rename_folder_not_found(&folder));
        }
        let demos = demo_rename::demos_in(&root);
        let total = demos.len();
        // The demos the analyzer cache already has go first: each takes
        // milliseconds, so the count jumps to "cached of total" at once and
        // only the parses (about a second each) are left to wait for.
        let cache_root = native::analyzer_cache_root();
        let fresh: Vec<bool> = demos
            .iter()
            .map(|d| analysis::cache::is_fresh(&cache_root, d))
            .collect();
        let cached = fresh.iter().filter(|f| **f).count();
        let mut order: Vec<usize> = (0..total).collect();
        order.sort_by_key(|&i| !fresh[i]);
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let parsed = AtomicUsize::new(0);
        let started = std::time::Instant::now();
        // Milliseconds since `started` of the last event, so the workers
        // share one throttle without a lock.
        let last_emit = Arc::new(AtomicU32::new(0));
        let emit = |done: usize, force: bool| {
            let now = started.elapsed().as_millis() as u32;
            let last = last_emit.load(Ordering::Relaxed);
            if force || now.saturating_sub(last) >= PROGRESS_INTERVAL_MS {
                last_emit.store(now, Ordering::Relaxed);
                let _ = app_handle.emit(
                    "demo_rename_progress",
                    serde_json::json!({
                        "done": done,
                        "total": total,
                        "cached": cached,
                        "parsed": parsed.load(Ordering::Relaxed),
                    }),
                );
            }
        };
        emit(0, true);
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, MAX_WORKERS)
            .min(total.max(1));
        let mut facts: Vec<(usize, DemoFacts)> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    scope.spawn(|| {
                        let mut mine = Vec::new();
                        loop {
                            if cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            let Some(&i) = order.get(next.fetch_add(1, Ordering::Relaxed)) else {
                                break;
                            };
                            mine.push((i, demo_rename::facts(&demos[i])));
                            if !fresh[i] {
                                parsed.fetch_add(1, Ordering::Relaxed);
                            }
                            let now_done = done.fetch_add(1, Ordering::Relaxed) + 1;
                            // A parse takes about a second, so each one may
                            // report; so does the last cached demo, which a
                            // throttle could otherwise hold back until the
                            // first parse ends.
                            emit(now_done, !fresh[i] || now_done == cached);
                        }
                        mine
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect()
        });
        emit(done.load(Ordering::Relaxed), true);
        facts.sort_by_key(|(i, _)| *i);
        Ok(facts.into_iter().map(|(_, f)| f).collect())
    }))
    .await
}

#[tauri::command]
pub fn demo_rename_cancel(state: tauri::State<'_, DemoRenameManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Renames each pair in place and logs the batch for Undo.
#[tauri::command]
pub async fn demo_rename_apply(renames: Vec<RenamePair>) -> Result<RenameOutcome, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(demo_rename::apply(&renames, &log_dir()))
    }))
    .await
}

/// Puts the newest batch back.
#[tauri::command]
pub async fn demo_rename_undo() -> Result<RenameOutcome, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(demo_rename::undo_last(&log_dir()))
    }))
    .await
}

/// The batch Undo would put back, if any.
#[tauri::command]
pub fn demo_rename_undoable() -> Option<UndoableBatch> {
    demo_rename::undoable(&log_dir())
}
