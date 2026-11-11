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

/// Why `demo` can't play after [`prepare_playable`] failed with `e`: a file
/// that isn't there any more, or one that couldn't be read, says so rather
/// than claiming an entity problem nobody measured (a demo with no cached
/// peak is read before anything is known about its entities).
fn cannot_play_line(demo: &Path, engine: &Engine, e: &TrimError) -> String {
    if !demo.is_file() {
        return crate::messages::demo_file_missing(demo);
    }
    match e {
        TrimError::Unreadable(why) => {
            crate::messages::demo_could_not_be_read(&file_name(demo), why)
        }
        _ => crate::messages::demo_cannot_be_trimmed(&file_name(demo), engine, e),
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
/// can't pile up. `Err` is the batch's error line: a demo that isn't there,
/// can't be read or can't be trimmed safely, a copy with no room, or Cancel.
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
                let line = cannot_play_line(path, engine, &e);
                native::log_markdown(&crate::messages::batch::batch_not_started(&line));
                return Err(line);
            }
        }
    }
    Ok(())
}

/// The trimmed copy a Launch Preview of `demo` plays, or `None` when the
/// original plays. Reports each step on [`PREVIEW_EVENT`], and a last line
/// saying the preview plays a copy. `Err` when the demo isn't there or can't
/// be read, can't be trimmed safely or its copy has no room
/// ([`cannot_play_line`]): the preview must not start then.
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
            let line = cannot_play_line(demo, &engine, &e);
            native::log_markdown(&crate::messages::preview_start::preview_not_started(&line));
            Err(line)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use native::sys::packet_limit::{ANNIVERSARY_PACKET_ENTITIES, PRE_ANNIVERSARY_PACKET_ENTITIES};

    fn engine(limit: u32) -> Engine {
        Engine {
            limit,
            maps_dir: None,
        }
    }

    fn not_enough() -> TrimError {
        TrimError::NotEnough {
            peak: 1100,
            target: 1000,
            available: 10,
            without_map: false,
        }
    }

    /// A demo file that is there, removed when the test ends.
    struct TempDemo(PathBuf);

    impl TempDemo {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dodstudio-trimmed-demos-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, b"HLDEMO").unwrap();
            Self(path)
        }
    }

    impl Drop for TempDemo {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_demo_renamed_away_says_it_is_missing_not_that_it_has_too_many_entities() {
        let gone = std::env::temp_dir()
            .join("dodstudio-no-such-dir")
            .join("split1_harrington.dem");
        let e = TrimError::Unreadable(format!("{} could not be opened", gone.display()));
        let line = cannot_play_line(&gone, &engine(ANNIVERSARY_PACKET_ENTITIES), &e);
        assert!(line.contains(&gone.display().to_string()), "{line}");
        assert!(line.contains("isn't there any more"), "{line}");
        assert!(line.contains("Locate"), "{line}");
        assert!(!line.contains("entities"), "{line}");
        assert!(!line.contains("25th Anniversary install"), "{line}");
    }

    #[test]
    fn a_demo_that_is_there_but_unreadable_says_it_could_not_be_read() {
        let demo = TempDemo::new("unreadable.dem");
        let e = TrimError::Unreadable("bad header".to_string());
        let line = cannot_play_line(&demo.0, &engine(ANNIVERSARY_PACKET_ENTITIES), &e);
        assert!(
            line.starts_with("unreadable.dem could not be read"),
            "{line}"
        );
        assert!(line.contains("bad header"), "{line}");
        assert!(!line.contains("more entities in one snapshot"), "{line}");
    }

    #[test]
    fn an_untrimmable_demo_on_pre_anniversary_is_told_to_use_the_anniversary_install() {
        let demo = TempDemo::new("big_pre.dem");
        let line = cannot_play_line(
            &demo.0,
            &engine(PRE_ANNIVERSARY_PACKET_ENTITIES),
            &not_enough(),
        );
        assert!(
            line.contains("more entities in one snapshot than the pre-Anniversary engine"),
            "{line}"
        );
        assert!(
            line.contains("Play it with the 25th Anniversary install instead"),
            "{line}"
        );
    }

    #[test]
    fn an_untrimmable_demo_on_anniversary_is_never_told_to_switch_to_anniversary() {
        let demo = TempDemo::new("big_post.dem");
        let line = cannot_play_line(&demo.0, &engine(ANNIVERSARY_PACKET_ENTITIES), &not_enough());
        assert!(
            line.contains("than the 25th Anniversary engine takes (1024)"),
            "{line}"
        );
        assert!(!line.contains("25th Anniversary install"), "{line}");
        assert!(line.contains("untick its highlights"), "{line}");
    }

    #[test]
    fn no_room_for_a_copy_on_anniversary_only_says_to_free_space() {
        let demo = TempDemo::new("no_room.dem");
        let e = TrimError::NoRoom {
            folder: PathBuf::from("X:/copies"),
            free_bytes: 0,
            needed_bytes: 1,
        };
        let line = cannot_play_line(&demo.0, &engine(ANNIVERSARY_PACKET_ENTITIES), &e);
        assert!(line.contains("free some space there"), "{line}");
        assert!(!line.contains("can't be played"), "{line}");
        assert!(!line.contains("25th Anniversary install"), "{line}");
    }
}
