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
    /// check that reads it never runs.
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

/// Whether OBS mode's batch has stalled. Counts from the last marker, or from
/// hl.exe first appearing when none has arrived yet — the second case is a
/// demo that never started playing, which produces no markers to count from
/// at all. Both mean the same thing: the game is alive and the batch is not
/// advancing. A batch held for renders (#861) sends no markers on purpose.
fn stalled(
    obs_mode: bool,
    holding: bool,
    clock: &MarkerClock,
    hl_first_seen: Option<Instant>,
    hl_alive: bool,
) -> Option<Stall> {
    if obs_mode
        && !holding
        && let Some(since) = clock.last_marker_at.or(hl_first_seen)
        && hl_alive
        && since.elapsed() > marker_stall_deadline(clock.longest_gap)
    {
        return Some(Stall {
            since,
            never_started: clock.last_marker_at.is_none(),
        });
    }
    None
}

/// The stall's log line and the reason the batch fails with.
fn stall_texts(stalled_for: Duration, never_started: bool) -> (String, &'static str) {
    let line = format!(
        "[HLAE] No console markers for {} with hl.exe still running — treating the batch as stalled. {}",
        duration_text(stalled_for.as_secs_f64()),
        if never_started {
            "Playback never produced a single marker, so the demo most likely failed to load."
        } else {
            "`disconnect` typed in the console, a demo that ended early, and a frozen engine all look like this from out here."
        }
    );
    let reason = if never_started {
        "the demo never started playing — hl.exe came up but produced no console markers at all"
    } else {
        "the batch stopped progressing while hl.exe was still running — no console markers arrived for several minutes"
    };
    (line, reason)
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
        "BATCH_COMPLETE from the game events pipe".to_string()
    } else if obs_seen && !trigger {
        // The common OBS-mode case: caught it off the marker, ahead of
        // exit_trigger ever needing to be written.
        "BATCH_COMPLETE marker".to_string()
    } else {
        format!("done marker: {dummy}, exit trigger: {trigger}")
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
            log_markdown(&format!(
                "[OBS] Recording; the game waited {} ms for it and plays on",
                waited.as_millis()
            ));
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
                log_markdown(&format!("[HLAE] Sent `{}`", super::hold_command(want_hold)));
            }
            other => {
                self.hold_retry_at = Some(Instant::now());
                log_markdown(&format!(
                    "[HLAE] Could not send `{}` yet ({other:?}); trying again",
                    super::hold_command(want_hold)
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
                log_markdown(&format!(
                    "[HLAE] Launcher exited after {} (status: {}) — expected handoff behaviour, not a crash by itself",
                    duration_text(self.start_time.elapsed().as_secs_f64()),
                    status
                ));
            }
            Ok(None) => {}
            Err(e) => log_markdown(&format!("[HLAE] try_wait() failed: {}", e)),
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
                log_markdown(&format!(
                    "[HLAE] The game is up (PID {pid}); started the batch: `{}`",
                    handoff.lines.join("; ")
                ))
            }
            Handoff::Failed(why) => {
                self.io.batch_start.set(StartState::Failed);
                log_markdown(&format!("[HLAE] {why}"));
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
        log_markdown(&format!(
            "[HLAE] Error box from PID {} after {}: {}",
            dialog.pid,
            self.elapsed_text(),
            dialog.full_text()
        ));
        self.failure_reason = Some(dialog.summary());
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
                log_markdown(&format!(
                    "[HLAE] The game was already exiting and couldn't be ended; answered its box with Ignore: {}",
                    dialog.full_text()
                ));
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
        log_markdown(&format!(
            "[HLAE] Cancelled by user after {}",
            self.elapsed_text()
        ));
        // A cancel in the first few seconds catches the launcher
        // still injecting into hl.exe. Killing only hl.exe then
        // leaves HLAE open on "AfxHook error, Code: 1" (seen
        // 2026-09-29, a cancel 1.0s in), so the launcher goes first.
        if !self.launcher_exit_logged
            && let Some(child) = self.child.as_mut()
        {
            child.kill();
            log_markdown("[HLAE] Closed the launcher too: it had not handed off to hl.exe yet");
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
        let (line, reason) = stall_texts(stall.since.elapsed(), stall.never_started);
        log_markdown(&line);
        self.failure_reason = Some(reason.into());
        kill_hl_exe_and_wait(&mut *self.procs, &mut self.game);
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
            log_markdown(&format!(
                "[HLAE] {why} — aborting rather than finishing the batch with nothing recorded."
            ));
            self.failure_reason = Some(why.to_string());
        } else {
            log_markdown(
                "[HLAE] OBS is unreachable and could not be reconnected — aborting rather than finishing the batch with nothing recorded.",
            );
            self.failure_reason =
                Some("lost contact with OBS mid-batch and could not reconnect".into());
        }
        kill_hl_exe_and_wait(&mut *self.procs, &mut self.game);
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
        log_markdown(&format!(
            "[HLAE] Batch complete after {} (via {}) — {}",
            self.elapsed_text(),
            via,
            if self.io.reuse_game.is_some() {
                "leaving the game open"
            } else {
                "taskkilling hl.exe"
            }
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
        log_markdown(&format!(
            "[HLAE] hl.exe never came up after the launcher exited ({} elapsed) — treating as failure",
            self.elapsed_text()
        ));
        self.failure_reason = Some("hl.exe never started after the HLAE launcher exited. Check that Steam is running and signed in to an account that owns Day of Defeat".into());
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
        log_markdown(&format!(
            "[HLAE] hl.exe is gone with no exit trigger after {} — either the game was closed (quit / ALT+F4 / End Process) or it crashed",
            self.elapsed_text()
        ));
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
                log_markdown(&format!("[HLAE] {crash}"));
                crash
            }
            None => "hl.exe ended before the batch finished — the game was either closed manually or crashed (no exit trigger was written)".into(),
        });
        self.game_closed = true;
        ControlFlow::Break(())
    }
}

