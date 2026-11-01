// messages/take_cleanup.rs
// Activity-log lines of the take cleanup (`take_cleanup_manager.rs`): what was
// removed after a finish, what stays and why, and the making-room outcomes
// (#237). Each function returns the line only; the caller still writes it to
// the log.

use std::fmt::Display;

/// A take moved to the Recycle Bin.
pub fn take_cleanup_recycled(take_folder: impl Display, size: &str, because: &str) -> String {
    format!("[take-cleanup] Moved {take_folder} to the Recycle Bin ({size}): {because}.")
}

/// A take deleted for good.
pub fn take_cleanup_deleted(take_folder: impl Display, size: &str, because: &str) -> String {
    format!("[take-cleanup] Deleted {take_folder} for good ({size}): {because}.")
}

/// A take whose removal failed; `error` is the removal's own text.
pub fn take_cleanup_failed(error: impl Display) -> String {
    format!("[take-cleanup] {error}")
}

/// A take left alone because its render history doesn't show it finished.
pub fn take_cleanup_stays_unfinished(take_folder: impl Display) -> String {
    format!(
        "[take-cleanup] {take_folder} stays: its render history doesn't show every stream finished with the clip file still there."
    )
}

/// A take left alone for the reason in `why` (a refused deletion or a list
/// that wouldn't let go of it).
pub fn take_cleanup_stays(take_folder: impl Display, why: impl Display) -> String {
    format!("[take-cleanup] {take_folder} stays: {why}")
}

/// Making room found the free space unreadable.
pub fn take_cleanup_room_unknown(job: impl Display) -> String {
    format!(
        "[take-cleanup] Making room for {job}: the free space on its drives can't be read, so no takes were deleted."
    )
}

/// Making room can't reach the space even by deleting every finished take.
pub fn take_cleanup_room_short(
    job: impl Display,
    needed: impl Display,
    available: impl Display,
    moved_takes_note: impl Display,
) -> String {
    format!(
        "[take-cleanup] Making room for {job}: it needs {needed} free on one drive, and deleting every finished take on its drives would leave at most {available} on any one.{moved_takes_note} No takes were deleted."
    )
}

/// Making room stopped by the user part-way.
pub fn take_cleanup_room_cancelled(job: impl Display, done: usize, total: usize) -> String {
    format!(
        "[take-cleanup] Making room for {job} stopped: cancelled after {done} of {total} takes."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_cleanup_lines_match_their_original_inline_text() {
        assert_eq!(
            take_cleanup_recycled("D:/takes/a", "1.5 GB", "the render finished"),
            "[take-cleanup] Moved D:/takes/a to the Recycle Bin (1.5 GB): the render finished."
        );
        assert_eq!(
            take_cleanup_stays("D:/takes/a", "outside Destinations"),
            "[take-cleanup] D:/takes/a stays: outside Destinations"
        );
        assert_eq!(
            take_cleanup_room_cancelled("a capture", 2, 5),
            "[take-cleanup] Making room for a capture stopped: cancelled after 2 of 5 takes."
        );
    }
}
