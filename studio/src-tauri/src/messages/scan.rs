// messages/scan.rs
// Activity-log lines of the demo scan and the demo cache (`scan_manager.rs`,
// `demo_cache_cmd.rs`) (#237).

use std::fmt::Display;

/// Scan paths that no longer exist, comma-separated in `paths`.
pub fn skipped_missing_paths(paths: impl Display) -> String {
    format!("[scan] Skipped paths that no longer exist: {paths}")
}

/// How a search for moved demos ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocateEnd {
    Finished,
    TimeCap,
    EntryCap,
}

/// One search for moved demos: how many folders it started from, the deepest
/// folder level it read, how long it took, how it ended and what it found.
pub fn locate_search_line(
    folders: usize,
    depth: usize,
    ms: u128,
    end: LocateEnd,
    found: usize,
    looked_for: usize,
) -> String {
    let ended = match end {
        LocateEnd::Finished => "finished",
        LocateEnd::TimeCap => "stopped early: time limit",
        LocateEnd::EntryCap => "stopped early: entry limit",
    };
    format!(
        "[project] searched {folders} folder(s) for {looked_for} moved demo(s), down to level {depth}, in {ms} ms ({ended}): {found} found"
    )
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
        assert_eq!(
            locate_search_line(2, 3, 41, LocateEnd::TimeCap, 0, 1),
            "[project] searched 2 folder(s) for 1 moved demo(s), down to level 3, in 41 ms (stopped early: time limit): 0 found"
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
