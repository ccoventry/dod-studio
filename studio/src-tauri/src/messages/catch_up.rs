// messages/catch_up.rs
// Activity-log lines of the catch-up for a batch that ran while Studio was
// closed (`batch_catch_up.rs`, `catch_up_takes.rs`). Each function returns the
// line only; the caller still writes it to the log (#237).

use std::fmt::Display;

// ── catch_up_takes.rs ────────────────────────────────────────────────────

/// The game's console log holds no run of the batch.
pub fn console_log_has_no_run(session: impl Display, log_path: impl Display) -> String {
    format!(
        "[batch-catch-up] session {session}: the game's console log at {log_path} has no run of this batch; \
         judging its takes by what they hold"
    )
}

/// A clip that was recording when the game closed.
pub fn recording_when_game_closed(take_key: impl Display, take_folder: impl Display) -> String {
    format!(
        "[batch-catch-up] {take_key} was recording when the game closed (its console log has no \
         STOP_RECORD for it): not counted as captured. Its partial take is left at {take_folder}"
    )
}

/// A take found unfinished on disk after the game closed.
pub fn cut_short_when_game_closed(take_key: impl Display, take_folder: impl Display) -> String {
    format!(
        "[batch-catch-up] {take_key} was cut short when the game closed: not counted as captured. Its partial take is left at {take_folder}"
    )
}

/// A take cut short for the reason in `why`.
pub fn cut_short_because(
    take_key: impl Display,
    why: impl Display,
    take_folder: impl Display,
) -> String {
    format!(
        "[batch-catch-up] {take_key} {why}: not counted as captured. The take is left at {take_folder}"
    )
}

// ── batch_catch_up.rs ────────────────────────────────────────────────────

/// What a session that ran while Studio was closed left on disk.
pub fn session_ran_while_closed(session: impl Display, captured: usize, total: usize) -> String {
    format!(
        "[batch-catch-up] session {session} ran while Studio was closed: {captured}/{total} takes on disk"
    )
}

/// Running games that cannot be capturing a batch.
pub fn games_without_hlae(count: usize) -> String {
    format!(
        "[batch-catch-up] {count} running game(s) have no HLAE in them, so they can't be capturing a batch: not waiting for them"
    )
}

/// Batches to check once the game closes.
pub fn checking_batches_once_game_closes(count: usize) -> String {
    format!(
        "[batch-catch-up] {count} batch(es) ran while Studio was closed; checking their takes once the game closes"
    )
}

/// The window could not be told the checks are over.
pub fn could_not_tell_window_checked(err: impl Display) -> String {
    format!("[batch-catch-up] Could not tell the window the batch's clips were checked: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_up_lines_match_their_original_inline_text() {
        assert_eq!(
            console_log_has_no_run("s1", "D:/hl/qconsole.log"),
            "[batch-catch-up] session s1: the game's console log at D:/hl/qconsole.log has no run of this batch; judging its takes by what they hold"
        );
        assert_eq!(
            recording_when_game_closed("k", "D:/t"),
            "[batch-catch-up] k was recording when the game closed (its console log has no STOP_RECORD for it): not counted as captured. Its partial take is left at D:/t"
        );
        assert_eq!(
            cut_short_when_game_closed("k", "D:/t"),
            "[batch-catch-up] k was cut short when the game closed: not counted as captured. Its partial take is left at D:/t"
        );
        assert_eq!(
            cut_short_because("k", "was cut short", "D:/t"),
            "[batch-catch-up] k was cut short: not counted as captured. The take is left at D:/t"
        );
        assert_eq!(
            session_ran_while_closed("s1", 2, 3),
            "[batch-catch-up] session s1 ran while Studio was closed: 2/3 takes on disk"
        );
        assert_eq!(
            games_without_hlae(2),
            "[batch-catch-up] 2 running game(s) have no HLAE in them, so they can't be capturing a batch: not waiting for them"
        );
        assert_eq!(
            checking_batches_once_game_closes(1),
            "[batch-catch-up] 1 batch(es) ran while Studio was closed; checking their takes once the game closes"
        );
        assert_eq!(
            could_not_tell_window_checked("gone"),
            "[batch-catch-up] Could not tell the window the batch's clips were checked: gone"
        );
    }
}
