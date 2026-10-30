//! The processes a batch's poll loop reads and ends: every `hl.exe` in the
//! process list, the HLAE launcher and its injectors. Behind [`Processes`]
//! so the loop's passes can be tested without a game, and without a test
//! ever ending one (#1213).

use crate::sys::launcher::Launcher;
use sysinfo::System;

/// One `hl.exe` in the process list. Its start time tells the game apart
/// from a later process Windows gives the same pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GameProcess {
    pub(super) pid: u32,
    /// Seconds since the epoch, as Windows reports it; 0 when it can't be read.
    pub(super) started: u64,
}

/// What the poll loop reads from, and does to, the machine's processes.
pub(super) trait Processes {
    /// Re-reads the process list.
    fn refresh(&mut self);
    /// Every `hl.exe` in the list as last read.
    fn games(&self) -> Vec<GameProcess>;
    /// The `injector.exe` processes launcher `launcher` started, still running.
    fn injectors_of(&self, launcher: u32) -> Vec<u32>;
    /// How `launcher` exited, once it has, against the list as last read.
    fn launcher_status(&self, launcher: &mut Launcher) -> std::io::Result<Option<String>>;
    /// Asks Windows to end process `pid` (`taskkill /F /PID`).
    fn kill_pid(&mut self, pid: u32);
    /// Asks Windows to end every `hl.exe` (`taskkill /F /IM hl.exe`).
    fn kill_every_game(&mut self);
    /// Frees the mouse a closed game confined (#1095); true when it was.
    fn release_cursor(&mut self) -> bool;
}

/// The machine's real process list, and real kills.
pub(super) struct Live {
    sys: System,
}

impl Live {
    pub(super) fn new() -> Self {
        Self {
            sys: crate::sys::process::snapshot(),
        }
    }
}

impl Processes for Live {
    fn refresh(&mut self) {
        crate::sys::process::refresh(&mut self.sys);
    }

    fn games(&self) -> Vec<GameProcess> {
        self.sys
            .processes()
            .values()
            .filter(|p| crate::sys::process::is_named(p, &[crate::sys::process::GAME_EXE]))
            .map(|p| GameProcess {
                pid: p.pid().as_u32(),
                started: p.start_time(),
            })
            .collect()
    }

    fn injectors_of(&self, launcher: u32) -> Vec<u32> {
        crate::sys::process::injector_pids_in(&self.sys, launcher)
    }

    fn launcher_status(&self, launcher: &mut Launcher) -> std::io::Result<Option<String>> {
        launcher.exit_status(&self.sys)
    }

    fn kill_pid(&mut self, pid: u32) {
        crate::sys::process::kill_pid(pid);
    }

    fn kill_every_game(&mut self) {
        crate::sys::process::kill_game();
    }

    fn release_cursor(&mut self) -> bool {
        crate::sys::cursor_clip::release()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test binary isn't a game, so the live list never names it as one.
    #[test]
    fn the_live_list_only_names_games() {
        let live = Live::new();
        assert!(live.games().iter().all(|g| g.pid != std::process::id()));
    }
}
