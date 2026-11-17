//! The batch's poll loop: from the launch until the batch ends, is
//! cancelled, or fails. Each pass reads the game's markers, keeps the game
//! told what it needs (the start, OBS's go-ahead, a hold), and checks, in a
//! fixed order, every way the batch can end.
//!
//! The HLAE launcher process injects into `hl.exe` and then exits on its own
//! under `-noGui -autoStart` — that is expected handoff behaviour, not a
//! crash. The thing that actually matters is whether the batch's own
//! `hl.exe` is still alive, so liveness is tracked separately in the process
//! list (`game::BatchGame`, #1213) rather than via the launcher's own exit
//! status.

use super::EngineEvent;
use super::game::{BatchGame, end_game, kill_hl_exe_and_wait, wait_for_hl_exe_to_exit};
use super::markers::{RecordingGoAhead, console_log_len, marker_event};
use super::processes::Processes;
use crate::game_handoff::{BatchStart, GameHandoff, StartState};
use crate::log_markdown;
use crate::messages;
use crate::utils::duration_text::duration_text;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

/// Longest a batch may go without a console marker before OBS mode calls it
/// stalled.
///
/// Every other guard in the capture loop keys off hl.exe being gone or a file
/// appearing, so none of them notices the engine being alive and well but no
/// longer playing the demo: `disconnect` typed into the console, a demo that
/// failed to load, a frozen engine. On the frame-sequence path that wastes
/// time. In OBS mode it records until the disk fills, which is why the
/// watchdog lives here.
///
/// The floor has to clear the longest *legitimate* silence, which is one
/// breadcrumb interval of real-time playback. That is not a fixed number of
/// seconds — demo ticks are frames, so their wall-clock spacing depends on the
/// fps the demo was recorded at. And an unfocused game stops fast-forwarding
/// altogether (GoldSrc throttles without focus), so alt-tabbing stretches gaps
/// that are normally seconds into minutes. Five minutes clears both.
///
/// Tripping this kills the game and loses the batch, so it is deliberately
/// biased towards waiting too long over firing on a slow one.
const MARKER_STALL_FLOOR: Duration = Duration::from_secs(300);

/// How long the game may be up without a single marker before a batch
/// outside OBS mode is called never started (#1214): an old hook DLL, or a
/// demo the game can't open, leaves it at its menu with nothing to show.
///
/// The first marker (`DEMO_START`) comes once the first demo has loaded.
/// Measured over 46 batches in the hook DLL's and Studio's logs, 8 days of
/// October 2026, launch to first demo loaded: median 32 s, 90th percentile
/// 38 s, slowest 94 s (a demo that took 85 s to load). Four minutes is
/// 2.5 times the slowest, for a slower drive or a bigger demo.
const FIRST_MARKER_DEADLINE: Duration = Duration::from_secs(240);

/// How long `hl.exe` may be up before a `qconsole.log` that has not grown is
/// treated as the engine not writing it at all.
///
/// GoldSrc echoes startup output within a second or two of the process
/// appearing, so this only has to clear a slow load. Deliberately generous:
/// the check only logs, but a false positive would tell someone their capture
/// is broken when it is fine.
const CONDEBUG_WRITE_GRACE: Duration = Duration::from_secs(60);

/// How long the game is kept from taking the foreground at most, when the
/// batch never starts in it (no hook DLL, a patched batch): long enough for
/// its window to open, short enough that nothing else is held up for long.
const FOREGROUND_LOCK_MAX: Duration = Duration::from_secs(30);

/// How often the process list is re-read: OBS mode spins this loop every
/// 16 ms for its markers, and enumerating every process 62 times a second
/// for the whole batch buys nothing when a quarter-second-late "the game is
/// gone" is as good as an immediate one.
const PROCESS_CHECK_EVERY: Duration = Duration::from_millis(250);

/// How often error boxes are looked for.
const DIALOG_CHECK_EVERY: Duration = Duration::from_secs(1);

/// How long a hold or go that couldn't be sent waits before it is tried again.
const HOLD_RETRY_AFTER: Duration = Duration::from_secs(1);

