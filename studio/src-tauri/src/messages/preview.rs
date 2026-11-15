// messages/preview.rs
// Activity-log lines and errors of the hook-driven runs: the preview in the
// original demo (`preview_in_place.rs`) and the batch the hook DLL plays
// (`hook_batch.rs`) (#237). Each function returns the text only; the caller
// still writes it to the log.

use std::fmt::{Debug, Display};

// ── preview_in_place.rs ──────────────────────────────────────────────────

/// The game launched for a preview of `arg`, with no patched copy.
pub fn preview_launched_in_place(arg: &str) -> String {
    format!("[preview] in place: launched the game for \"{arg}\" (no patched copy)")
}

/// A console line (or lines) sent to the game `pid`; `sent` is the send's
/// outcome as the log prints it.
pub fn preview_sent_to_game(lines: impl Display, pid: u32, sent: impl Debug) -> String {
    format!("[preview] in place: {lines} -> pid {pid}: {sent:?}")
}

/// The preview's `viewdemo` line reached an already running game.
pub fn preview_sent_to_running_game(viewdemo: &str, pid: u32) -> String {
    format!("[preview] in place: sent \"{viewdemo}\" to the running game (pid {pid})")
}

/// The game never opened its command pipe.
pub const PREVIEW_PIPE_NEVER_OPENED: &str =
    "[preview] in place: the game never opened its command pipe; the preview was not started";

// ── hook_batch.rs ────────────────────────────────────────────────────────

/// A demo read again to fill in the demo-player times of its kills.
pub fn read_again_filled(name: &str, filled: usize) -> String {
    format!(
        "🔎 **Read again** — `{name}` had no demo-player times for its kills (a project saved before they existed); {filled} highlight(s) have them now"
    )
}

/// A demo that could not be read again.
pub fn read_again_failed(name: &str, err: impl Display) -> String {
    format!(
        "⚠️ **Read again** — `{name}` could not be read for its kills' demo-player times: {err}"
    )
}

/// Stale demo copies of an earlier batch removed.
pub fn stale_demo_copies_removed(swept: usize, dod_dir: impl Display) -> String {
    format!(
        "🧹 **Cleanup** — removed {swept} demo cop{} an earlier batch left in `{dod_dir}`",
        if swept == 1 { "y" } else { "ies" },
    )
}

/// The batch needs the hook DLL and Studio has none.
pub const HOOK_DLL_NOT_INSTALLED_FOR_BATCH: &str =
    "the DoD Studio hook DLL isn't installed, and only it can run a batch this way";

/// The batch file could not be written.
pub fn could_not_write_batch_file(path: impl Display, err: impl Display) -> String {
    format!("could not write the batch file {path}: {err}")
}

/// The hook plays the original demos and seeks; nothing is patched.
pub fn seek_between_clips_batch(batch_file: impl Display) -> String {
    format!(
        "⏩ **Seek Between Clips** — the hook DLL plays the original demos and seeks to each clip; \
         nothing is patched. Batch file: `{batch_file}`"
    )
}

/// A demo on another drive than the game, played by its full path.
pub fn demo_played_in_place(demo: impl Display) -> String {
    format!(
        "💽 **Played in place** — `{demo}` is on another drive than the game; the hook DLL opens it by its full path, nothing is copied"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_lines_match_their_original_inline_text() {
        assert_eq!(
            preview_launched_in_place("a.dem"),
            "[preview] in place: launched the game for \"a.dem\" (no patched copy)"
        );
        assert_eq!(
            preview_sent_to_game("viewdemo a", 7, Some(1)),
            "[preview] in place: viewdemo a -> pid 7: Some(1)"
        );
        assert_eq!(
            preview_sent_to_running_game("viewdemo a", 7),
            "[preview] in place: sent \"viewdemo a\" to the running game (pid 7)"
        );
        assert!(PREVIEW_PIPE_NEVER_OPENED.ends_with("the preview was not started"));
    }

    #[test]
    fn hook_batch_lines_match_their_original_inline_text() {
        assert_eq!(
            stale_demo_copies_removed(1, "D:/dod"),
            "🧹 **Cleanup** — removed 1 demo copy an earlier batch left in `D:/dod`"
        );
        assert_eq!(
            stale_demo_copies_removed(3, "D:/dod"),
            "🧹 **Cleanup** — removed 3 demo copies an earlier batch left in `D:/dod`"
        );
        assert_eq!(
            read_again_failed("a.dem", "bad"),
            "⚠️ **Read again** — `a.dem` could not be read for its kills' demo-player times: bad"
        );
        assert_eq!(
            could_not_write_batch_file("b.txt", "denied"),
            "could not write the batch file b.txt: denied"
        );
        assert!(seek_between_clips_batch("b.txt").ends_with("Batch file: `b.txt`"));
        assert!(
            demo_played_in_place("x.dem").starts_with("💽 **Played in place** — `x.dem` is on")
        );
        assert!(read_again_filled("a.dem", 2).contains("; 2 highlight(s) have them now"));
    }
}
