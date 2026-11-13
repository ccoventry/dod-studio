// messages/windows_drive.rs
// "Keep free on the Windows drive" (#1430, `sys::windows_drive`): the
// activity-log lines when a folder on that drive is passed over for it, and
// the note a refusal adds so the player knows which setting to change.

use crate::hlcr::take_cleanup::format_gb;
use std::fmt::Display;

/// The setting's name and where it is, as the player sees it.
pub const SETTING: &str = "Keep free on the Windows drive, in Configuration → Destinations";

/// Added to a refusal (no render or capture folder with room) when a folder
/// on the Windows drive was passed over for the setting. Starts with a space.
pub fn kept_free_note(keep_free: u64) -> String {
    format!(
        " The Windows drive is used only while it keeps {} free ({SETTING}): lower that, or add a folder on another drive.",
        format_gb(keep_free)
    )
}

/// A render passed over an export folder on the Windows drive that had room
/// for it but for the setting.
pub fn render_folder_skipped(
    job_id: impl Display,
    dir: impl Display,
    room: u64,
    keep_free: u64,
) -> String {
    format!(
        "[windows-drive] Render {job_id} skipped {dir}: the Windows drive has {} free once other renders are counted, and keeps {} free ({SETTING})",
        format_gb(room),
        format_gb(keep_free)
    )
}

/// A capture batch is planned with a capture folder on the Windows drive.
pub fn capture_folder_on_windows_drive(dir: impl Display, free: u64, keep_free: u64) -> String {
    format!(
        "[windows-drive] Capture folder {dir} is on the Windows drive ({} free): clips are recorded there only while it keeps {} free ({SETTING})",
        format_gb(free),
        format_gb(keep_free)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn the_note_names_the_setting_and_its_size() {
        let note = kept_free_note(50 * GB);
        assert!(note.starts_with(' '));
        assert!(note.contains("50.0 GB"), "{note}");
        assert!(note.contains("Keep free on the Windows drive"), "{note}");
    }

    #[test]
    fn the_render_skip_line_names_the_folder_and_both_sizes() {
        let line = render_folder_skipped("job-1", r"C:\Exports", 40 * GB, 50 * GB);
        assert!(line.contains(r"C:\Exports"), "{line}");
        assert!(
            line.contains("40.0 GB") && line.contains("50.0 GB"),
            "{line}"
        );
    }
}
