//! Brings a game Studio started through WMI to the front once its window
//! shows, when Studio is still the window in front.
//!
//! Windows lets a process take the foreground when the foreground process
//! started it. A dev Studio starts the HLAE launcher through WMI (#545), so
//! the game is WMI's grandchild, not Studio's, and opened behind every other
//! window on Launch Game, Review and Launch Preview. Studio, still in front,
//! may hand the foreground on itself: once the new game's window shows, it
//! calls `SetForegroundWindow` on it. The game is left where it is when
//! another program's window is in front by then (the user moved on while the
//! game loaded), when no window is in front, or when Windows refuses the call
//! (it does when Studio didn't receive the last input, as with a scripted
//! click). [`Outcome`] tells these apart in the activity log.
//!
//! Only this launch's game is brought forward: an hl.exe that started after
//! the launch and wasn't running before it, told apart by pid and start time
//! as a batch tells its game apart (#1213). The watch ends soon after the
//! launcher exits with no such game, and a newer launch ends an older
//! launch's watch, as the error-box watch does (#726).
//!
//! A capture batch keeps the game behind (#434) and never comes here.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::sys::process::GameProcess;

/// How long to wait for the game's window: a cold start with the HD
/// textures loads for well over a minute.
const WAIT: Duration = Duration::from_secs(180);
/// How often to look: the process list is read each time.
const EVERY: Duration = Duration::from_millis(250);
/// How long the watch goes on once the launcher has exited with no game of
/// this launch's running, in case the game shows in the list a moment late.
const LAUNCHER_GONE_GRACE: Duration = Duration::from_secs(2);
/// How long the game's window, once shown, waits for some window to be in
/// front: the foreground passes through none for a moment while windows
/// change.
const NO_FRONT_RECHECK: Duration = Duration::from_secs(1);
/// How often that wait looks again.
const NO_FRONT_EVERY: Duration = Duration::from_millis(100);

/// The newest launch's watch; every launch bumps it, and an older watch ends
/// on its next look.
static CURRENT_WATCH: AtomicU64 = AtomicU64::new(0);

/// What a launch knew about the games before it started: which were already
/// running, and when it started.
#[derive(Debug, Clone)]
pub struct ThisLaunch {
    games_before: Vec<GameProcess>,
    /// Seconds since the epoch, as process start times are kept.
    started_secs: u64,
}

impl ThisLaunch {
    /// The games running now, and now as the launch's start. Called just
    /// before the launcher is started.
    pub fn starting_now() -> Self {
        // The clock is read before the list, so a game that starts between
        // the two is in `games_before` and never taken for this launch's.
        let started_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        Self {
            games_before: crate::sys::process::games(),
            started_secs,
        }
    }

    /// This launch's game among `games`: one that started at or after the
    /// launch and wasn't running before it. A game whose start time can't be
    /// read (0) never counts.
    fn game_in(&self, games: &[GameProcess]) -> Option<GameProcess> {
        games
            .iter()
            .copied()
            .find(|game| game.started >= self.started_secs && !self.games_before.contains(game))
    }
}

/// Watches, off the caller's thread, for this launch's game to show its
/// window, and brings it to the front when Studio is in front. Logs one
/// activity-log line saying how the watch ended. `launcher_pid` and
/// `launcher_exe` are the launcher WMI started: once it has exited with no
/// game of this launch's, the watch ends. Ends any older launch's watch.
pub fn when_shown(launch: ThisLaunch, launcher_pid: u32, launcher_exe: String) {
    let watch = Watch {
        launch,
        launcher_pid,
        launcher_exe,
        launcher_gone_grace: LAUNCHER_GONE_GRACE,
        turn: end_older_watches_in(&CURRENT_WATCH),
    };
    std::thread::spawn(move || {
        let started = Instant::now();
        let outcome = watch.run(&CURRENT_WATCH, started);
        crate::log_markdown(&crate::messages::bring_forward_outcome(
            outcome,
            started.elapsed(),
        ));
    });
}

/// Ends any running watch on its next look, without starting one. A launch
/// that keeps the game behind (a capture batch, #434) calls it, so an older
/// interactive launch's watch, still waiting, never brings the batch's game
/// forward.
pub fn end_older_watches() {
    end_older_watches_in(&CURRENT_WATCH);
}

