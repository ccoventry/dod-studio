//! The Demo Analyzer's **Cache all** button (#569): analyses every demo of
//! the current folder into the analyzer cache in the background
//! (`native::demo_cache`), reporting a `demo_cache_progress` event per demo.
//! One run at a time; **Stop** sets the run's cancel flag.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::Emitter;

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Starts caching `paths`. Returns at once; progress arrives as events.
#[tauri::command]
pub fn cache_demos(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), String> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err(crate::messages::DEMO_CACHE_ALREADY_RUNNING.to_string());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *CANCEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    native::log_markdown(&format!("Cache demos: {} demo(s)", paths.len()));
    std::thread::spawn(move || {
        native::demo_cache::cache_demos(paths, cancel, move |progress| {
            if progress.finished {
                native::log_markdown(&format!(
                    "Cache demos: {} of {} done ({} already cached, {} failed){}",
                    progress.done,
                    progress.total,
                    progress.already,
                    progress.failed,
                    if progress.cancelled { ", stopped" } else { "" }
                ));
                // Before the last event, so a click right after it starts anew.
                RUNNING.store(false, Ordering::Release);
            }
            let _ = app.emit("demo_cache_progress", progress);
        });
    });
    Ok(())
}

/// Stops the running cache job after the demos it is on.
#[tauri::command]
pub fn cancel_demo_cache() {
    if let Some(cancel) = CANCEL.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        cancel.store(true, Ordering::Release);
    }
}
