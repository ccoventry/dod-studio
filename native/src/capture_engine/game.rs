//! Ending the game a batch ran in: closing an hl.exe the batch started, or
//! leaving one it borrowed (#725) open at its menu.

use crate::log_markdown;

/// Waits, briefly, for `hl.exe` to actually leave the process list after a
/// `taskkill /F` -- `taskkill` returning only means Windows has been asked to
/// end the process, not that it (and its open file handles) are gone yet.
/// `CaptureCleanupGuard`'s `Drop` deletes this batch's demo files the moment
/// the batch's thread returns, right after every `taskkill` call site in the
/// batch's poll loop ends it -- without this wait, the very last demo in the
/// chain can still be open when that delete runs. See #198.
///
/// Bounded to 3 seconds so a process that was already gone when `taskkill`
/// ran (the game crashed on its own, or was closed by the user moments
/// earlier) can never turn this into an extra wait on every batch --
/// `remove_file_retrying` is the second, independent line of defence for
/// whatever this window doesn't cover.
pub(super) fn wait_for_hl_exe_to_exit(sys: &mut sysinfo::System) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        crate::sys::process::refresh(sys);
        if !crate::sys::process::is_game_in(sys) {
            return;
        }
        if std::time::Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Ends every `hl.exe` and waits for it to go, as [`wait_for_hl_exe_to_exit`]
/// describes. Every way out of a batch's loop that leaves the game running
/// goes through here. A killed game can't release the mouse it confined, so
/// once it's gone Studio does (#1095).
pub(super) fn kill_hl_exe_and_wait(sys: &mut sysinfo::System) {
    crate::sys::process::kill_game();
    wait_for_hl_exe_to_exit(sys);
    if !crate::sys::process::is_game_in(sys) && crate::sys::cursor_clip::release() {
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
pub(super) fn end_game(reuse_game: Option<u32>, sys: &mut sysinfo::System) {
    match reuse_game {
        Some(pid) => leave_game(pid),
        None => kill_hl_exe_and_wait(sys),
    }
}
