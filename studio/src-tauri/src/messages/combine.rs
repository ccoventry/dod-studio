// messages/combine.rs
// Activity-log line of joining clips (`combine_manager.rs`) (#237).

use std::fmt::Display;

/// A join started over `clips` clips.
pub fn combine_started(clips: impl Display) -> String {
    format!("[combine] started: {clips} clips")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn started_line_keeps_its_wording() {
        assert_eq!(combine_started(3), "[combine] started: 3 clips");
    }
}
