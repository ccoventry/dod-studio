// messages/render.rs
// Activity-log lines of the render history's helpers
// (`capture_history_manager.rs`, `render_autosave.rs`) (#237).

use std::fmt::Display;

/// A lookup of rendered videos that had to search the export folders.
pub fn videos_located(total: usize, moved: usize, missing: usize, ms: u128) -> String {
    format!(
        "[render-locate] {total} rendered video(s): {moved} found again in the export folders, {missing} missing ({ms} ms)"
    )
}

pub fn autosave_write_failed(err: impl Display) -> String {
    format!("[render_autosave] Failed to write lockfile: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_lines_keep_their_wording() {
        assert_eq!(
            videos_located(3, 1, 1, 9),
            "[render-locate] 3 rendered video(s): 1 found again in the export folders, 1 missing (9 ms)"
        );
        assert_eq!(
            autosave_write_failed("denied"),
            "[render_autosave] Failed to write lockfile: denied"
        );
    }
}
