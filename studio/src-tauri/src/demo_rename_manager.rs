//! The Demo Auditor's renamer (#469): Tauri commands over
//! `native::demo_rename`. Listing a folder reads each demo's analysis, from
//! the analyzer cache when it has one and by parsing it (about a second)
//! when it doesn't, on a few threads, with progress and Cancel.

use native::demo_rename::{self, DemoFacts, RenameOutcome, RenamePair, UndoableBatch};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
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
        // The demos the analyzer cache already has go first, then the parses
        // largest first (`analysis::cache::WorkPlan`). The bytes left to
        // parse give the page a steady time left.
        let plan = analysis::cache::WorkPlan::new(&native::analyzer_cache_root(), &demos);
        let cached = plan.cached();
        let bytes_to_parse = plan.bytes_to_parse();
        let analysis::cache::WorkPlan {
            fresh,
            sizes,
            order,
        } = plan;
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let parsed = AtomicUsize::new(0);
        let bytes_parsed = AtomicU64::new(0);
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
                        "bytes_to_parse": bytes_to_parse,
                        "bytes_parsed": bytes_parsed.load(Ordering::Relaxed),
                    }),
                );
            }
        };
        emit(0, true);
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, MAX_WORKERS)
            .min(total.max(1));
        // Each demo's analysis is kept, trimmed, until every one is read:
        // its match and half are decided over the whole list (#685).
        let mut facts: Vec<(usize, (DemoFacts, Option<analysis::Analysis>))> =
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..workers)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut mine = Vec::new();
                            loop {
                                if cancel.load(Ordering::Relaxed) {
                                    break;
                                }
                                let Some(&i) = order.get(next.fetch_add(1, Ordering::Relaxed))
                                else {
                                    break;
                                };
                                mine.push((i, demo_rename::facts_for_grouping(&demos[i])));
                                if !fresh[i] {
                                    parsed.fetch_add(1, Ordering::Relaxed);
                                    bytes_parsed.fetch_add(sizes[i], Ordering::Relaxed);
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
        let (mut facts, analyses): (Vec<DemoFacts>, Vec<Option<analysis::Analysis>>) =
            facts.into_iter().map(|(_, pair)| pair).unzip();
        demo_rename::apply_grouping(&mut facts, &analyses);
        Ok(facts)
    }))
    .await
}

#[tauri::command]
pub fn demo_rename_cancel(state: tauri::State<'_, DemoRenameManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Whether a rename count is worth an event: the first and the last always,
/// the rest at most every `PROGRESS_INTERVAL_MS`.
fn count_due(
    last: Option<std::time::Instant>,
    now: std::time::Instant,
    done: usize,
    total: usize,
) -> bool {
    done <= 1
        || done >= total
        || last
            .is_none_or(|t| now.duration_since(t).as_millis() >= u128::from(PROGRESS_INTERVAL_MS))
}

/// Sends `demo_rename_apply_progress` (`{ done, total }`) for Apply and Undo,
/// throttled by [`count_due`] (#958).
fn rename_counter(app: &AppHandle) -> impl FnMut(usize, usize) + '_ {
    let mut last = None;
    move |done, total| {
        let now = std::time::Instant::now();
        if count_due(last, now, done, total) {
            last = Some(now);
            let _ = app.emit(
                "demo_rename_apply_progress",
                serde_json::json!({ "done": done, "total": total }),
            );
        }
    }
}

/// Renames each pair in place and logs the batch for Undo.
#[tauri::command]
pub async fn demo_rename_apply(
    app_handle: AppHandle,
    renames: Vec<RenamePair>,
) -> Result<RenameOutcome, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(demo_rename::apply_reporting(
            &renames,
            &log_dir(),
            &mut rename_counter(&app_handle),
        ))
    }))
    .await
}

/// Puts the newest batch back.
#[tauri::command]
pub async fn demo_rename_undo(app_handle: AppHandle) -> Result<RenameOutcome, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(demo_rename::undo_last_reporting(
            &log_dir(),
            &mut rename_counter(&app_handle),
        ))
    }))
    .await
}

/// The batch Undo would put back, if any. Off the main thread (#853): it
/// reads and parses every rename log.
#[tauri::command]
pub async fn demo_rename_undoable() -> Result<Option<UndoableBatch>, String> {
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        demo_rename::undoable(&log_dir())
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn rename_counts_are_throttled_but_the_first_and_last_always_go() {
        let t = Instant::now();
        let soon = t + Duration::from_millis(5);
        let later = t + Duration::from_millis(40);
        assert!(count_due(None, t, 1, 40), "the first count");
        assert!(count_due(Some(t), soon, 40, 40), "the last count");
        assert!(
            !count_due(Some(t), soon, 12, 40),
            "too soon after the last event"
        );
        assert!(count_due(Some(t), later, 12, 40), "33 ms on");
        assert!(count_due(None, t, 12, 40), "no event yet");
    }
}
