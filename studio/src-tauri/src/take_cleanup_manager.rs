//! The automatic half of #644: "After a clip is finished" (Configuration >
//! Render Output) keeps a take, sends it to the Recycle Bin, or deletes it for
//! good, and "Only when space is needed" holds the permanent delete back until
//! a capture or finish job is short of room.
//!
//! Two moments act on it:
//!
//! - **A render batch ends** (`render_manager`'s scheduler, after its own
//!   `render_batch_finished`): each take that had a job finish in that run,
//!   with every one of its jobs Finished, goes. After the batch's end rather
//!   than as each job finishes, so the Render tab's summary and the finish
//!   step (#440) have counted the rows before they leave the list, and a Reset
//!   can't aim at a folder that's gone.
//! - **A capture or finish job is about to start** (`capture_manager`,
//!   `start_queued_render`): with "Only when space is needed", the oldest
//!   finished takes are deleted until the job fits.
//!
//! Which takes may go is `native::hlcr::take_cleanup`'s (a verified render of
//! every stream, its clip file still there, nothing queued or rendering), and
//! the delete itself is `take_delete`'s, with the Delete Take button's safety
//! check. Every take that goes is written to the activity log with its size,
//! where it went, and why.

use crate::render_manager::RenderManager;
use crate::settings_manager::SettingsManager;
use crate::take_delete_manager::capture_dirs;
use native::hlcr::take_cleanup::{
    AfterFinish, CleanupPolicy, RoomPlan, RoomStep, SpaceNeed, finished_at, folder_key, format_gb,
    plan_room,
};
use native::hlcr::take_delete::{
    TakeDeletion, delete_take_permanently, plan_take_deletion, recycle_take,
};
use native::log_markdown;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The setting as saved.
fn policy(settings: &SettingsManager) -> CleanupPolicy {
    let s = settings.inner.lock().unwrap();
    CleanupPolicy {
        after_finish: AfterFinish::from_setting(&s.take_after_finish),
        only_when_space_needed: s.take_delete_only_when_space_needed,
    }
}

/// Whether a job about to start should make room first. Checked before the
/// job's need is worked out, which costs a little.
pub(crate) fn makes_room(app: &AppHandle) -> bool {
    policy(&app.state::<SettingsManager>()).makes_room()
}

/// The job room is made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoomFor {
    Capture,
    Finish,
}

/// Why a take went: the log line's reason and the toast's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    Finished,
    Room(RoomFor),
}

impl Why {
    fn id(self) -> &'static str {
        match self {
            Why::Finished => "finished",
            Why::Room(RoomFor::Capture) => "capture",
            Why::Room(RoomFor::Finish) => "finish",
        }
    }

    fn because(self) -> &'static str {
        match self {
            Why::Finished => "its clip finished",
            Why::Room(RoomFor::Capture) => "space needed for a capture",
            Why::Room(RoomFor::Finish) => "space needed to finish clips",
        }
    }

    fn job(self) -> &'static str {
        match self {
            Why::Room(RoomFor::Capture) => "a capture",
            _ => "finishing clips",
        }
    }
}

/// The activity-log line for a take that went.
fn removed_line(take_folder: &Path, size_bytes: u64, method: AfterFinish, why: Why) -> String {
    let size = format_gb(size_bytes);
    match method {
        AfterFinish::Recycle => format!(
            "[take-cleanup] Moved {} to the Recycle Bin ({size}): {}.",
            take_folder.display(),
            why.because()
        ),
        _ => format!(
            "[take-cleanup] Deleted {} for good ({size}): {}.",
            take_folder.display(),
            why.because()
        ),
    }
}

/// Whether a take's jobs say its clip is done: listed, and every job (two for
/// a HUD pair) Finished. A failed, cancelled, queued or rendering half keeps
/// the take.
fn all_jobs_finished(statuses: &[String]) -> bool {
    !statuses.is_empty() && statuses.iter().all(|s| s == "Finished")
}