/// How long to wait for the next marker, given the longest gap this batch has
/// already shown.
///
/// The adaptive term covers a demo whose breadcrumbs really are further apart
/// than the floor allows: a batch that has already demonstrated a four-minute
/// gap is not stalled at five.
fn marker_stall_deadline(longest_gap: Duration) -> Duration {
    std::cmp::max(MARKER_STALL_FLOOR, longest_gap * 3)
}

/// The pause between passes. 500 ms is fine for watching a process; it is
/// far too coarse for acting on stage markers, where the engine's own flush
/// granularity is ~21 ms and the pre-roll budget being spent is one second.
/// The process checks tolerate the faster cadence — `refresh_processes` is
/// the only real cost, and it is cheap next to a capture.
fn poll_interval(obs_mode: bool) -> Duration {
    Duration::from_millis(if obs_mode { 16 } else { 500 })
}

/// Whether a check last run at `last` (never, when `None`) is due again.
fn is_due(last: Option<Instant>, every: Duration) -> bool {
    last.is_none_or(|t| t.elapsed() >= every)
}

/// The two folders that say a batch is done: the done marker in the export
/// folder and the exit trigger beside hl.exe. Read only when a check gets
/// that far, as they always were.
trait DoneFolders {
    fn done_marker(&self) -> bool;
    fn exit_trigger(&self) -> bool;
}

struct OnDisk<'a> {
    done_marker: &'a Path,
    exit_trigger: &'a Path,
}

impl DoneFolders for OnDisk<'_> {
    fn done_marker(&self) -> bool {
        self.done_marker.exists()
    }
    fn exit_trigger(&self) -> bool {
        self.exit_trigger.exists()
    }
}

/// How the batch has said it is complete, from its markers.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct CompleteMarkers {
    /// Set the instant the BATCH_COMPLETE marker is seen, in OBS mode
    /// only — a faster, side-effect-free alternative to waiting on
    /// `exit_trigger` there. `sys_capture_done_path`'s
    /// `mirv_recordmovie_start`/`_stop` (which is what actually writes
    /// `exit_trigger`) restarts the whole demo from the top as a side
    /// effect — harmless in frame-sequence mode (which is already
    /// mid-movie-recording throughout), but in OBS mode it replayed
    /// the primer (when there is one) *and* dodstudio_chain_01.dem from tick 0, twice, adding ~50s to
    /// a real batch before `exit_trigger` ever got written. The
    /// BATCH_COMPLETE marker already exists and already fires
    /// `ObsSession::on_marker`'s `end_block()` moments before this —
    /// taskkilling right after it means hl.exe is dead before it ever
    /// gets to process the command that would restart it. See #obs.
    obs: bool,
    /// BATCH_COMPLETE straight from the game (#434): the same "done"
    /// the exit-trigger folder signals, a moment sooner and without the
    /// folder, in every capture mode. The folder stays the fallback.
    pipe: bool,
}

impl CompleteMarkers {
    fn on_marker(&mut self, marker: &crate::obs::Marker, obs_mode: bool) {
        if obs_mode && marker.kind == crate::obs::MarkerKind::BatchComplete {
            self.obs = true;
        }
        if marker.via_pipe && marker.kind == crate::obs::MarkerKind::BatchComplete {
            self.pipe = true;
        }
    }
}

/// When markers arrived, for the stall check.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct MarkerClock {
    /// Stays `None` until the first marker; outside OBS mode the stall
    /// check only asks whether one has come (#1214).
    last_marker_at: Option<Instant>,
    longest_gap: Duration,
}

impl MarkerClock {
    /// A marker arrived at `now`. A gap that spans a hold (#861) is
    /// waiting, not the pace of the batch, so it doesn't widen the stall
    /// deadline.
    fn on_marker(&mut self, now: Instant, holding: bool) {
        if let Some(previous) = self.last_marker_at
            && !holding
        {
            self.longest_gap = self.longest_gap.max(now - previous);
        }
        self.last_marker_at = Some(now);
    }

    /// A hold ended at `now`. A hold sends no markers for as long as it
    /// lasts: the stall check counts from the `go`, not from the last
    /// marker before the hold.
    fn on_go(&mut self, now: Instant) {
        self.last_marker_at = Some(now);
    }
}

