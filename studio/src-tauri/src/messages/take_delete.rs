// messages/take_delete.rs
// Activity-log lines of deleting takes (`take_delete_manager.rs`) (#237).

use std::fmt::Display;

pub fn moved_to_recycle_bin(folder: impl Display, bytes: u64) -> String {
    format!("[take-delete] moved {folder} ({bytes} bytes) to the Recycle Bin")
}

pub fn delete_failed(error: impl Display) -> String {
    format!("[take-delete] {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_delete_lines_keep_their_wording() {
        assert_eq!(
            moved_to_recycle_bin("D:/t", 10),
            "[take-delete] moved D:/t (10 bytes) to the Recycle Bin"
        );
        assert_eq!(delete_failed("no"), "[take-delete] no");
    }
}