#[derive(Serialize, Debug, Clone, PartialEq)]
struct Removed {
    take_folder: String,
    take_key: String,
    size_bytes: u64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
struct Failure {
    take_folder: String,
    error: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
struct Short {
    needed_bytes: u64,
    available_bytes: u64,
}

/// `take_cleanup_done`: what went, what couldn't, or why nothing did.
#[derive(Serialize, Debug, Clone)]
struct Done {
    reason: &'static str,
    method: &'static str,
    deleted: Vec<Removed>,
    failed: Vec<Failure>,
    short: Option<Short>,
}

impl Done {
    fn new(why: Why, method: AfterFinish) -> Self {
        Self {
            reason: why.id(),
            method: if method == AfterFinish::Recycle {
                "recycle"
            } else {
                "delete"
            },
            deleted: Vec::new(),
            failed: Vec::new(),
            short: None,
        }
    }

    fn record(
        &mut self,
        why: Why,
        method: AfterFinish,
        take_folder: &Path,
        outcome: Result<TakeDeletion, String>,
    ) {
        match outcome {
            Ok(plan) => {
                log_markdown(&removed_line(
                    &plan.take_folder,
                    plan.size_bytes,
                    method,
                    why,
                ));
                self.deleted.push(Removed {
                    take_folder: plan.take_folder.to_string_lossy().into_owned(),
                    take_key: plan.take_key,
                    size_bytes: plan.size_bytes,
                });
            }
            Err(error) => {
                log_markdown(&format!("[take-cleanup] {error}"));
                self.failed.push(Failure {
                    take_folder: take_folder.to_string_lossy().into_owned(),
                    error,
                });
            }
        }
    }

    fn emit(self, app: &AppHandle) {
        let _ = app.emit("take_cleanup_done", self);
    }
}

/// `take_cleanup_progress` events: which step, and takes done of total. At
/// most one every ~33 ms, plus the first and the last of each step.
struct Progress {
    app: AppHandle,
    reason: &'static str,
    step: &'static str,
    last: Option<Instant>,
}

impl Progress {
    fn new(app: AppHandle, why: Why) -> Self {
        Self {
            app,
            reason: why.id(),
            step: "",
            last: None,
        }
    }