/// A batch that stopped advancing while hl.exe still runs.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stall {
    since: Instant,
    /// No marker ever arrived: the demo most likely never loaded.
    never_started: bool,
}

/// Whether the batch has stalled: the game is alive and the batch is not
/// advancing. A batch held for renders (#861) sends no markers on purpose.
///
/// OBS mode counts from the last marker, or from hl.exe first appearing when
/// none has arrived yet, a demo that never started playing. Every other mode
/// checks only that second case, against [`FIRST_MARKER_DEADLINE`] (#1214):
/// there a stall mid-batch wastes time but fills no drive, while a demo that
/// never starts leaves the game at its menu until someone presses Cancel.
fn stalled(
    obs_mode: bool,
    holding: bool,
    clock: &MarkerClock,
    hl_first_seen: Option<Instant>,
    hl_alive: bool,
) -> Option<Stall> {
    if holding || !hl_alive {
        return None;
    }
    let (since, deadline) = match (obs_mode, clock.last_marker_at) {
        (true, last) => (
            last.or(hl_first_seen)?,
            marker_stall_deadline(clock.longest_gap),
        ),
        (false, None) => (hl_first_seen?, FIRST_MARKER_DEADLINE),
        (false, Some(_)) => return None,
    };
    (since.elapsed() > deadline).then_some(Stall {
        since,
        never_started: clock.last_marker_at.is_none(),
    })
}

/// The stall's log line and the reason the batch fails with.
fn stall_texts(stalled_for: Duration, never_started: bool) -> (String, &'static str) {
    (
        messages::stall_log_line(duration_text(stalled_for.as_secs_f64()), never_started),
        messages::stall_reason(never_started),
    )
}

/// The reason a batch ended by an error box fails with. Every box the game
/// shows is modal: its frame loop stops until someone answers it, and the
/// known ones (Sys_Error's "Fatal Error", a CRT assert) end the process once
/// answered. So even a game the player had open (#725) is closed, unlike on
/// Cancel, and the message says so: they didn't close it (#1231).
fn dialog_failure_reason(summary: &str, borrowed_game: bool) -> String {
    if borrowed_game {
        messages::error_box_closed_borrowed_game(summary)
    } else {
        summary.to_string()
    }
}

/// Whether the batch is complete: past its first 10 s, by a done folder or a
/// BATCH_COMPLETE marker.
fn batch_complete(elapsed: Duration, markers: CompleteMarkers, folders: &impl DoneFolders) -> bool {
    elapsed.as_secs() > 10
        && (folders.done_marker() || folders.exit_trigger() || markers.obs || markers.pipe)
}

/// How the end of a batch was noticed, for the "Batch complete" log line.
///
/// The events pipe's `BATCH_COMPLETE` is named whenever it arrived (#434).
/// Outside OBS mode the capture loop checks only every 500 ms, and the
/// exit-trigger folder appears a few frames after the marker, so both are
/// usually there by the same check; naming the folder then hid that the pipe
/// had already delivered.
fn completion_source(pipe_seen: bool, obs_seen: bool, dummy: bool, trigger: bool) -> String {
    if pipe_seen {
        messages::COMPLETION_VIA_PIPE.to_string()
    } else if obs_seen && !trigger {
        // The common OBS-mode case: caught it off the marker, ahead of
        // exit_trigger ever needing to be written.
        messages::COMPLETION_VIA_OBS_MARKER.to_string()
    } else {
        messages::completion_via_folders(dummy, trigger)
    }
}

/// Only treat this as a real failure once the launcher has handed off (or
/// failed to) AND hl.exe itself is confirmed not running AND no exit
/// trigger has appeared — i.e. nothing is left that could ever finish the
/// batch. The 5s grace period covers the brief window between the launcher
/// exiting and hl.exe's own process becoming visible to sysinfo.
fn game_never_started(
    launcher_exited: bool,
    hl_seen_alive: bool,
    elapsed: Duration,
    folders: &impl DoneFolders,
) -> bool {
    launcher_exited
        && !hl_seen_alive
        && elapsed.as_secs() > 5
        && !folders.done_marker()
        && !folders.exit_trigger()
}

