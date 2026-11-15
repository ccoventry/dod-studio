// messages/blender.rs
// Activity-log line of a Blender step (`blender_manager.rs`) (#237).

use std::fmt::Display;

/// A step (`import`, `scene`, ...) started for `map`.
pub fn step_started(step: impl Display, map: impl Display) -> String {
    format!("[blender] {step} started for {map}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_line_keeps_its_wording() {
        assert_eq!(
            step_started("import", "dod_x"),
            "[blender] import started for dod_x"
        );
    }
}
