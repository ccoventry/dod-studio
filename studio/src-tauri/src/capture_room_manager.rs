//! Render While Capturing (#861), the running half: follows a capture batch
//! clip by clip, hands each recorded take to the Render tab, and holds the
//! batch between two clips when the next one would not fit until renders
//! free the space (`native::hlcr::capture_room` decides; this measures and
//! acts).
//!
//! - **A clip starts recording** (`START_RECORD`): is there room for it to
//!   finish *and* for the clip after it? If not, the hook's batch is asked to
//!   hold at the next clip boundary (`ds_capture_batch hold`), and a watcher
//!   checks the drive every second until `may_resume` says go.
//! - **A clip stops recording** (`STOP_RECORD`): once its take stops growing
//!   it is sent to the frontend (`capture_clip_ready`), which queues its
//!   render; the Render tab runs one render at a time while the capture runs
//!   and deletes each take as its clip finishes, per "After a clip is
//!   finished" (#644).
//! - **Held and it can never fit** (every pending take rendered and deleted
//!   would still leave it short, or nothing is rendering any more): the batch
//!   is cancelled at the boundary, with what was recorded kept, rather than
//!   filling the drive.
//!
//! Only a batch the hook DLL runs (Seek Between Clips) can hold: a patched
//! batch still renders as it goes, but can't wait.

use crate::render_manager::RenderManager;
use crate::settings_manager::SettingsManager;
use native::hlcr::capture_room::{
    ClipSizeModel, OutputRatio, PendingTake, RoomDecision, RoomInputs, decide, hold_seconds,
    may_resume,
};
use native::hlcr::take_cleanup::{AfterFinish, CleanupPolicy, format_gb, volume_of};
use native::log_markdown;
use native::patch::types::CaptureBlock;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// How long a take may keep changing after `STOP_RECORD` before it is handed
/// over anyway: HLAE's FFmpeg closes the video a moment after the stop.
const SETTLE_LIMIT: Duration = Duration::from_secs(120);
/// How often a settling take and a held batch are looked at.
const POLL: Duration = Duration::from_millis(500);
const HOLD_POLL: Duration = Duration::from_secs(1);
/// A block folder holding less than this has no take left in it.
const GONE_BELOW_BYTES: u64 = 1024 * 1024;
/// A hold with nothing queued or rendering this long can't end on its own.
const IDLE_HOLD_LIMIT: Duration = Duration::from_secs(60);

/// One block of the batch, in manifest order.
#[derive(Debug, Clone)]
struct RoomBlock {
    folder: PathBuf,
    /// Record span in demo ticks, the unit the size model learns in.
    ticks: u64,
    /// The planner's share of the batch estimate for this block.
    planned_bytes: u64,
    /// The ticks its `START_RECORD` and `STOP_RECORD` markers name.
    start_tick: i64,
    stop_tick: i64,
    /// What the frontend needs to mark the block's highlights Captured and
    /// name its clip (`capture_clip_ready`).
    take_key: String,
    source_streak_indices: Vec<usize>,
}

/// Which block a record marker naming `tick` belongs to: the first from
/// `cursor` on whose tick (`tick_of`) matches. A demo the hook's batch
/// skipped sends no markers for its blocks, so the next marker can belong to
/// a block further on; with no tick, or none matching, the one at `cursor`.
fn resolve_block(
    blocks: &[RoomBlock],
    cursor: usize,
    tick: Option<i64>,
    tick_of: impl Fn(&RoomBlock) -> i64,
) -> usize {
    tick.and_then(|t| {
        blocks
            .iter()
            .enumerate()
            .skip(cursor)
            .find(|(_, b)| tick_of(b) == t)
            .map(|(i, _)| i)
    })
    .unwrap_or(cursor)
}