/// hl.exe was running and has now disappeared without ever writing an exit
/// trigger/done marker, unlike the normal quit-cfg-driven exit which writes
/// the trigger before the process goes away.
///
/// Not necessarily a crash, and it used to say it was: `quit` in the
/// console, ALT+F4 and End Process all land here too, and all of them are
/// the user closing the game deliberately. Nothing visible from out here
/// separates them from an access violation — the exit status belongs to the
/// launcher, which handed off long ago — so the wording covers both rather
/// than guessing.
fn game_went_away(
    hl_seen_alive: bool,
    hl_alive: bool,
    markers: CompleteMarkers,
    folders: &impl DoneFolders,
) -> bool {
    hl_seen_alive && !hl_alive && !folders.done_marker() && !folders.exit_trigger() && !markers.pipe
}

/// Whether the hold the user wants still has to reach the game: only a
/// batch the hook runs can hold, and a send that failed waits
/// [`HOLD_RETRY_AFTER`] before the next try.
fn hold_send_due(
    hook_runs_batch: bool,
    want_hold: bool,
    hold_sent: bool,
    hold_retry_at: Option<Instant>,
) -> bool {
    hook_runs_batch && want_hold != hold_sent && is_due(hold_retry_at, HOLD_RETRY_AFTER)
}

/// What the loop is given, besides the game's processes it finds itself.
pub(super) struct WatchInputs<'a> {
    pub(super) tx: &'a Sender<EngineEvent>,
    pub(super) cancel_token: &'a AtomicBool,
    pub(super) hold: &'a AtomicBool,
    pub(super) batch_start: &'a BatchStart,
    pub(super) obs_mode: bool,
    /// The hook runs the batch (#434): the only kind that can hold (#861).
    pub(super) hook_runs_batch: bool,
    pub(super) reuse_game: Option<u32>,
    pub(super) marker_rx: &'a Receiver<crate::obs::Marker>,
    pub(super) log_path: &'a Path,
    /// Baseline for the console-log write check. `-condebug` being on the
    /// command line is not proof the log is being written, and everything
    /// marker-driven degrades in silence when it is not — the one failure
    /// mode this path cannot afford.
    ///
    /// The engine *appends* across launches rather than starting a fresh
    /// log — measured, not assumed: one 1.5 MB `qconsole.log` here held 100
    /// startup banners. `auto_clear_logs` deleting it at the end of a batch
    /// is the only thing that ever shortens it.
    ///
    /// So the test is "did the size change at all" rather than "did it
    /// grow". Under appending the two agree, but `<=` would also report a
    /// file that got deleted mid-run as a failure to write, which it is not.
    pub(super) log_len_before_launch: u64,
    pub(super) done_marker: &'a Path,
    pub(super) exit_trigger: &'a Path,
}

/// The loop's state, from the launch to the end of the batch.
pub(super) struct BatchWatch<'a> {
    io: WatchInputs<'a>,
    pub(super) obs_session: Option<crate::obs::ObsSession>,
    child: Option<crate::sys::launcher::Launcher>,
    handoff: Option<GameHandoff>,
    foreground_lock: crate::sys::foreground_lock::ForegroundLock,
    foreground_locked_at: Instant,
    start_time: Instant,
    launcher_exit_logged: bool,
    hl_seen_alive: bool,
    /// Armed from hl.exe coming up, not from the first marker. A batch
    /// where markers never start at all — a demo that fails to load, a
    /// game sitting at the menu — would otherwise never be watched,
    /// because the deadline had nothing to count from. Seen in a real
    /// run: 398s with hl.exe alive and not one marker.
    hl_first_seen: Option<Instant>,
    /// Whether hl.exe is up on this pass.
    hl_alive: bool,
    hl_alive_cached: bool,
    clock: MarkerClock,
    complete: CompleteMarkers,
    /// One-shot, so a batch cannot spam the log with it.
    condebug_write_checked: bool,
    /// The process list and kills (`processes::Live` outside tests).
    procs: Box<dyn Processes>,
    /// Which hl.exe in it is the batch's.
    game: BatchGame,
    last_process_check: Option<Instant>,
    last_dialog_check: Option<Instant>,
    /// What the game was last told (#861), and when a send last failed.
    hold_sent: bool,
    hold_retry_at: Option<Instant>,
    go_ahead: RecordingGoAhead,
    /// Why the batch failed, when it did.
    pub(super) failure_reason: Option<String>,
    /// hl.exe went away by itself mid-batch (closed or crashed).
    pub(super) game_closed: bool,
}

