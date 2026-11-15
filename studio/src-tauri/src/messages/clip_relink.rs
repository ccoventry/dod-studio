//! Activity-log lines for the DoD Studio tag on rendered clips and the scan
//! that finds moved clips by it (#1442).

use std::fmt::Display;

/// The first finished render of a batch on `drive` whose stream tag could
/// not be written (its in-file tag and fingerprint still find it). Logged
/// once per drive per batch: a FAT/exFAT export drive refuses every render.
pub fn stream_tag_not_written(
    drive: impl Display,
    path: impl Display,
    error: impl Display,
) -> String {
    format!(
        "[clip-tag] couldn't write the DoD Studio stream tag on {path}: {error}; not logged again for {drive} this batch; the in-file tag and size + hash still find these clips"
    )
}

/// The scan starting: how many recorded clips, over how many folders.
pub fn scan_started(clips: usize, folders: usize) -> String {
    format!(
        "[clip-relink] looking for {clips} rendered clip(s) in {folders} folder(s) and their subfolders"
    )
}

/// What a finished scan found, by how.
pub struct ScanCounts {
    pub in_place: usize,
    pub by_file_tag: usize,
    pub by_stream_tag: usize,
    pub by_fingerprint: usize,
    pub missing: usize,
    pub superseded: usize,
    pub files_checked: usize,
    pub tags_written: usize,
    pub tags_failed: usize,
    pub search_complete: bool,
    pub cloud_skipped: usize,
    pub cancelled: bool,
}

/// The scan's end line: counts only, no per-file lines.
pub fn scan_finished(counts: &ScanCounts, ms: u128) -> String {
    let ended = if counts.cancelled {
        "cancelled after"
    } else {
        "done in"
    };
    let stopped = if counts.search_complete {
        ""
    } else {
        "; the folder listing stopped at its limit"
    };
    format!(
        "[clip-relink] {ended} {ms} ms: {} in place, found elsewhere by in-file tag {}, by stream tag {}, by size + hash {}; {} still missing, {} replaced by a later render; {} video file(s) checked, {} online-only file(s) left unread; {} stream tag(s) written, {} couldn't be{stopped}",
        counts.in_place,
        counts.by_file_tag,
        counts.by_stream_tag,
        counts.by_fingerprint,
        counts.missing,
        counts.superseded,
        counts.files_checked,
        counts.cloud_skipped,
        counts.tags_written,
        counts.tags_failed,
    )
}

/// A picked folder that isn't there.
pub fn folder_not_found(path: impl Display) -> String {
    format!("Not searched: the folder {path} wasn't found.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_line_gives_every_count_and_says_when_the_listing_stopped() {
        let mut counts = ScanCounts {
            in_place: 3,
            by_file_tag: 2,
            by_stream_tag: 1,
            by_fingerprint: 1,
            missing: 4,
            superseded: 1,
            files_checked: 50,
            tags_written: 6,
            tags_failed: 0,
            search_complete: true,
            cloud_skipped: 2,
            cancelled: false,
        };
        assert_eq!(
            scan_finished(&counts, 120),
            "[clip-relink] done in 120 ms: 3 in place, found elsewhere by in-file tag 2, by stream tag 1, by size + hash 1; 4 still missing, 1 replaced by a later render; 50 video file(s) checked, 2 online-only file(s) left unread; 6 stream tag(s) written, 0 couldn't be"
        );
        counts.search_complete = false;
        assert!(scan_finished(&counts, 1).ends_with("stopped at its limit"));
        counts.cancelled = true;
        assert!(scan_finished(&counts, 1).starts_with("[clip-relink] cancelled after 1 ms"));
    }

    #[test]
    fn the_start_line_counts_clips_and_folders() {
        assert_eq!(
            scan_started(5, 2),
            "[clip-relink] looking for 5 rendered clip(s) in 2 folder(s) and their subfolders"
        );
    }

    #[test]
    fn the_stream_tag_failure_names_the_drive_it_stops_logging() {
        assert_eq!(
            stream_tag_not_written("E:", "E:/clips/a.mov", "not supported"),
            "[clip-tag] couldn't write the DoD Studio stream tag on E:/clips/a.mov: not supported; not logged again for E: this batch; the in-file tag and size + hash still find these clips"
        );
    }
}
