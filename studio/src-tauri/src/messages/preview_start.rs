// messages/preview_start.rs
// Activity-log lines of starting a preview (`demo_preview.rs`,
// `trimmed_demos.rs`) (#237). Each function returns the line only.

use std::fmt::Display;

/// The open game belongs to another running Studio, so the preview stays out.
pub const GAME_STARTED_BY_ANOTHER_STUDIO: &str = "[preview] the open game was started by another DoD Studio that is still open, so the preview does not go to it (#1235)";

/// A `viewdemo` line delivered to the running game `pid`.
pub fn sent_to_running_game(command: impl Display, pid: u32) -> String {
    format!("[preview] sent \"{command}\" to the running game (pid {pid})")
}

/// A preview refused because its demo cannot be trimmed; `line` is the toast text.
pub fn preview_not_started(line: impl Display) -> String {
    format!("⛔ **Preview not started** — {line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_start_lines_keep_their_wording() {
        assert_eq!(
            sent_to_running_game("viewdemo a", 7),
            "[preview] sent \"viewdemo a\" to the running game (pid 7)"
        );
        assert_eq!(preview_not_started("x"), "⛔ **Preview not started** — x");
        assert!(GAME_STARTED_BY_ANOTHER_STUDIO.ends_with("(#1235)"));
    }
}
