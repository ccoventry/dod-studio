//! `BatchWatch::pass` end to end, over a made-up process list (#1213): what
//! one pass checks, in which order, and which game it ends. No game is
//! started or ended: the list and the kills are [`FakeProcesses`]'. The pids
//! are odd, which Windows never gives a process, so the real error-box check
//! each pass makes can't find a window of theirs.

use super::*;
use crate::capture_engine::processes::GameProcess;
use crate::obs::log_tail::parse_marker;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

/// What the fake list holds and what was done to it.
#[derive(Default)]
struct Machine {
    games: Vec<GameProcess>,
    refreshes: u32,
    killed: Vec<u32>,
    cursor_released: u32,
}

/// A process list the test sets, shared with the test through `Rc`.
#[derive(Clone, Default)]
struct FakeProcesses(Rc<RefCell<Machine>>);

impl Processes for FakeProcesses {
    fn refresh(&mut self) {
        self.0.borrow_mut().refreshes += 1;
    }
    fn games(&self) -> Vec<GameProcess> {
        self.0.borrow().games.clone()
    }
    fn injectors_of(&self, _launcher: u32) -> Vec<u32> {
        Vec::new()
    }
    fn launcher_status(
        &self,
        _launcher: &mut crate::sys::launcher::Launcher,
    ) -> std::io::Result<Option<String>> {
        Ok(None)
    }
    fn kill_pid(&mut self, pid: u32) {
        let mut m = self.0.borrow_mut();
        m.killed.push(pid);
        m.games.retain(|g| g.pid != pid);
    }
    fn release_cursor(&mut self) -> bool {
        self.0.borrow_mut().cursor_released += 1;
        false
    }
}

impl FakeProcesses {
    fn set(&self, games: &[GameProcess]) {
        self.0.borrow_mut().games = games.to_vec();
    }
    fn m(&self) -> std::cell::Ref<'_, Machine> {
        self.0.borrow()
    }
}

/// A game open before the launch: never the batch's.
const THEIRS: GameProcess = GameProcess {
    pid: 5,
    started: 100,
};
/// The batch's own game.
const OURS: GameProcess = GameProcess {
    pid: 7,
    started: 200,
};
/// A game opened after the batch's was found.
const LATER: GameProcess = GameProcess {
    pid: 11,
    started: 300,
};

/// What a `BatchWatch` borrows.
struct Rig {
    tx: mpsc::Sender<EngineEvent>,
    _events: mpsc::Receiver<EngineEvent>,
    cancel: AtomicBool,
    hold: AtomicBool,
    start: BatchStart,
    markers: mpsc::Sender<crate::obs::Marker>,
    marker_rx: mpsc::Receiver<crate::obs::Marker>,
    log_path: std::path::PathBuf,
    done_marker: std::path::PathBuf,
    exit_trigger: std::path::PathBuf,
    _dir: crate::test_support::Scratch,
}

impl Rig {
    fn new(tag: &str) -> Self {
        let (tx, events) = mpsc::channel();
        let (markers, marker_rx) = mpsc::channel();
        let dir = crate::test_support::Scratch::new(tag);
        Self {
            tx,
            _events: events,
            cancel: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            start: BatchStart::default(),
            markers,
            marker_rx,
            log_path: dir.join("qconsole.log"),
            done_marker: dir.join("DOD_BATCH_DONE"),
            exit_trigger: dir.join("exit_trigger"),
            _dir: dir,
        }
    }

    /// A launched batch, not in an open game, with `THEIRS` running before
    /// the launch and `procs` as its process list.
    fn watch(&self, obs_mode: bool, procs: &FakeProcesses) -> BatchWatch<'_> {
        let mut watch = BatchWatch::new(
            WatchInputs {
                tx: &self.tx,
                cancel_token: &self.cancel,
                hold: &self.hold,
                batch_start: &self.start,
                obs_mode,
                hook_runs_batch: false,
                reuse_game: None,
                marker_rx: &self.marker_rx,
                log_path: &self.log_path,
                log_len_before_launch: 0,
                done_marker: &self.done_marker,
                exit_trigger: &self.exit_trigger,
            },
            None,
            None,
            BatchGame::new(vec![THEIRS], None),
            None,
            crate::sys::foreground_lock::ForegroundLock::none(),
            Instant::now(),
        );
        watch.procs = Box::new(procs.clone());
        watch
    }
}