/// Takes the next turn of `current`, ending every older watch; returns it.
fn end_older_watches_in(current: &AtomicU64) -> u64 {
    current.fetch_add(1, Ordering::SeqCst) + 1
}

/// The newest watch's turn, for tests that check a launch ended the others.
#[cfg(test)]
pub(crate) fn current_turn() -> u64 {
    CURRENT_WATCH.load(Ordering::SeqCst)
}

/// How the watch ended, for the activity log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The game came to the front.
    Brought,
    /// The game came to the front by itself.
    AlreadyInFront,
    /// Another program's window was in front when the game's showed, so the
    /// user had moved on and the game was left behind.
    OtherWindowInFront,
    /// No window was in front when the game's showed, even after waiting
    /// [`NO_FRONT_RECHECK`] for one, so the game was left behind.
    NoForeground,
    /// Studio was in front, but Windows refused `SetForegroundWindow`; the
    /// error is `GetLastError`'s (0 when it set none).
    Refused(u32),
    /// No new game window showed in time, or the launch ended without one.
    NoWindow,
    /// A newer launch started its own watch.
    Superseded,
}

/// One launch's watch.
struct Watch {
    launch: ThisLaunch,
    launcher_pid: u32,
    launcher_exe: String,
    /// How long the watch goes on once the launcher has exited with no game
    /// ([`LAUNCHER_GONE_GRACE`]; a test passes a short one).
    launcher_gone_grace: Duration,
    /// This watch's number: it runs while [`CURRENT_WATCH`] still holds it.
    turn: u64,
}

impl Watch {
    /// Polls the process list until this launch's game shows a window, then
    /// tries to bring it forward. `current` is the newest watch's number.
    fn run(&self, current: &AtomicU64, started: Instant) -> Outcome {
        let mut sys = crate::sys::process::snapshot();
        let mut game: Option<GameProcess> = None;
        let mut launcher_gone_since: Option<Instant> = None;
        while started.elapsed() < WAIT {
            if current.load(Ordering::SeqCst) != self.turn {
                return Outcome::Superseded;
            }
            std::thread::sleep(EVERY);
            crate::sys::process::refresh(&mut sys);
            let games = crate::sys::process::games_in(&sys);
            if game.is_none() {
                game = self.launch.game_in(&games);
            }
            match game {
                // This launch's game closed before it showed a window.
                Some(own) if !games.contains(&own) => return Outcome::NoWindow,
                Some(own) => {
                    if let Some(window) = win::visible_window(own.pid) {
                        return win::bring(window);
                    }
                }
                None => {
                    if crate::sys::launcher::is_running_in(
                        &sys,
                        self.launcher_pid,
                        &self.launcher_exe,
                    ) {
                        launcher_gone_since = None;
                    } else if launcher_gone_since
                        .get_or_insert_with(Instant::now)
                        .elapsed()
                        >= self.launcher_gone_grace
                    {
                        return Outcome::NoWindow;
                    }
                }
            }
        }
        Outcome::NoWindow
    }
}

/// What [`decide`] looks at once the game's window has shown.
#[derive(Debug, Clone, Copy)]
struct Foreground {
    /// The game's window.
    game_window: isize,
    /// The window in front; 0 for none.
    front: isize,
    /// The pid owning `front`.
    front_owner: u32,
    /// Studio's own pid.
    own_pid: u32,
}

/// What to do with the game's window, given what is in front.
/// `set_foreground` asks Windows to bring it forward, failing with
/// `GetLastError`'s code; it is only called when Studio is the one in front.
fn decide(seen: Foreground, set_foreground: impl FnOnce() -> Result<(), u32>) -> Outcome {
    if seen.front == seen.game_window {
        return Outcome::AlreadyInFront;
    }
    if seen.front == 0 {
        return Outcome::NoForeground;
    }
    if seen.front_owner != seen.own_pid {
        return Outcome::OtherWindowInFront;
    }
    match set_foreground() {
        Ok(()) => Outcome::Brought,
        Err(code) => Outcome::Refused(code),
    }
}

