// messages/scan.rs
// Activity-log lines of the demo scan and the demo cache (`scan_manager.rs`,
// `demo_cache_cmd.rs`) (#237).

use std::fmt::Display;

/// Scan paths that no longer exist, comma-separated in `paths`.
pub fn skipped_missing_paths(paths: impl Display) -> String {
    format!("[scan] Skipped paths that no longer exist: {paths}")
}

/// A demo the scan could not read.
pub fn skipped_demo(file: impl Display, reason: impl Display) -> String {
    format!("[scan] Skipped {file}: {reason}")
}

pub fn cache_demos_started(count: usize) -> String {
    format!("Cache demos: {count} demo(s)")
}

/// The cache run's closing line.
pub fn cache_demos_done(
    done: usize,
    total: usize,
    upgraded: usize,
    already: usize,
    failed: usize,
    cancelled: bool,
) -> String {
    format!(
        "Cache demos: {done} of {total} done ({upgraded} upgraded, {already} already cached, {failed} failed){}",
        if cancelled { ", stopped" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_lines_keep_their_wording() {
        assert_eq!(
            skipped_missing_paths("a, b"),
            "[scan] Skipped paths that no longer exist: a, b"
        );
        assert_eq!(skipped_demo("d.dem", "bad"), "[scan] Skipped d.dem: bad");
        assert_eq!(cache_demos_started(3), "Cache demos: 3 demo(s)");
        assert_eq!(
            cache_demos_done(3, 3, 1, 1, 0, true),
            "Cache demos: 3 of 3 done (1 upgraded, 1 already cached, 0 failed), stopped"
        );
        assert!(cache_demos_done(1, 1, 0, 1, 0, false).ends_with("0 failed)"));
    }
}
