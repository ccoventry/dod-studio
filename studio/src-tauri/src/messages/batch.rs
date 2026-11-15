// messages/batch.rs
// Activity-log lines around a capture batch's start: the close wait
// (`batch_close.rs`), the manifest file (`manifest_file.rs`) and a demo that
// cannot be trimmed (`trimmed_demos.rs`) (#237). Each function returns the
// line only; the caller writes it to the log.

use std::fmt::Display;

/// The batch's start had not reached the game by the time Studio closed.
pub fn start_not_reached_on_close(secs: u64) -> String {
    format!("[capture] Closing: the batch's start had not reached the game after {secs} s")
}

/// The capture manifest could not be written into `dir`.
pub fn manifest_not_written(dir: impl Display, err: impl Display) -> String {
    format!("Could not write the capture manifest to {dir}: {err}")
}

/// A batch refused because a demo cannot be trimmed; `line` is the toast text.
pub fn batch_not_started(line: impl Display) -> String {
    format!("⛔ **Batch not started** — {line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_lines_keep_their_wording() {
        assert_eq!(
            start_not_reached_on_close(5),
            "[capture] Closing: the batch's start had not reached the game after 5 s"
        );
        assert_eq!(
            manifest_not_written("D:/m", "denied"),
            "Could not write the capture manifest to D:/m: denied"
        );
        assert_eq!(batch_not_started("x"), "⛔ **Batch not started** — x");
    }
}