    /// Emits when due and says whether it did, so a caller's own status line
    /// keeps the same pace.
    fn report(&mut self, step: &'static str, done: usize, total: usize) -> bool {
        let due = step != self.step
            || done == 0
            || done >= total
            || self
                .last
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(33));
        if !due {
            return false;
        }
        self.step = step;
        self.last = Some(Instant::now());
        let _ = self.app.emit(
            "take_cleanup_progress",
            serde_json::json!({ "step": step, "done": done, "total": total, "reason": self.reason }),
        );
        true
    }
}

/// Why a take's rows keep it: one queued or rendering. Delete Take's own
/// check lets a queued row go with the rest, which is right for a click but
/// not here: a row queued again (Retry) after this cleanup looked is the
/// user asking for that take, so it stays.
pub(crate) fn busy_refusal(take_folder: &str, statuses: &[&str]) -> Option<String> {
    statuses
        .iter()
        .any(|s| *s == "Queued" || *s == "Rendering")
        .then(|| crate::messages::take_is_queued_or_rendering(take_folder))
}

/// Takes the rows of a take out of the Render tab before its folder goes, so a
/// Reset can't start a render of a folder that's gone. `remove_idle_take_rows`
/// re-checks under the job lock that none is queued or rendering. A take
/// that isn't listed has nothing to remove.
fn take_out_of_list(
    app: &AppHandle,
    render: &RenderManager,
    take_folder: &Path,
) -> Result<(), String> {
    let key = folder_key(&take_folder.to_string_lossy());
    match render.listed_take_folder(&key) {
        Some(listed) => render.remove_idle_take_rows(app, &listed),
        None => Ok(()),
    }
}

/// After a render batch: the takes in `finished_takes` (each had a job finish
/// in that run) go, if the setting says so right after finish. Runs on a
/// blocking thread; returns at once.
pub(crate) fn after_render_batch(app: &AppHandle, finished_takes: Vec<String>) {
    if finished_takes.is_empty() {
        return;
    }
    let Some(method) = policy(&app.state::<SettingsManager>()).on_finish() else {
        return;
    };
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let render = app.state::<RenderManager>();
        let dirs = capture_dirs(&app.state::<SettingsManager>());
        let why = Why::Finished;
        let mut progress = Progress::new(app.clone(), why);
        let mut done = Done::new(why, method);
        let step = if method == AfterFinish::Recycle {
            "recycling"
        } else {
            "deleting"
        };
        let total = finished_takes.len();
        for (i, folder) in finished_takes.iter().enumerate() {
            progress.report(step, i, total);
            let path = Path::new(folder);
            if !all_jobs_finished(&render.take_statuses(folder)) {
                continue;
            }
            let history = native::hlcr::take_meta::read_history(path);
            let streams = native::hlcr::take_cleanup::streams_on_disk(path);
            if finished_at(&history, &streams, |p| Path::new(p).is_file()).is_none() {
                log_markdown(&format!(
                    "[take-cleanup] {} stays: its render history doesn't show every stream finished with the clip file still there.",
                    path.display()
                ));
                continue;
            }
            // The Delete Take check first, so a take that won't be deleted
            // (say, rendered from outside the Destinations folders) keeps its
            // rows.
            if let Err(e) = plan_take_deletion(path, &dirs) {
                log_markdown(&format!("[take-cleanup] {} stays: {e}", path.display()));
                continue;
            }
            if let Err(e) = take_out_of_list(&app, &render, path) {
                log_markdown(&format!("[take-cleanup] {} stays: {e}", path.display()));
                continue;
            }
            let outcome = match method {
                AfterFinish::Recycle => plan_take_deletion(path, &dirs)
                    .and_then(|plan| recycle_take(&plan).map(|()| plan)),
                _ => delete_take_permanently(path, &dirs),
            };
            done.record(why, method, path, outcome);
        }
        progress.report(step, total, total);
        // Sent even when nothing went, so the progress bar goes away.
        done.emit(&app);
    });
}

