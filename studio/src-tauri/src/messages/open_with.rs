// messages/open_with.rs
// Errors and the activity-log line of Settings' "Open With for .dem files"
// (`dem_open_with.rs`, #528). Each function returns the text only; the caller
// still writes it to the log.

use std::fmt::Display;

/// The Open With entries are registry-only, so other platforms have none.
#[cfg(not(windows))]
pub const OPEN_WITH_WINDOWS_ONLY: &str = "Open With for .dem files is Windows-only";

pub fn could_not_find_own_exe(err: impl Display) -> String {
    format!("Could not find DoD Studio's own exe: {err}")
}

pub fn could_not_open_classes_key(err: impl Display) -> String {
    format!("Could not open HKCU\\Software\\Classes: {err}")
}

pub fn could_not_add_open_with(err: impl Display) -> String {
    format!("Could not add DoD Studio to Open With for .dem files: {err}")
}

pub fn could_not_remove_open_with(err: impl Display) -> String {
    format!("Could not remove DoD Studio from Open With for .dem files: {err}")
}

/// The toggle was applied; `exe` is the program the entry points at.
pub fn open_with_changed(enabled: bool, exe: impl Display) -> String {
    format!(
        "📂 **Open With** — DoD Studio {} for .dem files ({exe})",
        if enabled { "listed" } else { "removed" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_with_text_matches_its_original_inline_text() {
        assert_eq!(
            could_not_open_classes_key("denied"),
            "Could not open HKCU\\Software\\Classes: denied"
        );
        assert_eq!(
            could_not_add_open_with("x"),
            "Could not add DoD Studio to Open With for .dem files: x"
        );
        assert_eq!(
            open_with_changed(false, "C:/a.exe"),
            "📂 **Open With** — DoD Studio removed for .dem files (C:/a.exe)"
        );
        #[cfg(not(windows))]
        assert_eq!(
            OPEN_WITH_WINDOWS_ONLY,
            "Open With for .dem files is Windows-only"
        );
    }
}