impl<'a> BatchWatch<'a> {
    pub(super) fn new(
        io: WatchInputs<'a>,
        obs_session: Option<crate::obs::ObsSession>,
        child: Option<crate::sys::launcher::Launcher>,
        game: BatchGame,
        handoff: Option<GameHandoff>,
        foreground_lock: crate::sys::foreground_lock::ForegroundLock,
        foreground_locked_at: Instant,
    ) -> Self {
        Self {
            io,
            obs_session,
            launcher_exit_logged: child.is_none(),
            child,
            handoff,
            foreground_lock,
            foreground_locked_at,
            start_time: Instant::now(),
            hl_seen_alive: false,
            hl_first_seen: None,
            hl_alive: false,
            hl_alive_cached: false,
            clock: MarkerClock::default(),
            complete: CompleteMarkers::default(),
            condebug_write_checked: false,
            procs: Box::new(super::processes::Live::new()),
            game,
            last_process_check: None,
            last_dialog_check: None,
            hold_sent: false,
            hold_retry_at: None,
            go_ahead: RecordingGoAhead::default(),
            failure_reason: None,
            game_closed: false,
        }
    }

    /// Runs passes until one ends the batch.
    pub(super) fn run(&mut self) {
        while self.pass().is_continue() {
            std::thread::sleep(poll_interval(self.io.obs_mode));
        }
    }

    /// One pass, in the order the batch has always been checked.
    fn pass(&mut self) -> ControlFlow<()> {
        self.drain_markers();
        self.send_recording_go_ahead();
        self.send_hold();
        self.note_launcher_exit();
        self.note_game_alive();
        if self.foreground_locked_at.elapsed() > FOREGROUND_LOCK_MAX {
            self.foreground_lock.release();
        }
        self.poll_handoff()?;
        self.check_console_log_written();
        self.end_on_error_dialog()?;
        self.end_on_cancel()?;
        self.end_on_stall()?;
        self.end_on_obs_gone()?;
        self.end_on_complete()?;
        self.end_if_game_never_started()?;
        self.end_if_game_went_away()
    }

    fn elapsed_text(&self) -> String {
        duration_text(self.start_time.elapsed().as_secs_f64())
    }

