// messages/update.rs
// Activity-log lines of the in-app updater (`updater_manager.rs`): the check
// on a channel and the download and install of what it found (#237). Each
// function returns the line only; the caller still writes it to the log.

use std::fmt::Display;

/// An update check that could not read the channel's feed.
pub fn update_check_failed(channel: &str, err: impl Display) -> String {
    format!("[update] check on {channel} failed: {err}")
}

/// A finished update check: the versions when one was found.
pub fn update_check_result(channel: &str, found: Option<(&str, &str)>) -> String {
    match found {
        Some((current, new)) => {
            format!("[update] {channel} channel: {new} is available (running {current})")
        }
        None => format!("[update] {channel} channel: up to date"),
    }
}

/// The download of `version` starting.
pub fn update_downloading(version: &str) -> String {
    format!("[update] downloading {version}")
}

/// The download failed.
pub fn update_download_failed(err: impl Display) -> String {
    format!("[update] download failed: {err}")
}

/// The download is whole and the installer is next.
pub const UPDATE_DOWNLOADED_INSTALLING: &str = "[update] downloaded, installing";

/// The install failed.
pub fn update_install_failed(err: impl Display) -> String {
    format!("[update] install failed: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_lines_match_their_original_inline_text() {
        assert_eq!(
            update_check_failed("stable", "timeout"),
            "[update] check on stable failed: timeout"
        );
        assert_eq!(
            update_check_result("dev", Some(("1.0.0", "1.1.0"))),
            "[update] dev channel: 1.1.0 is available (running 1.0.0)"
        );
        assert_eq!(
            update_check_result("dev", None),
            "[update] dev channel: up to date"
        );
        assert_eq!(update_downloading("1.1.0"), "[update] downloading 1.1.0");
        assert_eq!(update_download_failed("x"), "[update] download failed: x");
        assert_eq!(update_install_failed("x"), "[update] install failed: x");
        assert_eq!(
            UPDATE_DOWNLOADED_INSTALLING,
            "[update] downloaded, installing"
        );
    }
}
