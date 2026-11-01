// messages/vegas.rs
// Activity-log lines of installing the VEGAS nameplate script
// (`vegas_manager.rs`) (#237).

use std::fmt::Display;

pub const INSTALLING_SCRIPT: &str = "[vegas] installing the nameplate script into the script menu";

pub fn script_installed(path: impl Display, ms: u128) -> String {
    format!("[vegas] script installed at {path} in {ms} ms")
}

pub fn script_install_failed(err: impl Display) -> String {
    format!("[vegas] script install failed: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vegas_lines_keep_their_wording() {
        assert_eq!(
            script_installed("D:/s.cs", 12),
            "[vegas] script installed at D:/s.cs in 12 ms"
        );
        assert_eq!(
            script_install_failed("no"),
            "[vegas] script install failed: no"
        );
        assert!(INSTALLING_SCRIPT.starts_with("[vegas] installing"));
    }
}
