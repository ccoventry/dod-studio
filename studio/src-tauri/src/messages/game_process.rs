// messages/game_process.rs
// Activity-log lines of the running-game commands (`game_process_manager.rs`):
// the error-box watch, the running-game check, closing the game and killing
// the engine processes (#237). Each function returns the line only; the
// caller still writes it to the log.

use std::fmt::{Debug, Display};

pub fn launch_error_box(pid: u32, text: impl Display) -> String {
    format!("[launch] Error box from PID {pid}: {text}")
}

/// A running game that was started with other settings than Studio has now.
/// `hook_note` is one of the `hook_*` notes below, or empty.
#[allow(clippy::too_many_arguments)]
pub fn launch_game_other_settings(
    pid: u32,
    differs: impl Debug,
    running_exe: impl Display,
    running_width: impl Debug,
    running_height: impl Debug,
    wanted_exe: impl Display,
    wanted_width: impl Display,
    wanted_height: impl Display,
    hook_note: impl Display,
) -> String {
    format!(
        "[launch] Game {pid} was started with other settings ({differs:?}): {running_exe} at {running_width:?}x{running_height:?}; Studio is set to {wanted_exe} at {wanted_width}x{wanted_height}{hook_note}"
    )
}

/// One side of a hook DLL comparison.
pub fn launch_hook_side(build: &str, dll: &str) -> String {
    format!("hook build {build} ({dll})")
}

pub fn launch_hook_unreadable(wanted_side: impl Display) -> String {
    format!("; which hook DLL the game has could not be read, Studio would launch {wanted_side}")
}

pub fn launch_hook_differs(running_side: impl Display, wanted_side: impl Display) -> String {
    format!("; the game runs {running_side}, Studio would launch {wanted_side}")
}

pub fn launch_batch_reuses_game(pid: u32) -> String {
    format!("[launch] The batch runs in the game that is already open (PID {pid}) (#725)")
}

pub fn launch_open_game_cannot_take_batch(why: impl Display) -> String {
    format!("[launch] The open game can't take the batch — {why}; asking to start it again")
}

pub fn launch_closing_game(pid: u32) -> String {
    format!("[launch] Closing game {pid} to start it with the current launch settings")
}

pub fn engine_kill_failed(pid: impl Display) -> String {
    format!("[engine] Failed to kill engine process pid={pid}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_lines_match_their_original_inline_text() {
        assert_eq!(
            launch_error_box(42, "Could not load"),
            "[launch] Error box from PID 42: Could not load"
        );
        assert_eq!(
            launch_batch_reuses_game(7),
            "[launch] The batch runs in the game that is already open (PID 7) (#725)"
        );
        assert_eq!(
            launch_hook_differs(
                launch_hook_side("1", "a.dll"),
                launch_hook_side("2", "b.dll")
            ),
            "; the game runs hook build 1 (a.dll), Studio would launch hook build 2 (b.dll)"
        );
    }
}
