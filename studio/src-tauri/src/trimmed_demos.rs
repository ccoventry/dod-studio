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
use native::patch::entity_trim::{Step, TrimError};
use native::patch::trimmed_copies::{self, Engine, Playable, prepare_playable};
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
fn throttled(name: String, mut report: impl FnMut(String)) -> impl FnMut(Step) {
    let mut last: Option<(Instant, std::mem::Discriminant<Step>)> = None;
    move |step: Step| {
        let kind = std::mem::discriminant(&step);
        if last.is_some_and(|(at, k)| k == kind && at.elapsed() < REPORT_EVERY) {
            return;
        }
        last = Some((Instant::now(), kind));
        report(crate::messages::trimming_demo(&name, step));
    }
}

/// The trimmed copy each of `jobs`' demos plays from, keyed by the original's
/// path; demos under the limit aren't in it. Reports each trim on the Capture
/// footer (`capture_status`). `Err` is the batch's error line: a demo that
/// can't be trimmed safely, or Cancel.
pub fn for_batch(
    app: &tauri::AppHandle,
    jobs: &[PatchJob],
    game_path: &str,
    cancel: &AtomicBool,
) -> Result<HashMap<String, PathBuf>, String> {
    let engine = Engine::of(Path::new(game_path));
    let mut demos: Vec<&str> = Vec::new();
    for job in jobs.iter().filter(|j| !j.blocks.is_empty()) {
        if !demos.contains(&job.source_demo.as_str()) {
            demos.push(&job.source_demo);
        }
    }
    let total = demos.len() as u32;
    let mut copies = HashMap::new();
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
        match prepare_playable(path, &engine, &is_cancelled, &mut progress) {
            Ok(Playable::Original) => {}
            Ok(Playable::Copy { path: copy, .. }) => {
                copies.insert(demo.to_string(), copy);
            }
            Err(TrimError::Cancelled) => {
                return Err(crate::messages::CAPTURE_CANCELLED_WHILE_TRIMMING.to_string());
            }
            Err(e) => {
                let line = crate::messages::demo_cannot_be_trimmed(&file_name(path), &engine, &e);
                native::log_markdown(&format!("⛔ **Batch not started** — {line}"));
                return Err(line);
            }
        }
    }
    trimmed_copies::prune_keeping(&trimmed_copies::copies_root(), copies.len());
    Ok(copies)
}

/// The trimmed copy a Launch Preview of `demo` plays, or `None` when the
/// original plays. Reports each step on [`PREVIEW_EVENT`], and a last line
/// saying the preview plays a copy. `Err` when the demo can't be trimmed
/// safely: the preview must not start then.
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
            trimmed_copies::prune_keeping(&trimmed_copies::copies_root(), 1);
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