    fn folders(&self) -> OnDisk<'a> {
        OnDisk {
            done_marker: self.io.done_marker,
            exit_trigger: self.io.exit_trigger,
        }
    }

    /// Drain whatever the engine has echoed since the last pass.
    /// This is the whole synchronisation mechanism: markers reach
    /// the log 21-40 ms after the tick that emitted them, measured
    /// over a 17-block batch, so a 500 ms poll would be far
    /// too coarse to act on them — hence the shorter sleep chosen
    /// for OBS mode (`poll_interval`).
    fn drain_markers(&mut self) {
        for marker in self.io.marker_rx.try_iter() {
            self.clock.on_marker(Instant::now(), self.hold_sent);
            if let Some(session) = self.obs_session.as_mut() {
                if let Some(late) = session.on_marker(&marker) {
                    let _ = self.io.tx.send(EngineEvent::ObsStartedLate(late));
                }
                self.go_ahead.on_marker(&marker);
            }
            self.complete.on_marker(&marker, self.io.obs_mode);
            if let Some(event) = marker_event(&marker) {
                let _ = self.io.tx.send(event);
            }
        }
    }

    /// OBS is recording the clip the game waits on (#1109): let it
    /// play. A failed send tries again on the next pass.
    fn send_recording_go_ahead(&mut self) {
        if !self.go_ahead.owed() {
            return;
        }
        let sent = self.game.pid_in(&*self.procs).map(|pid| {
            crate::sys::game_remote::send_console_commands(
                pid,
                &[crate::patch::runtime_batch::RECORDING_GO.to_string()],
            )
        });
        if let Some(Ok(crate::sys::game_remote::Sent::Delivered)) = sent
            && let Some(waited) = self.go_ahead.sent()
        {
            log_markdown(&messages::obs_recording_game_waited(waited.as_millis()));
        }
    }

    /// Render While Capturing (#861): tell the hook's batch when the
    /// hold changes. A failed send tries again a second later.
    fn send_hold(&mut self) {
        let want_hold = self.io.hold.load(Ordering::Relaxed);
        if !hold_send_due(
            self.io.hook_runs_batch,
            want_hold,
            self.hold_sent,
            self.hold_retry_at,
        ) {
            return;
        }
        let sent = self.game.pid_in(&*self.procs).map(|pid| {
            crate::sys::game_remote::send_console_commands(pid, &[super::hold_command(want_hold)])
        });
        match sent {
            Some(Ok(crate::sys::game_remote::Sent::Delivered)) => {
                if self.hold_sent && !want_hold {
                    self.clock.on_go(Instant::now());
                }
                self.hold_sent = want_hold;
                self.hold_retry_at = None;
                log_markdown(&messages::hold_sent(super::hold_command(want_hold)));
            }
            other => {
                self.hold_retry_at = Some(Instant::now());
                log_markdown(&messages::hold_send_failed(
                    super::hold_command(want_hold),
                    other,
                ));
            }
        }
    }

    fn note_launcher_exit(&mut self) {
        if self.launcher_exit_logged {
            return;
        }
        let Some(child) = self.child.as_mut() else {
            return;
        };
        match self.procs.launcher_status(child) {
            Ok(Some(status)) => {
                self.launcher_exit_logged = true;
                log_markdown(&messages::launcher_exited(
                    duration_text(self.start_time.elapsed().as_secs_f64()),
                    status,
                ));
            }
            Ok(None) => {}
            Err(e) => log_markdown(&messages::launcher_try_wait_failed(e)),
        }
    }

    /// Re-reads the process list at most every [`PROCESS_CHECK_EVERY`]. Only
    /// the batch's own game counts as the game being up (#1213).
    fn note_game_alive(&mut self) {
        if is_due(self.last_process_check, PROCESS_CHECK_EVERY) {
            self.last_process_check = Some(Instant::now());
            self.procs.refresh();
            self.hl_alive_cached = self.game.pid_in(&*self.procs).is_some();
        }
        self.hl_alive = self.hl_alive_cached;
        if self.hl_alive {
            self.hl_seen_alive = true;
            self.hl_first_seen.get_or_insert_with(Instant::now);
        }
    }

    /// Starts the batch in the game once it takes commands.
    fn poll_handoff(&mut self) -> ControlFlow<()> {
        use crate::game_handoff::Handoff;
        let Some(handoff) = self.handoff.as_mut() else {
            return ControlFlow::Continue(());
        };
        let own: Vec<u32> = self.game.pid_in(&*self.procs).into_iter().collect();
        match handoff.poll(&own) {
            Handoff::Waiting | Handoff::Sent => {}
            Handoff::JustSent(pid) => {
                self.foreground_lock.release();
                self.io.batch_start.set(StartState::Sent);
                log_markdown(&messages::game_up_batch_started(
                    pid,
                    handoff.lines.join("; "),
                ))
            }
            Handoff::Failed(why) => {
                self.io.batch_start.set(StartState::Failed);
                log_markdown(&messages::handoff_failed(&why));
                self.failure_reason = Some(why);
                // An open game the batch never reached stays as it was.
                if self.io.reuse_game.is_none() {
                    kill_hl_exe_and_wait(&mut *self.procs, &mut self.game);
                }
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }

    /// Is the engine actually writing the console log? Checked once,
    /// after the game has had long enough to produce startup output.
    ///
    /// Nothing else notices this. The tailer runs happily against a
    /// file that never grows, no marker ever arrives, and every
    /// feature that depends on one — per-demo progress, fast-forward
    /// notifications, crash detection, and all of OBS capture —
    /// simply never fires. The batch still finishes, so it looks like
    /// success. Say so loudly instead.
    fn check_console_log_written(&mut self) {
        if !self.condebug_write_checked
            && let Some(seen) = self.hl_first_seen
            && seen.elapsed() >= CONDEBUG_WRITE_GRACE
        {
            self.condebug_write_checked = true;
            if console_log_len(self.io.log_path) == self.io.log_len_before_launch {
                log_markdown(&console_log_not_written_text(self.io.log_path));
            }
        }
    }

    /// An error box from the game or HLAE stops everything until
    /// someone clicks it, and nothing about it reaches Studio. Read
    /// it (never click it), say what it means, and end the batch the
    /// way a failure does. Only processes this batch started: the
    /// game, the launcher, and the launcher's own injector.
    fn end_on_error_dialog(&mut self) -> ControlFlow<()> {
        if !is_due(self.last_dialog_check, DIALOG_CHECK_EVERY) {
            return ControlFlow::Continue(());
        }
        self.last_dialog_check = Some(Instant::now());
        let launcher_pid = self.child.as_ref().map(crate::sys::launcher::Launcher::id);
        let game_pids: Vec<u32> = self.game.pid_in(&*self.procs).into_iter().collect();
        let mut launcher_pids = launcher_pid
            .map(|pid| self.procs.injectors_of(pid))
            .unwrap_or_default();
        if let Some(pid) = launcher_pid.filter(|_| !self.launcher_exit_logged) {
            launcher_pids.push(pid);
        }
        let Some(dialog) = crate::sys::dialogs::error_dialogs(&game_pids, &launcher_pids)
            .into_iter()
            .next()
        else {
            return ControlFlow::Continue(());
        };
        log_markdown(&messages::error_box_seen(
            dialog.pid,
            self.elapsed_text(),
            dialog.full_text(),
        ));
        self.failure_reason = Some(dialog_failure_reason(
            &dialog.summary(),
            self.io.reuse_game.is_some(),
        ));
        if self.io.reuse_game.is_some() {
            log_markdown(messages::BORROWED_GAME_CLOSED_LOG);
        }
        for pid in launcher_pids
            .iter()
            .filter(|&&pid| Some(pid) != launcher_pid)
        {
            self.procs.kill_pid(*pid);
        }
        if !self.launcher_exit_logged
            && let Some(child) = self.child.as_mut()
        {
            child.kill();
        }
        kill_hl_exe_and_wait(&mut *self.procs, &mut self.game);
        // A game already exiting when its box came up (the
        // Anniversary !m_bMounted assert, after the user
        // clicked the Steam box themselves) can't be killed:
        // Windows won't terminate a terminating process. Its
        // box is answered instead, so it doesn't sit there.
        let left = crate::sys::dialogs::error_dialogs(&game_pids, &[]);
        if !left.is_empty() {
            for dialog in &left {
                log_markdown(&messages::error_box_answered_ignore(dialog.full_text()));
                crate::sys::dialogs::dismiss(dialog);
            }
            wait_for_hl_exe_to_exit(&mut *self.procs, &self.game);
        }
        ControlFlow::Break(())
    }

    fn end_on_cancel(&mut self) -> ControlFlow<()> {
        if !self.io.cancel_token.load(Ordering::Relaxed) {
            return ControlFlow::Continue(());
        }
        log_markdown(&messages::cancelled_by_user(self.elapsed_text()));
        // A cancel in the first few seconds catches the launcher
        // still injecting into hl.exe. Killing only hl.exe then
        // leaves HLAE open on "AfxHook error, Code: 1" (seen
        // 2026-09-29, a cancel 1.0s in), so the launcher goes first.
        if !self.launcher_exit_logged
            && let Some(child) = self.child.as_mut()
        {
            child.kill();
            log_markdown(messages::LAUNCHER_CLOSED_TOO);
        }
        end_game(self.io.reuse_game, &mut *self.procs, &mut self.game);
        ControlFlow::Break(())
    }

    fn end_on_stall(&mut self) -> ControlFlow<()> {
        let Some(stall) = stalled(
            self.io.obs_mode,
            self.hold_sent,
            &self.clock,
            self.hl_first_seen,
            self.hl_alive,
        ) else {
            return ControlFlow::Continue(());
        };
        // A game that isn't writing its console log sends no markers even
        // while it plays, so outside OBS mode, where markers only report
        // progress, their absence proves nothing; `check_console_log_written`
        // has said so in the log already.
        if !self.io.obs_mode && console_log_len(self.io.log_path) == self.io.log_len_before_launch {
            return ControlFlow::Continue(());
        }
        let (line, reason) = stall_texts(stall.since.elapsed(), stall.never_started);
        log_markdown(&line);
        self.failure_reason = Some(reason.into());
        // A borrowed game (#725) is left open at its menu, as on Cancel.
        end_game(self.io.reuse_game, &mut *self.procs, &mut self.game);
        ControlFlow::Break(())
    }

    /// OBS gone for good, after a reconnect was already tried, or
    /// refusing to record (`ObsSession::refused`). Continuing would
    /// play the demo to the end capturing nothing and then report
    /// the batch as finished.
    fn end_on_obs_gone(&mut self) -> ControlFlow<()> {
        let Some(session) = self.obs_session.as_ref().filter(|s| s.is_dead()) else {
            return ControlFlow::Continue(());
        };
        if let Some(why) = session.refused() {
            log_markdown(&messages::obs_refused_abort(why));
            self.failure_reason = Some(why.to_string());
        } else {
            log_markdown(messages::OBS_UNREACHABLE_LOG);
            self.failure_reason = Some(messages::OBS_LOST_REASON.into());
        }
        // The game itself is fine: a borrowed one (#725) is left open at its
        // menu, as on Cancel (#1231).
        end_game(self.io.reuse_game, &mut *self.procs, &mut self.game);
        ControlFlow::Break(())
    }

    fn end_on_complete(&mut self) -> ControlFlow<()> {
        let folders = self.folders();
        if !batch_complete(self.start_time.elapsed(), self.complete, &folders) {
            return ControlFlow::Continue(());
        }
        let via = completion_source(
            self.complete.pipe,
            self.complete.obs,
            folders.done_marker(),
            folders.exit_trigger(),
        );
        log_markdown(&messages::batch_complete_log(
            self.elapsed_text(),
            via,
            self.io.reuse_game.is_some(),
        ));
        end_game(self.io.reuse_game, &mut *self.procs, &mut self.game);
        ControlFlow::Break(())
    }

    fn end_if_game_never_started(&mut self) -> ControlFlow<()> {
        if !game_never_started(
            self.launcher_exit_logged,
            self.hl_seen_alive,
            self.start_time.elapsed(),
            &self.folders(),
        ) {
            return ControlFlow::Continue(());
        }
        log_markdown(&messages::game_never_started_log(self.elapsed_text()));
        self.failure_reason = Some(messages::GAME_NEVER_STARTED_REASON.into());
        ControlFlow::Break(())
    }

    fn end_if_game_went_away(&mut self) -> ControlFlow<()> {
        if !game_went_away(
            self.hl_seen_alive,
            self.hl_alive,
            self.complete,
            &self.folders(),
        ) {
            return ControlFlow::Continue(());
        }
        log_markdown(&messages::game_went_away_log(self.elapsed_text()));
        // The hook DLL's crash recorder says whether it was a
        // crash, where, and on which map; a known one is
        // remembered so later batches on that map are warned
        // (#207).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let batch_started = std::time::SystemTime::now()
            .checked_sub(self.start_time.elapsed())
            .unwrap_or(std::time::UNIX_EPOCH);
        let explained = crate::crash_maps::explain_and_record(
            &crate::activity_log_dir(),
            &crate::shared::paths::get_appdata_dir(),
            batch_started,
            now,
        );
        self.failure_reason = Some(match explained {
            Some(crash) => {
                log_markdown(&messages::game_crash_log(&crash));
                crash
            }
            None => messages::GAME_ENDED_EARLY_REASON.into(),
        });
        self.game_closed = true;
        ControlFlow::Break(())
    }
}

/// The loud warning for a console log the engine isn't writing.
fn console_log_not_written_text(log_path: &Path) -> String {
    messages::console_log_not_written(
        log_path.display(),
        duration_text(CONDEBUG_WRITE_GRACE.as_secs_f64()),
    )
}

#[cfg(test)]
#[path = "watch_pass_tests.rs"]
mod pass_tests;

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