#[derive(Debug, Default)]
struct State {
    sizes: ClipSizeModel,
    ratio: OutputRatio,
    /// Blocks recorded so far, with what they held once settled.
    recorded: Vec<(usize, u64)>,
    /// Partial takes (#1105), with what they hold: kept for the user and
    /// never deleted from here, so a hold never counts on their space.
    partial: Vec<(usize, u64)>,
    /// The block recording now, if any.
    recording: Option<usize>,
    /// A watcher is running.
    watching: bool,
    /// The game said it is holding.
    held: bool,
    /// Takes recorded but not yet settled: their size isn't learned yet, so
    /// no hold gives up while one is.
    settling: usize,
    /// The block the next `START_RECORD` / `STOP_RECORD` is looked for from.
    next_start: usize,
    next_stop: usize,
}

impl State {
    /// Block `index`'s take settled holding `settled` bytes (`None`: it
    /// didn't). A whole take is a pending one a render will free, and teaches
    /// the size model; a partial one is never rendered or deleted from here,
    /// so it stays out of both and a hold never waits for its space.
    fn take_settled(&mut self, index: usize, ticks: u64, settled: Option<u64>, partial: bool) {
        self.settling = self.settling.saturating_sub(1);
        if self.recording == Some(index) {
            self.recording = None;
        }
        match settled {
            Some(bytes) if partial => self.partial.push((index, bytes)),
            Some(bytes) => {
                self.sizes.learn(bytes, ticks);
                self.recorded.push((index, bytes));
            }
            None => {}
        }
    }
}

/// What a hold that gives up adds about the partial takes on its drive:
/// space it could never get back, since they are kept for the user.
fn partial_note(partial_bytes: u64, count: usize) -> String {
    if count == 0 {
        return String::new();
    }
    format!(
        " ({} of the drive is {count} partial take{} kept for you to check, which Studio never deletes)",
        format_gb(partial_bytes),
        if count == 1 { "" } else { "s" }
    )
}

/// One look at the drive before a clip boundary.
#[derive(Debug, Clone, PartialEq)]
struct Check {
    decision: RoomDecision,
    resume: bool,
    needed: u64,
    free: u64,
    next_bytes: u64,
    writing: u64,
}

/// What a running batch's Render While Capturing needs; one per batch.
pub(crate) struct CaptureRoom {
    app: AppHandle,
    blocks: Vec<RoomBlock>,
    session_id: String,
    capture_fps: i32,
    capture_mode: String,
    /// True asks the hook batch to hold (`capture_engine` sends it).
    pub hold: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    /// The batch can hold at all (the hook runs it).
    can_hold: bool,
    state: Mutex<State>,
}

/// The block folders, each with its share of `planned_total` by record span.
fn room_blocks(blocks: &[CaptureBlock], planned_total: u64) -> Vec<RoomBlock> {
    let spans: Vec<u64> = blocks
        .iter()
        .map(|b| (i64::from(b.record_stop_tick) - i64::from(b.record_start_tick)).max(1) as u64)
        .collect();
    let all: u64 = spans.iter().sum::<u64>().max(1);
    blocks
        .iter()
        .zip(spans)
        .map(|(b, ticks)| RoomBlock {
            folder: b.take_folder.clone(),
            ticks,
            planned_bytes: ((planned_total as f64) * (ticks as f64) / (all as f64)).ceil() as u64,
            start_tick: i64::from(b.record_start_tick),
            stop_tick: i64::from(b.record_stop_tick),
            take_key: b.take_key.clone(),
            source_streak_indices: b.source_streak_indices.clone(),
        })
        .collect()
}

fn policy(app: &AppHandle) -> CleanupPolicy {
    let settings = app.state::<SettingsManager>();
    let s = settings.inner.lock().unwrap();
    CleanupPolicy {
        after_finish: AfterFinish::from_setting(&s.take_after_finish),
        only_when_space_needed: s.take_delete_only_when_space_needed,
    }
}

fn export_volumes(app: &AppHandle) -> Vec<String> {
    let settings = app.state::<SettingsManager>();
    let s = settings.inner.lock().unwrap();
    s.render_export_dirs
        .iter()
        .filter(|d| !d.trim().is_empty())
        .map(|d| volume_of(Path::new(d)))
        .collect()
}

