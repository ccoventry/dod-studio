//! Brings a game Studio started through WMI to the front once its window
//! shows, when Studio is still the window in front.
//!
//! Windows lets a process take the foreground when the foreground process
//! started it. A dev Studio starts the HLAE launcher through WMI (#545), so
//! the game is WMI's grandchild, not Studio's, and opened behind every other
//! window on Launch Game, Review and Launch Preview. Studio, still in front,
//! may hand the foreground on itself: once the new game's window shows, it
//! calls `SetForegroundWindow` on it. When another window is in front by then
//! (the user moved on while the game loaded), the game is left where it is.
//!
//! A capture batch keeps the game behind (#434) and never comes here.

use std::time::{Duration, Instant};

/// How long to wait for the game's window: a cold start with the HD
/// textures loads for well over a minute.
const WAIT: Duration = Duration::from_secs(180);
/// How often to look: the process list is read each time.
const EVERY: Duration = Duration::from_millis(250);

/// Watches, off the caller's thread, for a game that wasn't running before
/// this launch (`games_before`) to show its window, and brings it to the
/// front when Studio is in front. Gives up when the launcher `launcher_pid`
/// has ended with no new game running.
pub fn when_shown(launcher_pid: u32, games_before: Vec<u32>) {
    std::thread::spawn(move || {
        let started = Instant::now();
        let outcome = watch(launcher_pid, &games_before, started);
        crate::log_markdown(&crate::messages::bring_forward_outcome(
            outcome,
            started.elapsed(),
        ));
    });
}

/// How the watch ended, for the activity log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The game came to the front.
    Brought,
    /// Another window was in front when the game's showed.
    LeftBehind,
    /// The game came to the front by itself.
    AlreadyInFront,
    /// No new game window showed in time, or the launch ended.
    NoWindow,
}

fn watch(launcher_pid: u32, games_before: &[u32], started: Instant) -> Outcome {
    let mut sys = crate::sys::process::snapshot();
    while started.elapsed() < WAIT {
        std::thread::sleep(EVERY);
        crate::sys::process::refresh(&mut sys);
        let new_games: Vec<u32> = crate::sys::process::game_pids_in(&sys)
            .into_iter()
            .filter(|pid| !games_before.contains(pid))
            .collect();
        if new_games.is_empty() && sys.process(sysinfo::Pid::from_u32(launcher_pid)).is_none() {
            return Outcome::NoWindow;
        }
        if let Some(window) = new_games
            .iter()
            .find_map(|&pid| crate::sys::minimize::visible_window(pid))
        {
            return win::bring(window);
        }
    }
    Outcome::NoWindow
}

#[cfg(windows)]
mod win {
    use super::Outcome;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowThreadProcessId(window: isize, pid: *mut u32) -> u32;
        fn SetForegroundWindow(window: isize) -> i32;
    }

    fn owner(window: isize) -> u32 {
        let mut pid = 0u32;
        // SAFETY: `pid` is a valid out pointer; a closed window only fails.
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        pid
    }

    pub fn bring(window: isize) -> Outcome {
        // SAFETY: no arguments; returns 0 when no window is in front.
        let front = unsafe { GetForegroundWindow() };
        if front == window {
            return Outcome::AlreadyInFront;
        }
        if front == 0 || owner(front) != std::process::id() {
            return Outcome::LeftBehind;
        }
        // SAFETY: a window that has closed since only makes this fail.
        if unsafe { SetForegroundWindow(window) } != 0 {
            Outcome::Brought
        } else {
            Outcome::LeftBehind
        }
    }
}

#[cfg(not(windows))]
mod win {
    use super::Outcome;

    pub fn bring(_window: isize) -> Outcome {
        Outcome::LeftBehind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_that_ended_with_no_new_game_stops_the_watch() {
        // No launcher has this pid, and every game running now was running
        // before: the first look ends the watch, bringing nothing forward.
        let started = Instant::now();
        let outcome = watch(u32::MAX - 1, &crate::sys::process::game_pids(), started);
        assert_eq!(outcome, Outcome::NoWindow);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_log_says_what_happened_and_when() {
        assert_eq!(
            crate::messages::bring_forward_outcome(Outcome::Brought, Duration::from_millis(9400)),
            "[launch] brought the game's window to the front (9.4 s after the launch)"
        );
    }
}