/// The window in front, read with `read_front` until it isn't 0 or `wait`
/// has passed, looking every `every`. 0 when none showed in that time.
fn settled_front(mut read_front: impl FnMut() -> isize, wait: Duration, every: Duration) -> isize {
    let since = Instant::now();
    loop {
        let front = read_front();
        if front != 0 || since.elapsed() >= wait {
            return front;
        }
        std::thread::sleep(every);
    }
}

#[cfg(windows)]
mod win {
    use super::{Foreground, NO_FRONT_EVERY, NO_FRONT_RECHECK, Outcome, decide, settled_front};
    use crate::sys::window_list::Hwnd;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> Hwnd;
        fn SetForegroundWindow(window: Hwnd) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetLastError(code: u32);
    }

    pub fn visible_window(pid: u32) -> Option<Hwnd> {
        crate::sys::window_list::visible_window(pid)
    }

    fn set_foreground(window: Hwnd) -> Result<(), u32> {
        // SetForegroundWindow often fails without setting an error, so the
        // code is cleared first: otherwise the log would show whatever an
        // earlier call on this thread left (the process-list refresh leaves
        // "Access is denied" for processes it can't open).
        // SAFETY: sets this thread's last-error value; no pointers.
        unsafe { SetLastError(0) };
        // SAFETY: a window that has closed since only makes this fail.
        if unsafe { SetForegroundWindow(window) } != 0 {
            return Ok(());
        }
        // 0 when the refusal set no error.
        Err(std::io::Error::last_os_error()
            .raw_os_error()
            .map_or(0, |code| code as u32))
    }

    pub fn bring(window: Hwnd) -> Outcome {
        let front = settled_front(
            // SAFETY: no arguments; returns 0 when no window is in front.
            || unsafe { GetForegroundWindow() },
            NO_FRONT_RECHECK,
            NO_FRONT_EVERY,
        );
        let seen = Foreground {
            game_window: window,
            front,
            front_owner: crate::sys::window_list::owner(front),
            own_pid: std::process::id(),
        };
        decide(seen, || set_foreground(window))
    }
}

#[cfg(not(windows))]
mod win {
    use super::Outcome;

    pub fn visible_window(_pid: u32) -> Option<isize> {
        None
    }