fn folder_bytes(path: &Path) -> u64 {
    native::sys::folder_walk::Walker::new()
        .files(path)
        .iter()
        .map(|file| std::fs::metadata(file).map(|m| m.len()).unwrap_or(0))
        .sum()
}

/// Whether Render While Capturing is on in the saved settings.
pub(crate) fn enabled(app: &AppHandle) -> bool {
    app.state::<SettingsManager>()
        .inner
        .lock()
        .unwrap()
        .render_while_capturing
}

impl CaptureRoom {
    pub(crate) fn new(
        app: AppHandle,
        blocks: &[CaptureBlock],
        planned_total: u64,
        session_id: String,
        capture_fps: i32,
        capture_mode: String,
        cancel: Arc<AtomicBool>,
        can_hold: bool,
    ) -> Arc<Self> {
        app.state::<RenderManager>().set_while_capturing(true);
        log_markdown(&format!(
            "[render-while-capturing] on for {} clip(s): each take renders once recorded, one render at a time{}",
            blocks.len(),
            if can_hold {
                ", and the batch holds between clips when the next one would not fit"
            } else {
                "; this batch is patched, so it can't hold for space"
            }
        ));
        Arc::new(Self {
            app,
            blocks: room_blocks(blocks, planned_total),
            session_id,
            capture_fps,
            capture_mode,
            hold: Arc::new(AtomicBool::new(false)),
            cancel,
            can_hold,
            state: Mutex::new(State::default()),
        })
    }