/// The process list is re-read once per `PROCESS_CHECK_EVERY`, not on every
/// pass: OBS mode passes every 16 ms (M8 of the #1210 review).
#[test]
fn the_process_list_is_re_read_only_once_its_interval_passed() {
    let rig = Rig::new("watch_pass_reread");
    let procs = FakeProcesses::default();
    procs.set(&[THEIRS, OURS]);
    let mut watch = rig.watch(true, &procs);
    assert!(watch.pass().is_continue());
    assert!(watch.pass().is_continue());
    assert_eq!(procs.m().refreshes, 1, "the second pass came too soon");
    watch.last_process_check = Some(Instant::now() - PROCESS_CHECK_EVERY);
    assert!(watch.pass().is_continue());
    assert_eq!(procs.m().refreshes, 2);
}

/// Cancel is checked on every pass, and ends only the batch's own game: the
/// one open before the launch and one opened since are left (M1).
#[test]
fn a_cancel_ends_the_batch_and_only_its_own_game() {
    let rig = Rig::new("watch_pass_cancel");
    let procs = FakeProcesses::default();
    procs.set(&[THEIRS, OURS]);
    let mut watch = rig.watch(false, &procs);
    assert!(watch.pass().is_continue());
    procs.set(&[THEIRS, OURS, LATER]);
    rig.cancel.store(true, Ordering::Relaxed);
    assert!(watch.pass().is_break());
    assert_eq!(procs.m().killed, [OURS.pid]);
    assert_eq!(procs.m().games, [THEIRS, LATER]);
    assert_eq!(procs.m().cursor_released, 0, "games are still open");
    assert_eq!(watch.failure_reason, None);
    assert!(!watch.game_closed);
}

/// A batch that never saw its game (cancelled while HLAE was starting it)
/// ends nothing: every game in the list was open before the launch, so none
/// is the batch's (#1212).
#[test]
fn a_cancel_before_the_game_was_seen_ends_no_game() {
    let rig = Rig::new("watch_pass_cancel_unseen");
    let procs = FakeProcesses::default();
    procs.set(&[THEIRS]);
    let mut watch = rig.watch(false, &procs);
    rig.cancel.store(true, Ordering::Relaxed);
    assert!(watch.pass().is_break());
    assert!(procs.m().killed.is_empty());
    assert_eq!(procs.m().games, [THEIRS]);
    assert_eq!(procs.m().cursor_released, 0, "a game is still open");
}

/// The batch's end is checked before its game going away: an OBS-mode
/// BATCH_COMPLETE with the game already gone is a finished batch, not a
/// closed or crashed game (M2).
#[test]
fn a_complete_batch_whose_game_is_gone_is_finished_not_closed() {
    let rig = Rig::new("watch_pass_complete_order");
    let procs = FakeProcesses::default();
    procs.set(&[OURS]);
    let mut watch = rig.watch(true, &procs);
    watch.start_time = Instant::now() - Duration::from_secs(20);
    assert!(watch.pass().is_continue());
    rig.markers
        .send(parse_marker("[dod-studio] BATCH_COMPLETE").unwrap())
        .unwrap();
    procs.set(&[]);
    watch.last_process_check = None;
    assert!(watch.pass().is_break());
    assert!(!watch.game_closed);
    assert_eq!(watch.failure_reason, None);
    assert!(procs.m().killed.is_empty(), "nothing left to end");
}

/// A finished batch closes its own game by pid and leaves the other one.
#[test]
fn a_finished_batch_closes_only_its_own_game() {
    let rig = Rig::new("watch_pass_complete");
    let procs = FakeProcesses::default();
    procs.set(&[THEIRS, OURS]);
    let mut watch = rig.watch(false, &procs);
    watch.start_time = Instant::now() - Duration::from_secs(20);
    assert!(watch.pass().is_continue());
    std::fs::create_dir_all(&rig.done_marker).unwrap();
    assert!(watch.pass().is_break());
    assert_eq!(procs.m().killed, [OURS.pid]);
    assert_eq!(procs.m().games, [THEIRS]);
    assert!(!watch.game_closed);
}

/// Another game still open doesn't keep the batch's alive: the batch's
/// going away is noticed, and nothing is ended.
#[test]
fn the_batchs_game_going_away_is_noticed_with_another_game_open() {
    let rig = Rig::new("watch_pass_went_away");
    let procs = FakeProcesses::default();
    procs.set(&[THEIRS, OURS]);
    let mut watch = rig.watch(false, &procs);
    assert!(watch.pass().is_continue());
    procs.set(&[THEIRS, LATER]);
    watch.last_process_check = None;
    assert!(watch.pass().is_break());
    assert!(watch.game_closed);
    assert!(watch.failure_reason.is_some());
    assert!(procs.m().killed.is_empty());
}
