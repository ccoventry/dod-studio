// messages/blender.rs
// Activity-log line of a Blender step (`blender_manager.rs`) (#237).

use std::fmt::Display;

/// A step (`import`, `scene`, ...) started for `map`.
pub fn step_started(step: impl Display, map: impl Display) -> String {
    format!("[blender] {step} started for {map}")
}

/// Which Blender a step runs with, from the `--version` the step's own
/// resolve already read; logged once as the step starts (#1492).
pub fn version_used(exe: impl Display, version: Option<&str>) -> String {
    match version {
        Some(v) => format!("[blender] Using Blender {v} ({exe})"),
        None => format!("[blender] Using {exe}: its version could not be read"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_lines_name_the_build_or_say_it_is_unreadable() {
        assert_eq!(
            version_used("Blender/blender.exe", Some("4.4.3")),
            "[blender] Using Blender 4.4.3 (Blender/blender.exe)"
        );
        assert_eq!(
            version_used("blender.exe", None),
            "[blender] Using blender.exe: its version could not be read"
        );
    }

    #[test]
    fn step_line_keeps_its_wording() {
        assert_eq!(
            step_started("import", "dod_x"),
            "[blender] import started for dod_x"
        );
    }
}