    /// The batch ended, however it ended.
    pub(crate) fn finish(&self) {
        self.hold.store(false, Ordering::Relaxed);
        self.app.state::<RenderManager>().set_while_capturing(false);
        let _ = self
            .app
            .emit("capture_room_status", serde_json::Value::Null);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What the next clip after `after` needs, or `None` after the last.
    fn clip_bytes(&self, state: &State, index: usize) -> Option<u64> {
        let block = self.blocks.get(index)?;
        Some(state.sizes.estimate(block.ticks, block.planned_bytes))
    }

    /// The batch's pending takes on `volume`, priced from their renders.
    fn pending_on(&self, state: &State, volume: &str, policy: CleanupPolicy) -> Vec<PendingTake> {
        let render = self.app.state::<RenderManager>();
        let exports = export_volumes(&self.app);
        let deletes = policy.after_finish == AfterFinish::Delete;
        state
            .recorded
            .iter()
            .filter_map(|&(index, _)| {
                let block = self.blocks.get(index)?;
                if volume_of(&block.folder) != volume {
                    return None;
                }
                // Measured now: a deleted take leaves its block folder with
                // only the render history in it.
                let raw_bytes = folder_bytes(&block.folder);
                if raw_bytes < GONE_BELOW_BYTES {
                    return None;
                }
                let renders = render.renders_under(&block.folder.to_string_lossy());
                let finished: Vec<_> = renders.iter().filter(|r| r.status == "Finished").collect();
                let output_bytes = (!renders.is_empty() && finished.len() == renders.len())
                    .then(|| finished.iter().filter_map(|r| r.output_size_bytes).sum());
                let output_on_same_drive = if finished.is_empty() {
                    exports.iter().any(|v| v == volume)
                } else {
                    finished
                        .iter()
                        .any(|r| volume_of(Path::new(&r.output_path)) == volume)
                };
                Some(PendingTake {
                    raw_bytes,
                    output_bytes,
                    output_on_same_drive,
                    deleted_after_render: deletes,
                })
            })
            .collect()
    }

    /// The decision for the boundary before block `next`, with `writing`
    /// bytes still to come from the clip recording now. `None` after the
    /// last block, or when the drive's free space can't be read.
    fn check(&self, next: usize, writing: u64) -> Option<Check> {
        let state = self.lock();
        let next_bytes = self.clip_bytes(&state, next)?;
        let folder = &self.blocks[next].folder;
        let free = native::sys::disk::get_available_bytes(existing_ancestor(folder));
        if free == u64::MAX {
            return None;
        }
        let pending = self.pending_on(&state, &volume_of(folder), policy(&self.app));
        let inputs = RoomInputs {
            free_bytes: free,
            headroom_bytes: native::sys::disk::MIN_DRIVE_HEADROOM_BYTES,
            writing_bytes: writing,
            next_clip_bytes: next_bytes,
            pending: &pending,
            output_ratio: state.ratio.get(),
        };
        Some(Check {
            decision: decide(&inputs),
            resume: may_resume(&inputs),
            needed: inputs.needed_bytes(),
            free,
            next_bytes,
            writing,
        })
    }

    /// Whether blocks `a` and `b` record to the same drive.
    fn same_drive(&self, a: usize, b: usize) -> bool {
        match (self.blocks.get(a), self.blocks.get(b)) {
            (Some(a), Some(b)) => volume_of(&a.folder) == volume_of(&b.folder),
            _ => false,
        }
    }

    /// `START_RECORD` naming `tick`.
    pub(crate) fn clip_started(self: &Arc<Self>, tick: Option<i64>) {
        let index = {
            let mut state = self.lock();
            let index = resolve_block(&self.blocks, state.next_start, tick, |b| b.start_tick);
            state.next_start = index + 1;
            state.recording = Some(index);
            if state.watching || !self.can_hold {
                return;
            }
            index
        };
        // The clip recording now only takes room from the next one's drive
        // when it records there too.
        let writing = if self.same_drive(index, index + 1) {
            self.clip_bytes(&self.lock(), index).unwrap_or(0)
        } else {
            0
        };
        let Some(check) = self.check(index + 1, writing) else {
            return;
        };
        if check.decision == RoomDecision::Continue {
            return;
        }
        log_markdown(&format!(
            "[render-while-capturing] Clip {} needs {} free with the one recording now (free {}): {:?}. Holding the batch after this clip.",
            index + 2,
            format_gb(check.needed),
            format_gb(check.free),
            check.decision
        ));
        self.hold.store(true, Ordering::Relaxed);
        self.lock().watching = true;
        self.emit_status(&STRINGS_HOLDING, None);
        let room = Arc::clone(self);
        std::thread::spawn(move || room.watch(index + 1));
    }

    /// `STOP_RECORD` naming `tick`: hand the take over once it settles.
    pub(crate) fn clip_recorded(self: &Arc<Self>, tick: Option<i64>) {
        let (index, block) = {
            let mut state = self.lock();
            let index = resolve_block(&self.blocks, state.next_stop, tick, |b| b.stop_tick);
            state.next_stop = index + 1;
            let Some(block) = self.blocks.get(index).cloned() else {
                return;
            };
            state.settling += 1;
            (index, block)
        };
        let room = Arc::clone(self);
        std::thread::spawn(move || {
            let settled = settle(&block.folder, &room.cancel);
            // A take that stopped writing early (#1105) is partial: never
            // rendered (and so never deleted) from here, nor learned from.
            // Take verification after the batch calls it out.
            let short = settled.and_then(|_| {
                native::hlcr::take_length::short_take(&block.folder, room.capture_fps)
            });
            room.lock()
                .take_settled(index, block.ticks, settled, short.is_some());
            let Some(bytes) = settled else {
                // Nothing there, still changing, or the batch was cancelled:
                // the finish step after the batch sees it, as without this
                // setting. A take still being written is never rendered (and
                // so never deleted) from here.
                log_markdown(&format!(
                    "[render-while-capturing] Clip {} at {} didn't settle into a take to render; left for after the batch",
                    index + 1,
                    block.folder.display()
                ));
                return;
            };
            if let Some(short) = short {
                log_markdown(&format!(
                    "[render-while-capturing] Clip {} at {} stopped writing video early ({:.2} s of video, {:.2} s of audio); not rendering a partial take",
                    index + 1,
                    block.folder.display(),
                    short.video_secs,
                    short.audio_secs
                ));
                return;
            }
            log_markdown(&format!(
                "[render-while-capturing] Clip {} recorded ({}); queueing its render",
                index + 1,
                format_gb(bytes)
            ));
            let _ = room.app.emit(
                "capture_clip_ready",
                serde_json::json!({
                    "session_id": room.session_id,
                    "block_index": index,
                    "take_folder": block.folder.to_string_lossy(),
                    "take_key": block.take_key,
                    "source_streak_indices": block.source_streak_indices,
                    "capture_fps": room.capture_fps,
                    "capture_mode": room.capture_mode,
                }),
            );
        });
    }

    /// The game held (`BATCH_HELD`) or went on (`BATCH_GOING`). The hook
    /// repeats `BATCH_HELD` while it holds, to notice a Studio that has gone,
    /// so only a change is logged.
    pub(crate) fn game_held(&self, held: bool) {
        let was = std::mem::replace(&mut self.lock().held, held);
        self.app.state::<RenderManager>().set_capture_held(held);
        if held && !was {
            log_markdown("[render-while-capturing] The batch is holding between clips");
        }
    }

    /// While held: learn from finished renders, make room if the setting
    /// waits for need, and let the batch go once `may_resume` says so.
    fn watch(self: Arc<Self>, next: usize) {
        let started = Instant::now();
        let mut idle_since: Option<Instant> = None;
        let mut first_free: Option<(Instant, u64)> = None;
        loop {
            if self.cancel.load(Ordering::Relaxed) || !self.hold.load(Ordering::Relaxed) {
                break;
            }
            self.learn_ratio();
            let (held, settling) = {
                let state = self.lock();
                (state.held, state.settling > 0)
            };
            let writing = if held {
                0
            } else {
                let state = self.lock();
                match state.recording {
                    Some(i) if self.same_drive(i, next) => {
                        let est = self.clip_bytes(&state, i).unwrap_or(0);
                        est.saturating_sub(folder_bytes(&self.blocks[i].folder))
                    }
                    _ => 0,
                }
            };
            let Some(check) = self.check(next, writing) else {
                // Can't read the drive: don't hold on a guess.
                self.hold.store(false, Ordering::Relaxed);
                break;
            };
            let free = check.free;
            let (at, free0) = *first_free.get_or_insert((Instant::now(), free));
            let rate =
                (free > free0).then(|| (free - free0) as f64 / at.elapsed().as_secs_f64().max(1.0));
            let short = check.needed.saturating_sub(free);

            if policy(&self.app).makes_room() && short > 0 {
                crate::take_cleanup_manager::make_room(
                    &self.app,
                    crate::take_cleanup_manager::RoomFor::Capture,
                    native::hlcr::take_cleanup::SpaceNeed {
                        targets: vec![existing_ancestor(&self.blocks[next].folder).to_path_buf()],
                        // One target drive, so one piece: the block being
                        // written and the next one both land on it.
                        pieces: vec![check.writing.saturating_add(check.next_bytes)],
                        headroom_bytes: native::sys::disk::MIN_DRIVE_HEADROOM_BYTES,
                    },
                    Some(&self.cancel),
                    |_, _, _| {},
                );
            }

            if check.resume {
                log_markdown(&format!(
                    "[render-while-capturing] Room for clip {} ({} free); going on after {:.0} s",
                    next + 1,
                    format_gb(free),
                    started.elapsed().as_secs_f64()
                ));
                self.hold.store(false, Ordering::Relaxed);
                break;
            }
            let renders = self.app.state::<RenderManager>().has_pending_renders();
            idle_since = if renders {
                None
            } else {
                idle_since.or(Some(Instant::now()))
            };
            let stuck = held
                && !settling
                && (matches!(check.decision, RoomDecision::CannotFit { .. })
                    || idle_since.is_some_and(|t| t.elapsed() >= IDLE_HOLD_LIMIT));
            if stuck {
                let why = match check.decision {
                    RoomDecision::CannotFit { short_bytes } => format!(
                        "clip {} needs {} more than the drive will have even after every recorded take is rendered and deleted",
                        next + 1,
                        format_gb(short_bytes)
                    ),
                    _ => format!(
                        "clip {} needs {} more, and no render has run for a minute to free it",
                        next + 1,
                        format_gb(short)
                    ),
                };
                let why = why + &self.partial_on_drive(next);
                log_markdown(&format!(
                    "[render-while-capturing] Stopping the batch before clip {}: {why}. The clips recorded so far are kept.",
                    next + 1
                ));
                let _ = self.app.emit(
                    "capture_room_stopped",
                    serde_json::json!({ "clip": next + 1, "reason": why }),
                );
                self.cancel.store(true, Ordering::Relaxed);
                break;
            }
            let eta = hold_seconds(short, rate);
            self.emit_status(
                if held {
                    &STRINGS_HELD
                } else {
                    &STRINGS_HOLDING
                },
                Some((short, eta)),
            );
            std::thread::sleep(HOLD_POLL);
        }
        self.lock().watching = false;
        let _ = self
            .app
            .emit("capture_room_status", serde_json::Value::Null);
    }

    /// [`partial_note`] for the partial takes on block `next`'s drive.
    fn partial_on_drive(&self, next: usize) -> String {
        let state = self.lock();
        let on_drive: Vec<u64> = state
            .partial
            .iter()
            .filter(|&&(i, _)| self.same_drive(i, next))
            .map(|&(_, bytes)| bytes)
            .collect();
        partial_note(on_drive.iter().sum(), on_drive.len())
    }

    /// Learns clip bytes per take byte from this batch's finished renders.
    fn learn_ratio(&self) {
        let render = self.app.state::<RenderManager>();
        let mut state = self.lock();
        let recorded = state.recorded.clone();
        for (index, raw) in recorded {
            let Some(block) = self.blocks.get(index) else {
                continue;
            };
            let renders = render.renders_under(&block.folder.to_string_lossy());
            if !renders.is_empty() && renders.iter().all(|r| r.status == "Finished") {
                let out: u64 = renders.iter().filter_map(|r| r.output_size_bytes).sum();
                state.ratio.learn(raw, out);
            }
        }
    }

    fn emit_status(&self, which: &RoomLine, short: Option<(u64, Option<f64>)>) {
        let _ = self.app.emit(
            "capture_room_status",
            serde_json::json!({
                "state": which.0,
                "short_bytes": short.map(|s| s.0),
                "eta_seconds": short.and_then(|s| s.1),
            }),
        );
    }
}

/// The frontend words each state (`render_while_capturing.js`).
struct RoomLine(&'static str);
const STRINGS_HOLDING: RoomLine = RoomLine("holding");
const STRINGS_HELD: RoomLine = RoomLine("held");

/// Waits for a take to stop changing (HLAE's FFmpeg closes the video a
/// moment after the stop) and returns what it holds. `None` when it holds
/// nothing, was still changing at [`SETTLE_LIMIT`], or the batch was
/// cancelled: a take that may still be written is never handed over.
fn settle(folder: &Path, cancel: &AtomicBool) -> Option<u64> {
    let started = Instant::now();
    let mut last = None;
    let mut steady = 0;
    while started.elapsed() < SETTLE_LIMIT && !cancel.load(Ordering::Relaxed) {
        let bytes = folder_bytes(folder);
        if bytes > 0 && Some(bytes) == last {
            steady += 1;
            // Two looks a second apart with nothing written, and a take the
            // Render tab would admit.
            if steady >= 2 && take_renderable(folder) {
                return Some(bytes);
            }
        } else {
            steady = 0;
        }
        last = Some(bytes);
        std::thread::sleep(POLL * 2);
    }
    None
}

/// The nearest folder of `path` that exists, for the free-space query: a
/// block's folder (and its session's) only appear once HLAE writes there.
fn existing_ancestor(path: &Path) -> &Path {
    path.ancestors().find(|p| p.is_dir()).unwrap_or(path)
}

/// HLAE writes the take into a numbered folder inside the block's.
fn take_renderable(folder: &Path) -> bool {
    native::hlcr::scanner::is_renderable_take(folder)
        || std::fs::read_dir(folder)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| native::hlcr::scanner::is_renderable_take(&e.path()))
            })
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(start: i32, stop: i32) -> CaptureBlock {
        CaptureBlock {
            demo_name: "d".into(),
            block_index: 0,
            drive_index: 0,
            take_folder: PathBuf::from("x"),
            take_key: String::new(),
            source_streak_indices: vec![],
            start_tick: start,
            end_tick: stop,
            record_start_tick: start,
            record_stop_tick: stop,
        }
    }

    #[test]
    fn a_record_marker_finds_its_block_by_tick_past_a_skipped_demo() {
        let blocks = room_blocks(&[block(100, 200), block(300, 400), block(500, 600)], 3000);
        let start = |b: &RoomBlock| b.start_tick;
        let stop = |b: &RoomBlock| b.stop_tick;
        assert_eq!(resolve_block(&blocks, 0, Some(100), start), 0);
        // Block 1's demo was skipped: block 2's marker comes next.
        assert_eq!(resolve_block(&blocks, 1, Some(500), start), 2);
        assert_eq!(resolve_block(&blocks, 1, Some(600), stop), 2);
        // An earlier block's tick is never matched again.
        assert_eq!(resolve_block(&blocks, 2, Some(100), start), 2);
        // No tick, or one nothing names: the next in order.
        assert_eq!(resolve_block(&blocks, 1, None, start), 1);
        assert_eq!(resolve_block(&blocks, 1, Some(7), start), 1);
        // Past the end stays past the end, for the callers' `get`.
        assert_eq!(resolve_block(&blocks, 3, Some(100), start), 3);
    }

    #[test]
    fn a_take_that_never_settles_is_not_handed_over() {
        let cancel = AtomicBool::new(true);
        let dir = std::env::temp_dir().join("dod_test_rwc_settle_cancelled");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("video.avi"), b"frames").unwrap();
        assert_eq!(settle(&dir, &cancel), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A partial take (#1105) is kept, never rendered or deleted from here:
    /// a hold that only its space could end gives up at once instead of
    /// waiting for it, and says why.
    #[test]
    fn a_partial_take_is_never_counted_on_to_free_space() {
        const GB: u64 = 1024 * 1024 * 1024;
        let mut state = State {
            settling: 2,
            recording: Some(1),
            ..State::default()
        };
        state.take_settled(0, 100, Some(4 * GB), false);
        state.take_settled(1, 100, Some(5 * GB), true);
        assert_eq!(state.settling, 0);
        assert_eq!(state.recording, None);
        assert_eq!(state.recorded, vec![(0, 4 * GB)]);
        assert_eq!(state.partial, vec![(1, 5 * GB)]);

        // Held with only the partial take on the drive: the pending list
        // (built from `recorded`) has nothing that will free space.
        let inputs = RoomInputs {
            free_bytes: GB,
            headroom_bytes: GB,
            writing_bytes: 0,
            next_clip_bytes: 3 * GB,
            pending: &[],
            output_ratio: None,
        };
        assert!(matches!(decide(&inputs), RoomDecision::CannotFit { .. }));
        assert_eq!(
            partial_note(5 * GB, 1),
            format!(
                " ({} of the drive is 1 partial take kept for you to check, which Studio never deletes)",
                format_gb(5 * GB)
            )
        );
        assert_eq!(partial_note(0, 0), "");

        // A take that never settled is in neither list.
        state.settling = 1;
        state.take_settled(2, 100, None, false);
        assert_eq!((state.recorded.len(), state.partial.len()), (1, 1));
    }

    #[test]
    fn each_block_gets_the_planners_share_by_its_record_span() {
        let blocks = room_blocks(&[block(0, 100), block(200, 500)], 4000);
        assert_eq!(blocks[0].ticks, 100);
        assert_eq!(blocks[0].planned_bytes, 1000);
        assert_eq!(blocks[1].planned_bytes, 3000);
        // A block with no span still counts one tick rather than dividing by 0.
        let blocks = room_blocks(&[block(5, 5)], 10);
        assert_eq!(blocks[0].ticks, 1);
        assert_eq!(blocks[0].planned_bytes, 10);
    }
}
