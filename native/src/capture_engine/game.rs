//! Ending the game a batch ran in: closing an hl.exe the batch started, or
//! leaving one it borrowed (#725) open at its menu. Only the batch's own
//! hl.exe is ever closed, by its pid (#1213): another game open on the
//! machine is not the batch's to end.

use super::processes::{GameProcess, Processes};
use crate::log_markdown;

/// Which hl.exe is the batch's: the open game it runs in (#725), or the first
/// one to appear that wasn't running before the launch. Kept by pid and start
/// time, so a pid Windows later gives another process is not taken for the
/// game, the way `Launcher` tracks a launcher it doesn't own (#1198).
#[derive(Debug, Default)]
pub(super) struct BatchGame {
    /// The games running before the launch: not the batch's.
    before_launch: Vec<GameProcess>,
    /// The open game the batch runs in instead of starting one.
    reuse: Option<u32>,
    /// The batch's game, from the first time it was found.
    seen: Option<GameProcess>,
}

impl BatchGame {
    pub(super) fn new(before_launch: Vec<GameProcess>, reuse: Option<u32>) -> Self {
        Self {
            before_launch,
            reuse,
            seen: None,
        }
    }

    /// The batch's game among `games`, when it is there. The first one found
    /// is the batch's from then on.
    pub(super) fn find(&mut self, games: &[GameProcess]) -> Option<GameProcess> {
        if let Some(seen) = self.seen {
            return games.contains(&seen).then_some(seen);
        }
        self.seen = games.iter().copied().find(|g| match self.reuse {
            Some(pid) => g.pid == pid,
            None => !self.before_launch.contains(g),
        });
        self.seen
    }

    /// The batch's game's pid in the list as last read, when it is there.
    pub(super) fn pid_in(&mut self, procs: &dyn Processes) -> Option<u32> {
        self.find(&procs.games()).map(|g| g.pid)
    }
}

/// Waits, briefly, for the batch's game to actually leave the process list
/// after a `taskkill /F` -- `taskkill` returning only means Windows has been
/// asked to end the process, not that it (and its open file handles) are gone
/// yet. `CaptureCleanupGuard`'s `Drop` deletes this batch's demo files the
/// moment the batch's thread returns, right after every `taskkill` call site
/// in the batch's poll loop ends it -- without this wait, the very last demo
/// in the chain can still be open when that delete runs. See #198.
///
/// Bounded to 3 seconds so a process that was already gone when `taskkill`
/// ran (the game crashed on its own, or was closed by the user moments
/// earlier) can never turn this into an extra wait on every batch --
/// `remove_file_retrying` is the second, independent line of defence for
/// whatever this window doesn't cover. Not cut short by Cancel: a cancelled
/// batch removes its demo copies too, which is what the wait is for.
///
/// A batch whose game was never seen waits for every hl.exe, the ones
/// [`kill_hl_exe_and_wait`]'s fallback ended by name.
pub(super) fn wait_for_hl_exe_to_exit(procs: &mut dyn Processes, game: &BatchGame) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        procs.refresh();
        let games = procs.games();
        let gone = match game.seen {
            Some(own) => !games.contains(&own),
            None => games.is_empty(),
        };
        if gone || std::time::Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Ends the batch's game by its pid and waits for it to go, as
/// [`wait_for_hl_exe_to_exit`] describes. Every way out of a batch's loop
/// that leaves the game running goes through here.
///
/// A batch that never saw its game (a cancel while HLAE was still starting
/// it) falls back to ending every hl.exe by name, as every batch used to, and
/// says so: ending nothing could leave the batch's game running. A game seen
/// and already gone is left at that. A killed game can't release the mouse it
/// confined, so once no game is left Studio does (#1095).
pub(super) fn kill_hl_exe_and_wait(procs: &mut dyn Processes, game: &mut BatchGame) {
    procs.refresh();
    let own = game.find(&procs.games());
    match (own, game.seen) {
        (Some(own), _) => {
            procs.kill_pid(own.pid);
            wait_for_hl_exe_to_exit(procs, game);
        }
        (None, Some(_)) => {}
        (None, None) => {
            log_markdown(
                "[HLAE] This batch's hl.exe never showed up in the process list, so every hl.exe \
                 is ended by name, any other game open included (#1213)",
            );
            procs.kill_every_game();
            wait_for_hl_exe_to_exit(procs, game);
        }
    }
    if procs.games().is_empty() && procs.release_cursor() {
        log_markdown("[capture] Released the mouse the closed game had confined to its window.");
    }
}

/// How a reused game is left when the batch ends or is cancelled:
/// open, at its menu, instead of closed.
pub(super) fn leave_game(pid: u32) {
    let lines: Vec<String> = crate::patch::batch_reuse::LEAVE_LINES
        .iter()
        .map(|l| l.to_string())
        .collect();
    match crate::sys::game_remote::send_console_commands(pid, &lines) {
        Ok(crate::sys::game_remote::Sent::Delivered) => log_markdown(&format!(
            "[HLAE] Left the game open (PID {pid}): `{}`",
            lines.join("; ")
        )),
        other => log_markdown(&format!(
            "[HLAE] Could not reach the game (PID {pid}) to stop its recording and demo: {other:?}"
        )),
    }
}

/// Ends the batch's game the way its end calls for: a borrowed game is left
/// open, one the batch started is closed.
pub(super) fn end_game(reuse_game: Option<u32>, procs: &mut dyn Processes, game: &mut BatchGame) {
    match reuse_game {
        Some(pid) => leave_game(pid),
        None => kill_hl_exe_and_wait(procs, game),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(pid: u32, started: u64) -> GameProcess {
        GameProcess { pid, started }
    }

    /// The batch's game is the first one not running before the launch, and
    /// stays that one: a later game is not taken for it, nor a new process
    /// with its pid.
    #[test]
    fn the_batchs_game_is_the_first_new_one_and_stays_it() {
        let theirs = game(8, 100);
        let mut own = BatchGame::new(vec![theirs], None);
        assert_eq!(own.find(&[theirs]), None, "only a game from before");
        let ours = game(12, 200);
        assert_eq!(own.find(&[theirs, ours]), Some(ours));
        assert_eq!(own.find(&[game(16, 300), ours, theirs]), Some(ours));
        assert_eq!(own.find(&[theirs, game(16, 300)]), None, "ours is gone");
        assert_eq!(own.find(&[game(12, 400)]), None, "its pid, reused");
    }

    /// A batch in the open game (#725) has that game, by pid.
    #[test]
    fn a_batch_in_the_open_game_has_that_game() {
        let open = game(20, 100);
        let mut own = BatchGame::new(vec![open, game(24, 50)], Some(20));
        assert_eq!(own.find(&[game(24, 50), open]), Some(open));
    }
}
