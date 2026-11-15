// messages/overview.rs
// Activity-log lines of the Overviews page (`overview_manager.rs`) (#237).

use std::fmt::Display;

/// The page's edits for `map` were saved.
pub fn edits_saved(map: impl Display) -> String {
    format!("[overview] saved edits for {map}")
}

/// The page's edits for `map` were reset.
pub fn edits_reset(map: impl Display) -> String {
    format!("[overview] reset edits for {map}")
}

/// The high-quality copy of `map` was written to `path`.
pub fn hd_saved(map: impl Display, width: u32, height: u32, path: impl Display) -> String {
    format!("[overview] saved the high-quality {map} ({width}x{height}): {path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_lines_keep_their_wording() {
        assert_eq!(edits_saved("m"), "[overview] saved edits for m");
        assert_eq!(edits_reset("m"), "[overview] reset edits for m");
        assert_eq!(
            hd_saved("m", 4096, 3072, "D:/m_hd.tga"),
            "[overview] saved the high-quality m (4096x3072): D:/m_hd.tga"
        );
    }
}