/// The loud warning for a console log the engine isn't writing.
fn console_log_not_written_text(log_path: &Path) -> String {
    format!(
        "⚠️ **The engine is not writing its console log.** `{}` is unchanged \
         {} after the game started, so dod-studio is getting no console \
         markers from it.\n\n\
         Per-demo progress, fast-forward-to-clip notifications, crash \
         detection and OBS capture all read that log — without it they do \
         not fail, they simply never fire, and this batch may finish looking \
         like it worked.\n\n\
         `-condebug` is passed on every launch and this is the folder the \
         engine writes to, so the cause is on the writing side: the folder \
         may not be writable (antivirus and Controlled Folder Access can \
         block it as well as permissions), something else may be holding \
         the file open, or HLAE may not be passing the flag through to the \
         game.",
        log_path.display(),
        duration_text(CONDEBUG_WRITE_GRACE.as_secs_f64()),
    )
}

#[cfg(test)]
#[path = "watch_pass_tests.rs"]
mod pass_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obs::log_tail::parse_marker;
    use std::cell::Cell;

    /// Done folders whose answers are set by the test, counting each read.
    #[derive(Default)]
    struct Folders {
        done: bool,
        trigger: bool,
        reads: Cell<u32>,
    }

    impl DoneFolders for Folders {
        fn done_marker(&self) -> bool {
            self.reads.set(self.reads.get() + 1);
            self.done
        }
        fn exit_trigger(&self) -> bool {
            self.reads.set(self.reads.get() + 1);
            self.trigger
        }
    }

    fn ago(secs: u64) -> Instant {
        Instant::now() - Duration::from_secs(secs)
    }

    /// A batch that has shown nothing unusual gets the floor. Five minutes is
    /// the number that has to clear an unfocused game's stalled fast-forward.
    #[test]
    fn quiet_batch_waits_the_floor() {
        assert_eq!(marker_stall_deadline(Duration::ZERO), MARKER_STALL_FLOOR);
        assert_eq!(
            marker_stall_deadline(Duration::from_secs(30)),
            MARKER_STALL_FLOOR
        );
    }

    /// A demo whose breadcrumbs are genuinely further apart than the floor
    /// must not be killed for being slow — it has already demonstrated the gap
    /// is legitimate.
    #[test]
    fn adapts_to_a_batch_with_long_legitimate_gaps() {
        let observed = Duration::from_secs(240);
        assert_eq!(
            marker_stall_deadline(observed),
            Duration::from_secs(720),
            "three times the longest gap already survived"
        );
    }

    /// The adaptive term only ever widens the window. Narrowing it would make
    /// a fast batch trip on its first slow stretch.
    #[test]
    fn never_shortens_below_the_floor() {
        for secs in [0, 1, 10, 99, 100, 101] {
            assert!(marker_stall_deadline(Duration::from_secs(secs)) >= MARKER_STALL_FLOOR);
        }
    }

    #[test]
    fn a_batch_end_names_the_pipe_whenever_its_marker_arrived() {
        // Marker and exit-trigger folder both there by the same check.
        assert_eq!(
            completion_source(true, false, false, true),
            "BATCH_COMPLETE from the game events pipe"
        );
        assert_eq!(
            completion_source(false, true, false, false),
            "BATCH_COMPLETE marker"
        );
        // No pipe (an old hook DLL, or GOLDSRC_HOOKS_EVENTS=0): the folder.
        assert_eq!(
            completion_source(false, false, false, true),
            "done marker: false, exit trigger: true"
        );
    }

    #[test]
    fn obs_mode_polls_every_16_ms_and_the_rest_every_half_second() {
        assert_eq!(poll_interval(true), Duration::from_millis(16));
        assert_eq!(poll_interval(false), Duration::from_millis(500));
    }

    #[test]
    fn a_check_is_due_the_first_time_and_then_once_its_interval_passed() {
        let every = Duration::from_secs(1);
        assert!(is_due(None, every));
        assert!(!is_due(Some(Instant::now()), every));
        assert!(is_due(Some(ago(1)), every));
    }

    #[test]
    fn only_batch_complete_markers_say_the_batch_is_done() {
        let marker = |line: &str, via_pipe: bool| {
            let mut m = parse_marker(line).unwrap();
            m.via_pipe = via_pipe;
            m
        };
        let done_log = marker("[dod-studio] BATCH_COMPLETE", false);
        let done_pipe = marker("[dod-studio] BATCH_COMPLETE", true);
        let other = marker("[dod-studio] STOP_RECORD - Tick 5", true);

        let mut seen = CompleteMarkers::default();
        seen.on_marker(&other, true);
        seen.on_marker(&done_log, false);
        assert_eq!(
            seen,
            CompleteMarkers::default(),
            "the log's, outside OBS mode"
        );
        seen.on_marker(&done_log, true);
        assert_eq!(
            seen,
            CompleteMarkers {
                obs: true,
                pipe: false
            }
        );

        let mut seen = CompleteMarkers::default();
        seen.on_marker(&done_pipe, false);
        assert_eq!(
            seen,
            CompleteMarkers {
                obs: false,
                pipe: true
            }
        );
        seen.on_marker(&done_pipe, true);
        assert_eq!(
            seen,
            CompleteMarkers {
                obs: true,
                pipe: true
            }
        );
    }

    /// The longest gap between markers widens the stall deadline, except a
    /// gap that spans a hold; a `go` restarts the count.
    #[test]
    fn the_marker_clock_skips_gaps_that_span_a_hold() {
        let t0 = ago(100);
        let mut clock = MarkerClock::default();
        clock.on_marker(t0, false);
        assert_eq!(clock.longest_gap, Duration::ZERO, "nothing to measure from");
        clock.on_marker(t0 + Duration::from_secs(10), false);
        assert_eq!(clock.longest_gap, Duration::from_secs(10));
        clock.on_marker(t0 + Duration::from_secs(70), true);
        assert_eq!(clock.longest_gap, Duration::from_secs(10), "held");
        assert_eq!(clock.last_marker_at, Some(t0 + Duration::from_secs(70)));
        clock.on_go(t0 + Duration::from_secs(90));
        assert_eq!(clock.last_marker_at, Some(t0 + Duration::from_secs(90)));
        clock.on_marker(t0 + Duration::from_secs(95), false);
        assert_eq!(clock.longest_gap, Duration::from_secs(10));
    }

    #[test]
    fn a_stall_is_only_called_in_obs_mode_with_the_game_up_and_not_held() {
        let quiet = MarkerClock {
            last_marker_at: Some(ago(301)),
            longest_gap: Duration::from_secs(5),
        };
        let stall = stalled(true, false, &quiet, Some(ago(400)), true).expect("stalled");
        assert!(!stall.never_started);
        assert_eq!(Some(stall.since), quiet.last_marker_at);
        assert_eq!(stalled(false, false, &quiet, Some(ago(400)), true), None);
        assert_eq!(stalled(true, true, &quiet, Some(ago(400)), true), None);
        assert_eq!(stalled(true, false, &quiet, Some(ago(400)), false), None);

        // Not yet past the deadline, which a long gap already seen widens.
        let recent = MarkerClock {
            last_marker_at: Some(ago(200)),
            longest_gap: Duration::ZERO,
        };
        assert_eq!(stalled(true, false, &recent, Some(ago(400)), true), None);
        let slow_demo = MarkerClock {
            last_marker_at: Some(ago(400)),
            longest_gap: Duration::from_secs(200),
        };
        assert_eq!(stalled(true, false, &slow_demo, Some(ago(500)), true), None);
    }

    /// No marker at all counts from hl.exe coming up, and says the demo
    /// never started; before hl.exe is seen there is nothing to count from.
    #[test]
    fn a_batch_with_no_markers_counts_from_the_game_coming_up() {
        let none = MarkerClock::default();
        let stall = stalled(true, false, &none, Some(ago(301)), true).expect("stalled");
        assert!(stall.never_started);
        assert_eq!(stalled(true, false, &none, None, true), None);
        assert_eq!(stalled(true, false, &none, Some(ago(10)), true), None);
    }

    #[test]
    fn a_stall_says_whether_the_demo_ever_started() {
        let (line, reason) = stall_texts(Duration::from_secs(301), true);
        assert_eq!(
            line,
            "[HLAE] No console markers for 5 min 1 s with hl.exe still running — treating the batch as stalled. Playback never produced a single marker, so the demo most likely failed to load."
        );
        assert_eq!(
            reason,
            "the demo never started playing — hl.exe came up but produced no console markers at all"
        );
        let (line, reason) = stall_texts(Duration::from_secs(30), false);
        assert!(line.starts_with("[HLAE] No console markers for 30.0 s with hl.exe still running"));
        assert!(line.ends_with("a frozen engine all look like this from out here."));
        assert!(reason.starts_with("the batch stopped progressing"));
    }

    /// Done needs 11 whole seconds, then any one of the four signs; the
    /// folders aren't read before then, nor after the first one found.
    #[test]
    fn a_batch_is_complete_after_its_first_ten_seconds_by_any_sign() {
        let folders = Folders {
            done: true,
            ..Default::default()
        };
        let none = CompleteMarkers::default();
        assert!(!batch_complete(
            Duration::from_millis(10_999),
            none,
            &folders
        ));
        assert_eq!(folders.reads.get(), 0, "too early to look");
        assert!(batch_complete(Duration::from_secs(11), none, &folders));
        assert_eq!(folders.reads.get(), 1, "the done marker was enough");

        let trigger = Folders {
            trigger: true,
            ..Default::default()
        };
        assert!(batch_complete(Duration::from_secs(11), none, &trigger));
        let nothing = Folders::default();
        assert!(!batch_complete(Duration::from_secs(60), none, &nothing));
        let obs = CompleteMarkers {
            obs: true,
            pipe: false,
        };
        let pipe = CompleteMarkers {
            obs: false,
            pipe: true,
        };
        assert!(batch_complete(Duration::from_secs(11), obs, &nothing));
        assert!(batch_complete(Duration::from_secs(11), pipe, &nothing));
    }

    #[test]
    fn the_game_never_started_once_the_launcher_left_and_5_s_passed() {
        let nothing = Folders::default();
        let after = Duration::from_secs(6);
        assert!(game_never_started(true, false, after, &nothing));
        assert!(
            !game_never_started(false, false, after, &nothing),
            "launcher still up"
        );
        assert!(
            !game_never_started(true, true, after, &nothing),
            "the game was seen"
        );
        assert!(!game_never_started(
            true,
            false,
            Duration::from_millis(5_999),
            &nothing
        ));
        let done = Folders {
            done: true,
            ..Default::default()
        };
        assert!(!game_never_started(true, false, after, &done));
        let trigger = Folders {
            trigger: true,
            ..Default::default()
        };
        assert!(!game_never_started(true, false, after, &trigger));
    }

    #[test]
    fn the_game_went_away_when_it_was_seen_and_nothing_says_done() {
        let nothing = Folders::default();
        let none = CompleteMarkers::default();
        assert!(game_went_away(true, false, none, &nothing));
        assert!(!game_went_away(false, false, none, &nothing), "never seen");
        assert!(!game_went_away(true, true, none, &nothing), "still up");
        assert_eq!(
            nothing.reads.get(),
            2,
            "read only on the one that got that far"
        );
        let pipe = CompleteMarkers {
            obs: false,
            pipe: true,
        };
        assert!(!game_went_away(true, false, pipe, &nothing));
        // The OBS-mode marker alone doesn't count here.
        let obs = CompleteMarkers {
            obs: true,
            pipe: false,
        };
        assert!(game_went_away(true, false, obs, &nothing));
        let done = Folders {
            done: true,
            ..Default::default()
        };
        assert!(!game_went_away(true, false, none, &done));
        let trigger = Folders {
            trigger: true,
            ..Default::default()
        };
        assert!(!game_went_away(true, false, none, &trigger));
    }

    #[test]
    fn a_hold_is_sent_when_it_changes_and_retried_a_second_after_a_failure() {
        assert!(hold_send_due(true, true, false, None));
        assert!(hold_send_due(true, false, true, None));
        assert!(!hold_send_due(true, true, true, None), "already told");
        assert!(
            !hold_send_due(false, true, false, None),
            "a patched batch can't hold"
        );
        assert!(!hold_send_due(true, true, false, Some(Instant::now())));
        assert!(hold_send_due(true, true, false, Some(ago(1))));
    }

    #[test]
    fn the_console_log_warning_names_the_file_and_the_grace() {
        let text = console_log_not_written_text(Path::new("C:/Games/qconsole.log"));
        assert!(text.starts_with(
            "⚠️ **The engine is not writing its console log.** `C:/Games/qconsole.log` is unchanged 1 min 0 s after the game started"
        ));
        assert!(text.ends_with("or HLAE may not be passing the flag through to the game."));
    }
}
