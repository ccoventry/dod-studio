//! Trimmed copies of demos the game's engine can't play (#207), the Studio
//! half: before a capture batch or a Launch Preview, each demo over the
//! engine's `MAX_PACKET_ENTITIES` is swapped for a trimmed copy
//! (`native::patch::trimmed_copies`), with a status line while one is made.
//! The original is never written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use native::patch::PatchJob;
use native::patch::entity_trim::TrimError;
use native::patch::trimmed_copies::{
    self, Engine, KEEP_COPIES, Pins, Playable, Progress, prepare_playable,
};
use tauri::Emitter;

/// The event a Launch Preview's trim reports on: `{ status, done }`.
pub const PREVIEW_EVENT: &str = "preview_trim";

/// The least time between two progress reports (CLAUDE.md's ~33 ms).
const REPORT_EVERY: Duration = Duration::from_millis(33);

fn file_name(demo: &Path) -> String {
    demo.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| demo.display().to_string())
}

/// Calls `report` with each step's line, at most every [`REPORT_EVERY`]
/// (a step change always reports).
fn throttled(name: String, mut report: impl FnMut(String)) -> impl FnMut(Progress) {
    let mut last: Option<(Instant, std::mem::Discriminant<Progress>)> = None;
    move |step: Progress| {
        let kind = std::mem::discriminant(&step);
        if last.is_some_and(|(at, k)| k == kind && at.elapsed() < REPORT_EVERY) {
            return;
        }
        last = Some((Instant::now(), kind));
        report(crate::messages::trimming_demo(&name, step));
    }
}

/// A batch's trimmed copies: the copy each demo plays from, keyed by the
/// original's path (demos under the limit aren't in it), and the pins that
/// keep them on disk. Hold `pins` until the batch's game is done with them.
pub struct BatchCopies {
    pub copies: HashMap<String, PathBuf>,
    pub pins: Pins,
}

/// The trimmed copies `jobs`' demos play from ([`BatchCopies`]). Reports each
/// step on the Capture footer (`capture_status`). Prunes the copies past
/// [`KEEP_COPIES`] however it ends, a failed or cancelled batch too, so they
/// can't pile up. `Err` is the batch's error line: a demo that can't be
/// trimmed safely, a copy with no room, or Cancel.
pub fn for_batch(
    app: &tauri::AppHandle,
    jobs: &[PatchJob],
    game_path: &str,
    cancel: &AtomicBool,
) -> Result<BatchCopies, String> {
    let engine = Engine::of(Path::new(game_path));
    let mut batch = BatchCopies {
        copies: HashMap::new(),
        pins: Pins::default(),
    };
    let made = make_batch_copies(app, jobs, &engine, cancel, &mut batch);
    trimmed_copies::prune(&trimmed_copies::copies_root(), KEEP_COPIES);
    made.map(|()| batch)
}

/// Fills `batch` with the copy of each of `jobs`' demos over the engine's
/// limit, pinning each as soon as it is ready.
fn make_batch_copies(
    app: &tauri::AppHandle,
    jobs: &[PatchJob],
    engine: &Engine,
    cancel: &AtomicBool,
    batch: &mut BatchCopies,
) -> Result<(), String> {
    let mut demos: Vec<&str> = Vec::new();
    for job in jobs.iter().filter(|j| !j.blocks.is_empty()) {
        if !demos.contains(&job.source_demo.as_str()) {
            demos.push(&job.source_demo);
        }
    }
    let total = demos.len() as u32;
    for (i, demo) in demos.iter().enumerate() {
        let path = Path::new(demo);
        let mut progress = throttled(file_name(path), |status| {
            let _ = app.emit(
                "capture_status",
                serde_json::json!({
                    "running": true,
                    "stage": "trimming",
                    "index": i as u32,
                    "total": total,
                    "measured": false,
                    "status": status,
                }),
            );
        });
        let is_cancelled = || cancel.load(Ordering::Relaxed);
        match prepare_playable(path, engine, &is_cancelled, &mut progress) {
            Ok(Playable::Original) => {}
            Ok(Playable::Copy { path: copy, .. }) => {
                batch.pins.add(&copy);
                batch.copies.insert(demo.to_string(), copy);
            }
            Err(TrimError::Cancelled) => {
                return Err(crate::messages::CAPTURE_CANCELLED_WHILE_TRIMMING.to_string());
            }
            Err(e) => {
                let line = crate::messages::demo_cannot_be_trimmed(&file_name(path), engine, &e);
                native::log_markdown(&format!("⛔ **Batch not started** — {line}"));
                return Err(line);
            }
        }
    }
    Ok(())
}

/// The trimmed copy a Launch Preview of `demo` plays, or `None` when the
/// original plays. Reports each step on [`PREVIEW_EVENT`], and a last line
/// saying the preview plays a copy. `Err` when the demo can't be trimmed
/// safely or its copy has no room: the preview must not start then.
pub fn for_preview(
    app: &tauri::AppHandle,
    demo: &Path,
    game_path: &str,
) -> Result<Option<PathBuf>, String> {
    let engine = Engine::of(Path::new(game_path));
    let emit = |status: String, done: bool| {
        let _ = app.emit(
            PREVIEW_EVENT,
            serde_json::json!({ "status": status, "done": done }),
        );
    };
    let mut progress = throttled(file_name(demo), |status| emit(status, false));
    match prepare_playable(demo, &engine, &|| false, &mut progress) {
        Ok(Playable::Original) => Ok(None),
        Ok(Playable::Copy { path, .. }) => {
            // The copy was just stamped used, so it is the newest and stays;
            // a running batch's copies are pinned.
            trimmed_copies::prune(&trimmed_copies::copies_root(), KEEP_COPIES);
            emit(crate::messages::preview_plays_trimmed_copy(&engine), true);
            Ok(Some(path))
        }
        Err(e) => {
            let line = crate::messages::demo_cannot_be_trimmed(&file_name(demo), &engine, &e);
            native::log_markdown(&format!("⛔ **Preview not started** — {line}"));
            Err(line)
        }
    }
}
