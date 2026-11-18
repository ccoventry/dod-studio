// messages/vegas.rs
// Activity-log lines of installing the VEGAS scripts (`vegas_manager.rs`)
// (#237, #1514).

use std::fmt::Display;

pub const INSTALLING_SCRIPT: &str =
    "[vegas] installing the nameplate and relink scripts into the script menu";

pub fn script_installed(count: usize, folder: impl Display, ms: u128) -> String {
    format!("[vegas] {count} scripts installed in {folder} in {ms} ms")
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
            script_installed(2, "D:/menu", 12),
            "[vegas] 2 scripts installed in D:/menu in 12 ms"
        );
        assert_eq!(
            script_install_failed("no"),
            "[vegas] script install failed: no"
        );
        assert!(INSTALLING_SCRIPT.starts_with("[vegas] installing"));
    }
}
