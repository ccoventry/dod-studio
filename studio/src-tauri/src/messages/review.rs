// messages/review.rs
// Activity-log lines of a review from the game (`review_manager.rs`) (#237).

use std::fmt::Display;

pub fn game_closed(pid: u32) -> String {
    format!("[review] game {pid} has closed; no more answers to read")
}

pub fn reading_answers(pid: u32) -> String {
    format!("[review] reading answers from game {pid}")
}

/// One answer line the game sent, trimmed.
pub fn answer_line(line: impl Display) -> String {
    format!("[review] {line}")
}

pub fn highlights_sent(count: impl Display, pid: u32, launched: bool) -> String {
    format!("[review] {count} highlight(s) sent to game {pid} (launched: {launched})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_lines_keep_their_wording() {
        assert_eq!(
            game_closed(4),
            "[review] game 4 has closed; no more answers to read"
        );
        assert_eq!(reading_answers(4), "[review] reading answers from game 4");
        assert_eq!(answer_line("ended"), "[review] ended");
        assert_eq!(
            highlights_sent(2, 4, true),
            "[review] 2 highlight(s) sent to game 4 (launched: true)"
        );
    }
}