    pub fn bring(_window: isize) -> Outcome {
        Outcome::NoWindow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME_WINDOW: isize = 0x10;
    const STUDIO_WINDOW: isize = 0x20;
    const STUDIO: u32 = 100;
    const OTHER_PROGRAM: u32 = 200;

    fn never_called() -> Result<(), u32> {
        panic!("SetForegroundWindow was called when Studio wasn't in front")
    }

    /// The game's window shown, with `front` (owned by `front_owner`) in
    /// front of Studio's pid.
    fn seen(front: isize, front_owner: u32) -> Foreground {
        Foreground {
            game_window: GAME_WINDOW,
            front,
            front_owner,
            own_pid: STUDIO,
        }
    }

    #[test]
    fn a_game_already_in_front_is_left_alone() {
        let outcome = decide(seen(GAME_WINDOW, 300), never_called);
        assert_eq!(outcome, Outcome::AlreadyInFront);
    }

    #[test]
    fn no_window_in_front_leaves_the_game_behind() {
        assert_eq!(decide(seen(0, 0), never_called), Outcome::NoForeground);
    }

    #[test]
    fn another_programs_window_in_front_leaves_the_game_behind() {
        assert_eq!(
            decide(seen(0x30, OTHER_PROGRAM), never_called),
            Outcome::OtherWindowInFront
        );
    }

    #[test]
    fn studio_in_front_brings_the_game_forward() {
        assert_eq!(
            decide(seen(STUDIO_WINDOW, STUDIO), || Ok(())),
            Outcome::Brought
        );
    }

    #[test]
    fn a_refused_bring_keeps_its_error() {
        assert_eq!(
            decide(seen(STUDIO_WINDOW, STUDIO), || Err(5)),
            Outcome::Refused(5)
        );
    }

    #[test]
    fn a_moment_with_no_window_in_front_is_waited_out() {
        let mut reads = [0, 0, STUDIO_WINDOW].into_iter();
        let front = settled_front(
            || reads.next().unwrap_or(STUDIO_WINDOW),
            Duration::from_secs(5),
            Duration::from_millis(1),
        );
        assert_eq!(front, STUDIO_WINDOW);
    }

    #[test]
    fn no_window_in_front_for_the_whole_wait_reads_as_none() {
        let started = Instant::now();
        let front = settled_front(|| 0, Duration::from_millis(30), Duration::from_millis(5));
        assert_eq!(front, 0);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    fn launch(games_before: Vec<GameProcess>, started_secs: u64) -> ThisLaunch {
        ThisLaunch {
            games_before,
            started_secs,
        }
    }

    #[test]
    fn a_game_running_before_the_launch_is_not_this_launchs() {
        let before = GameProcess {
            pid: 7,
            started: 900,
        };
        assert_eq!(launch(vec![before], 1000).game_in(&[before]), None);
    }

    #[test]
    fn a_reused_pid_started_after_the_launch_is_this_launchs() {
        let before = GameProcess {
            pid: 7,
            started: 1000,
        };
        let reused = GameProcess {
            pid: 7,
            started: 1001,
        };
        assert_eq!(launch(vec![before], 1000).game_in(&[reused]), Some(reused));
    }

    #[test]
    fn a_game_started_before_the_launch_is_never_this_launchs() {
        // Missing from the list taken before the launch, but older than it.
        let older = GameProcess {
            pid: 8,
            started: 999,
        };
        let unreadable = GameProcess { pid: 9, started: 0 };
        assert_eq!(launch(Vec::new(), 1000).game_in(&[older, unreadable]), None);
    }

    fn watch_of_no_launcher(turn: u64) -> Watch {
        Watch {
            launch: ThisLaunch::starting_now(),
            launcher_pid: u32::MAX - 1,
            launcher_exe: "no-such-launcher-dodstudio.exe".to_string(),
            launcher_gone_grace: Duration::ZERO,
            turn,
        }
    }

    #[test]
    fn a_launch_that_ended_with_no_new_game_stops_the_watch_soon() {
        // No launcher has this pid, and no game has started since: with no
        // grace the first look ends the watch, bringing nothing forward.
        let current = AtomicU64::new(1);
        let started = Instant::now();
        assert_eq!(
            watch_of_no_launcher(1).run(&current, started),
            Outcome::NoWindow
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_newer_launch_ends_an_older_watch() {
        let current = AtomicU64::new(2);
        assert_eq!(
            watch_of_no_launcher(1).run(&current, Instant::now()),
            Outcome::Superseded
        );
    }

    #[test]
    fn a_launch_that_keeps_the_game_behind_ends_an_older_watch() {
        let current = AtomicU64::new(0);
        let watch = watch_of_no_launcher(end_older_watches_in(&current));
        end_older_watches_in(&current);
        assert_eq!(watch.run(&current, Instant::now()), Outcome::Superseded);
    }

    #[test]
    fn the_log_says_what_happened_and_when() {
        assert_eq!(
            crate::messages::bring_forward_outcome(Outcome::Brought, Duration::from_millis(9400)),
            "[launch] brought the game's window to the front (9.4 s after the launch)"
        );
    }

    #[test]
    fn a_refusal_is_logged_as_a_failure_with_its_error() {
        let line = crate::messages::bring_forward_outcome(
            Outcome::Refused(5),
            Duration::from_millis(2800),
        );
        assert!(
            line.starts_with("[launch] failed to bring the game's window to the front"),
            "{line}"
        );
        assert!(line.contains("error 5"), "{line}");
        assert!(line.ends_with("(2.8 s after the launch)"), "{line}");
    }

    #[test]
    fn each_way_of_leaving_the_game_behind_has_its_own_line() {
        let lines: Vec<String> = [
            Outcome::OtherWindowInFront,
            Outcome::NoForeground,
            Outcome::Refused(0),
        ]
        .into_iter()
        .map(|outcome| crate::messages::bring_forward_outcome(outcome, Duration::ZERO))
        .collect();
        assert_ne!(lines[0], lines[1]);
        assert_ne!(lines[1], lines[2]);
        assert_ne!(lines[0], lines[2]);
    }
}
