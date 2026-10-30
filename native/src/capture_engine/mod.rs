//! Runs one capture batch on its own thread, from the game folder's links
//! to the last event, and reports to Studio's listener over `EngineEvent`s.
//!
//! The stages, in order (`run_batch`), each returning at the first failure:
//! 1. `workspace`: clear what an earlier batch left, link the session and
//!    capture folders, copy the patched demos into `dod/`.
//! 2. `preflight`: the last free-space check.
//! 3. `obs_capture` (OBS mode) and `markers`: connect to OBS, start the
//!    marker readers.
//! 4. `launch`: the game's command line and start lines, the launcher.
//! 5. `watch`: the poll loop, until the batch ends, is cancelled or fails.
//! 6. The teardown here: late markers, OBS's stop, the batch's files, the
//!    last event.

mod game;
mod launch;
mod markers;
mod obs_capture;
mod preflight;
mod watch;
mod workspace;

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CaptureJob {
    pub patched_demo_path: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub enum EngineEvent {
    Starting(usize),
    Launching(String),
    Finished(String),
    Error(String),
    AllCompleted,
    /// Posted when the cancellation token is raised mid-batch.
    /// Signals the GUI to reset the running flag and show a cancelled message.
    Cancelled,
    /// A demo in the batch has started playing, read from its `DEMO_START`
    /// console marker: (job_idx, total_jobs, clip_count), 1-based job_idx.
    /// Read from the engine console log, which `-condebug` produces on every
    /// launch. See issue #98.
    DemoLoading(u32, u32, u32),
    /// Playback is about to fast-forward towards a specific clip, read from
    /// its `NEXT_CLIP` console marker: (job_idx, total_jobs, clip_idx,
    /// clip_count), all 1-based. Same console-log source as DemoLoading.
    /// See issue #98.
    FastForwardToClip(u32, u32, u32, u32),
    /// OBS mode: the frames OBS is losing, about once a second while a clip
    /// records and once more with its totals when it ends (#172).
    ObsDropRate(crate::obs::ObsDropReport),
    /// A clip's recording started (`START_RECORD`), with the demo tick the
    /// marker names: its block's `record_start_tick` (#861). The listener
    /// finds the block by it, since a skipped demo sends no markers for its
    /// clips and a plain count would fall behind the batch's order.
    ClipStarted(Option<i64>),
    /// A clip's recording stopped (`STOP_RECORD`), with its block's
    /// `record_stop_tick`. The take may still be flushing to disk.
    ClipRecorded(Option<i64>),
    /// The hook's batch stopped at a clip boundary after `hold` (#861).
    Held,
    /// The hook's batch went on after `go`.
    Going,
    /// OBS mode: OBS started recording a clip after the clip's first frame
    /// (#1109), so its take is missing the opening and isn't captured.
    ObsStartedLate(crate::obs::LateStart),
    /// hl.exe went away by itself mid-batch: closed (quit, ALT+F4, End
    /// Process) or crashed. Sent just before the `Error` that ends the batch,
    /// so the clip it cut short says the game closed.
    GameClosed,
}

/// The hook batch command that asks for `hold` (true) or `go` (false), #861.
pub fn hold_command(hold: bool) -> String {
    format!(
        "{} {}",
        crate::patch::runtime_batch::BATCH_COMMAND,
        if hold { "hold" } else { "go" }
    )
}

/// The `Error` text a batch ends with, around what failed.
fn abort_text(msg: &str) -> String {
    format!(
        "Capture Engine Aborted — {} (see View Logs for details)",
        msg
    )
}

/// Ends the batch on a failure: the activity log gets `msg`, the listener an
/// `Error`.
fn abort(tx: &Sender<EngineEvent>, msg: &str) {
    crate::log_markdown(msg);
    let _ = tx.send(EngineEvent::Error(abort_text(msg)));
}

/// Everything a batch is given, for its stages to share.
struct Batch {
    jobs: Vec<CaptureJob>,
    hl_path: Arc<PathBuf>,
    tx: Sender<EngineEvent>,
    cancel_token: Arc<AtomicBool>,
    config: crate::patch::PatcherConfig,
    drive_headroom: Vec<crate::patch::DriveHeadroom>,
    obs_take_folders: Vec<PathBuf>,
    runtime_batch: Option<crate::patch::runtime_batch::RuntimeBatch>,
    batch_start: Arc<crate::game_handoff::BatchStart>,
    hold: Arc<AtomicBool>,
}

pub fn spawn_capture_engine(
    jobs: Vec<CaptureJob>,
    hl_path: Arc<PathBuf>,
    tx: Sender<EngineEvent>,
    cancel_token: Arc<AtomicBool>,
    config: crate::patch::PatcherConfig,
    // Final per-drive headroom `build_batch_queue` already computed for every
    // drive this batch touches (see `native/src/patch/builder.rs`) — the
    // pre-launch check re-validates these same numbers instead of
    // re-querying disk space itself for just the primary export dir.
    drive_headroom: Vec<crate::patch::DriveHeadroom>,
    // Each planned block's take folder, in the order the batch will record
    // them — the same flattened order the capture manifest uses.
    //
    // Only read in `CaptureMode::Obs`, where the engine has to know where a
    // block's output belongs *before* the recording starts, so OBS can be
    // pointed straight at it. In every other mode HLAE decides that from
    // `mirv_movie_filename` and this is empty.
    obs_take_folders: Vec<PathBuf>,
    // The hook's batch file (`patch::runtime_batch`), when this batch runs on
    // the original demos from the hook DLL instead of patched copies (#434).
    // `jobs` is then empty: nothing is patched into `dod/`, the game starts on
    // no demo, and the file is handed to it once it takes commands.
    runtime_batch: Option<crate::patch::runtime_batch::RuntimeBatch>,
    // Where the batch's start in the game stands, read by Studio's close
    // prompt (#545). Studio sets it to `Preparing` when the batch starts.
    batch_start: Arc<crate::game_handoff::BatchStart>,
    // Render While Capturing (#861): true asks the hook's batch to stop at the
    // next clip boundary, false lets it go on. Sent over the game's pipe when
    // it changes; a patched batch can't hold, so it is ignored there.
    hold: Arc<AtomicBool>,
) {
    let batch = Batch {
        jobs,
        hl_path,
        tx,
        cancel_token,
        config,
        drive_headroom,
        obs_take_folders,
        runtime_batch,
        batch_start,
        hold,
    };
    std::thread::Builder::new()
        .name("capture_engine".into())
        .spawn(move || run_batch(batch))
        .unwrap();
}

/// The batch's stages in order; each returns at the first failure, which it
/// has already reported.
fn run_batch(batch: Batch) {
    let tx = &batch.tx;
    if tx.send(EngineEvent::Starting(batch.jobs.len())).is_err() {
        return;
    }
    let Some(hl_exe_parent) = batch.hl_path.parent() else {
        abort(tx, "Invalid hl.exe path: it has no parent folder");
        return;
    };
    let dod_dir = crate::shared::paths::mod_dir(hl_exe_parent);

    let Some(mut workspace) = workspace::Workspace::prepare(
        &batch.config,
        batch.runtime_batch.as_ref(),
        hl_exe_parent,
        tx,
    ) else {
        return;
    };
    let Some(demo_copies) = workspace::copy_demos_into_game(
        &batch.jobs,
        &dod_dir,
        batch.config.save_local_patched_copy,
        &batch.cancel_token,
        tx,
    ) else {
        return;
    };
    if demo_copies.is_empty() && batch.runtime_batch.is_none() {
        let _ = tx.send(EngineEvent::AllCompleted);
        return;
    }
    if tx
        .send(EngineEvent::Launching("Batch Queue".into()))
        .is_err()
    {
        abort(tx, "Failed to send Launching event (channel disconnected)");
        workspace::remove_demo_copies(&demo_copies);
        return;
    }
    if let Err(problem) = preflight::check_room(
        &batch.drive_headroom,
        &batch.jobs,
        batch.config.save_local_patched_copy,
        hl_exe_parent,
    ) {
        abort(tx, &problem);
        return;
    }

    let Some(mut feed) = start_marker_feed(&batch, &mut workspace.cleanup, hl_exe_parent) else {
        return;
    };
    let Some(game) = launch::start_game(&batch, &workspace.export_dir, &dod_dir, &demo_copies)
    else {
        return;
    };

    let drop_poller = feed.drop_poller.take();
    let mut watch = watch::BatchWatch::new(
        watch::WatchInputs {
            tx,
            cancel_token: &batch.cancel_token,
            hold: &batch.hold,
            batch_start: &batch.batch_start,
            obs_mode: feed.obs_mode,
            hook_runs_batch: batch.runtime_batch.is_some(),
            reuse_game: game.reuse_game,
            marker_rx: &feed.marker_rx,
            log_path: &feed.log_path,
            log_len_before_launch: feed.log_len_before_launch,
            done_marker: &game.dummy_path,
            exit_trigger: &workspace.exit_trigger,
        },
        feed.obs_session.take(),
        game.child,
        game.handoff,
        game.foreground_lock,
        game.foreground_locked_at,
    );
    watch.run();

    stop_markers(tx, &mut watch, &feed, drop_poller);
    let files = workspace::BatchFiles {
        demo_copies: &demo_copies,
        cfg_path: &game.cfg_path,
        dummy_path: &game.dummy_path,
        exit_trigger: &workspace.exit_trigger,
        session_junction: &workspace.session_junction,
        pool_junctions: &workspace.pool_junctions,
    };
    let cancelled = || batch.cancel_token.load(Ordering::Relaxed);
    send_batch_end(
        tx,
        batch_end(watch.failure_reason.take(), cancelled),
        &files,
    );
}

/// Where a batch's markers come from: the readers' channel and stop flag,
/// the console log they read, and OBS's session when it drives one.
struct MarkerFeed {
    obs_mode: bool,
    obs_session: Option<crate::obs::ObsSession>,
    /// Stops when dropped, so an early return can't leave it polling.
    drop_poller: Option<crate::obs::DropPoller>,
    marker_rx: Receiver<crate::obs::Marker>,
    tail_cancel: Arc<AtomicBool>,
    log_path: PathBuf,
    log_len_before_launch: u64,
}

/// Connects to OBS in OBS mode (before the game starts, see
/// `obs_capture::start`), then starts the marker readers. `None` once a
/// failure has been reported.
fn start_marker_feed(
    batch: &Batch,
    cleanup: &mut workspace::CaptureCleanupGuard,
    hl_exe_parent: &std::path::Path,
) -> Option<MarkerFeed> {
    let tx = &batch.tx;
    let obs_mode = batch.config.capture_mode == crate::patch::CaptureMode::Obs;
    let mut obs_session: Option<crate::obs::ObsSession> = None;
    let mut drop_poller: Option<crate::obs::DropPoller> = None;
    let (marker_tx, marker_rx) = std::sync::mpsc::channel::<crate::obs::Marker>();
    let tail_cancel = Arc::new(AtomicBool::new(false));
    // The log feeds the markers until the game's events pipe
    // connects, then the pipe does (#434); see `MarkerGate`.
    let marker_gate = Arc::new(crate::obs::MarkerGate::default());

    if obs_mode {
        match obs_capture::start(
            &batch.config,
            &batch.obs_take_folders,
            &batch.cancel_token,
            tx,
            cleanup,
        ) {
            Ok((session, poller)) => {
                obs_session = Some(session);
                drop_poller = poller;
            }
            Err(problem) => {
                abort(tx, &problem);
                return None;
            }
        }
    }

    let log_path = hl_exe_parent.join("qconsole.log");
    let log_len_before_launch = markers::console_log_len(&log_path);
    if let Err(e) = markers::start_readers(&log_path, marker_tx, &tail_cancel, &marker_gate) {
        abort(
            tx,
            &format!("could not start the console log reader: {}", e),
        );
        return None;
    }
    Some(MarkerFeed {
        obs_mode,
        obs_session,
        drop_poller,
        marker_rx,
        tail_cancel,
        log_path,
        log_len_before_launch,
    })
}

/// Markers the game ran before it closed or was ended, still on their way
/// when the loop stopped reading: a `STOP_RECORD` among them means the clip
/// finished, and must reach the listener before the Cancel or the failure
/// does (#1081). Then OBS stops recording and puts the scene back, before
/// the teardown starts removing junctions, and the readers stop.
fn stop_markers(
    tx: &Sender<EngineEvent>,
    watch: &mut watch::BatchWatch,
    feed: &MarkerFeed,
    drop_poller: Option<crate::obs::DropPoller>,
) {
    let late = markers::late_markers(&feed.marker_rx, markers::LATE_MARKERS);
    for event in late.iter().filter_map(markers::marker_event) {
        let _ = tx.send(event);
    }
    if watch.game_closed {
        let _ = tx.send(EngineEvent::GameClosed);
    }
    if let Some(session) = watch.obs_session.take() {
        obs_capture::finish(
            session,
            &late,
            &feed.marker_rx,
            &feed.tail_cancel,
            drop_poller,
            tx,
        );
    }
    feed.tail_cancel.store(true, Ordering::Relaxed);
}

/// Removes what the launched batch left and sends its last event: a failure
/// and a cancel also delete the demos copied into `dod/`.
fn send_batch_end(tx: &Sender<EngineEvent>, end: BatchEnd, files: &workspace::BatchFiles) {
    match end {
        BatchEnd::Failed(reason) => {
            abort(
                tx,
                &format!(
                    "{} — see [HLAE] lines above in this log for timing.",
                    reason
                ),
            );
            files.remove_with_demo_copies();
        }
        BatchEnd::Cancelled => {
            files.remove_with_demo_copies();
            let _ = tx.send(EngineEvent::Cancelled);
        }
        BatchEnd::Finished => {
            files.remove();
            let _ = tx.send(EngineEvent::Finished("Batch Queue".into()));
            let _ = tx.send(EngineEvent::AllCompleted);
        }
    }
}

/// How a launched batch ended, for its last event.
#[derive(Debug, PartialEq)]
enum BatchEnd {
    Failed(String),
    Cancelled,
    Finished,
}

/// A failure counts unless the user cancelled: a cancel ends the game,
/// which can look like a failure from out here.
fn batch_end(failure_reason: Option<String>, cancelled: impl Fn() -> bool) -> BatchEnd {
    match failure_reason.filter(|_| !cancelled()) {
        Some(reason) => BatchEnd::Failed(reason),
        None if cancelled() => BatchEnd::Cancelled,
        None => BatchEnd::Finished,
    }
}

#[cfg(all(test, windows))]
mod batch_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_and_go_use_the_hooks_batch_command() {
        assert_eq!(hold_command(true), "ds_capture_batch hold");
        assert_eq!(hold_command(false), "ds_capture_batch go");
    }

    #[test]
    fn an_abort_names_what_failed_and_points_at_the_log() {
        assert_eq!(
            abort_text("OBS capture could not start — refused"),
            "Capture Engine Aborted — OBS capture could not start — refused (see View Logs for details)"
        );
    }

    #[test]
    fn a_cancel_outranks_a_failure_and_a_clean_end_finishes() {
        assert_eq!(
            batch_end(Some("hl.exe ended".into()), || false),
            BatchEnd::Failed("hl.exe ended".into())
        );
        assert_eq!(
            batch_end(Some("hl.exe ended".into()), || true),
            BatchEnd::Cancelled
        );
        assert_eq!(batch_end(None, || true), BatchEnd::Cancelled);
        assert_eq!(batch_end(None, || false), BatchEnd::Finished);
    }
}