/// Before a job: with "Only when space is needed", deletes the oldest finished
/// takes on the job's drives until `need` fits. Blocking.
///
/// `cancel` stops it between takes. `on_step(line, done, total)` gets each
/// progress line as it is emitted, for a caller with its own status line (the
/// Capture footer).
pub(crate) fn make_room(
    app: &AppHandle,
    room_for: RoomFor,
    need: SpaceNeed,
    cancel: Option<&AtomicBool>,
    on_step: impl Fn(&str, usize, usize),
) {
    let settings = app.state::<SettingsManager>();
    if !policy(&settings).makes_room() {
        return;
    }
    // The Destinations folders are the only place a take is ever deleted from.
    let dirs: Vec<PathBuf> = capture_dirs(&settings);
    if dirs.is_empty() || need.targets.is_empty() {
        return;
    }
    let render = app.state::<RenderManager>();
    let busy = render.busy_take_keys();
    let why = Why::Room(room_for);
    let mut progress = Progress::new(app.clone(), why);

    let plan = plan_room(
        &need,
        &dirs,
        &busy,
        native::sys::disk::get_available_bytes,
        |step| match step {
            RoomStep::Looking => {
                if progress.report("looking", 0, 0) {
                    on_step(crate::messages::MAKING_ROOM_LOOKING, 0, 0);
                }
            }
            RoomStep::Measuring { done, total } => {
                if progress.report("measuring", done, total) {
                    on_step(
                        &crate::messages::making_room_measuring(done, total),
                        done,
                        total,
                    );
                }
            }
        },
    );

    let mut done = Done::new(why, AfterFinish::Delete);
    let chosen = match plan {
        RoomPlan::Enough => return,
        RoomPlan::Unknown => {
            log_markdown(&format!(
                "[take-cleanup] Making room for {}: the free space on its drives can't be read, so no takes were deleted.",
                why.job()
            ));
            return;
        }
        RoomPlan::Short {
            needed_bytes,
            available_bytes,
        } => {
            log_markdown(&format!(
                "[take-cleanup] Making room for {}: it needs {} on its drives, and deleting every finished take there would give only {}. No takes were deleted.",
                why.job(),
                format_gb(needed_bytes),
                format_gb(available_bytes)
            ));
            done.short = Some(Short {
                needed_bytes,
                available_bytes,
            });
            done.emit(app);
            return;
        }
        RoomPlan::Delete(chosen) => chosen,
    };

    let total = chosen.len();
    for (i, plan) in chosen.iter().enumerate() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            log_markdown(&format!(
                "[take-cleanup] Making room for {} stopped: cancelled after {i} of {total} takes.",
                why.job()
            ));
            break;
        }
        if progress.report("deleting", i, total) {
            on_step(&crate::messages::making_room_deleting(i, total), i, total);
        }
        if let Err(e) = take_out_of_list(app, &render, &plan.take_folder) {
            log_markdown(&format!(
                "[take-cleanup] {} stays: {e}",
                plan.take_folder.display()
            ));
            continue;
        }
        let outcome = delete_take_permanently(&plan.take_folder, &dirs);
        done.record(why, AfterFinish::Delete, &plan.take_folder, outcome);
    }
    if progress.report("deleting", total, total) {
        on_step(
            &crate::messages::making_room_deleting(total, total),
            total,
            total,
        );
    }
    done.emit(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_goes_only_when_every_job_of_it_finished() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(all_jobs_finished(&s(&["Finished"])));
        assert!(all_jobs_finished(&s(&["Finished", "Finished"])));
        assert!(!all_jobs_finished(&s(&[])), "not listed");
        assert!(
            !all_jobs_finished(&s(&["Finished", "Error"])),
            "HUD half failed"
        );
        assert!(!all_jobs_finished(&s(&["Finished", "Cancelled"])));
        assert!(!all_jobs_finished(&s(&["Finished", "Queued"])));
        assert!(!all_jobs_finished(&s(&["Rendering"])));
    }

    #[test]
    fn a_queued_or_rendering_row_keeps_its_take() {
        assert!(busy_refusal("t", &["Finished"]).is_none());
        assert!(busy_refusal("t", &["Finished", "Error"]).is_none());
        assert!(busy_refusal("t", &[]).is_none(), "not listed");
        assert!(busy_refusal("t", &["Finished", "Queued"]).is_some());
        assert!(busy_refusal("t", &["Rendering"]).is_some());
    }

    #[test]
    fn the_log_line_says_which_take_how_much_where_and_why() {
        let take = PathBuf::from("D:")
            .join("cap")
            .join("s1")
            .join("b0")
            .join("take0000");
        let gib = 1_073_741_824 * 3 / 2;
        let line = removed_line(&take, gib, AfterFinish::Recycle, Why::Finished);
        assert!(line.contains(&take.display().to_string()), "{line}");
        assert!(line.contains("Recycle Bin"), "{line}");
        assert!(line.contains("1.5 GB"), "{line}");
        assert!(line.contains("its clip finished"), "{line}");
        let line = removed_line(&take, gib, AfterFinish::Delete, Why::Room(RoomFor::Capture));
        assert!(line.contains("for good"), "{line}");
        assert!(line.contains("space needed for a capture"), "{line}");
        let line = removed_line(&take, gib, AfterFinish::Delete, Why::Room(RoomFor::Finish));
        assert!(line.contains("space needed to finish clips"), "{line}");
    }

    #[test]
    fn a_done_event_names_its_reason_and_method() {
        let done = Done::new(Why::Room(RoomFor::Capture), AfterFinish::Delete);
        assert_eq!((done.reason, done.method), ("capture", "delete"));
        let done = Done::new(Why::Finished, AfterFinish::Recycle);
        assert_eq!((done.reason, done.method), ("finished", "recycle"));
    }
}
