// messages/hd.rs
// Activity-log lines of the HD textures page's commands (`hd_manager.rs`):
// the tool download, the build, the Python and upscaler choices, the styles
// and the map list (#237). Each function returns the line only; the caller
// still writes it to the log.

use std::fmt::Display;

pub const HD_TOOL_DOWNLOAD_STARTED: &str = "[hd] tool download started";

pub const HD_TOOLS_ALREADY_THERE: &str = "[hd] tools already there, nothing to download";

pub fn hd_tool_download_done(files: usize) -> String {
    format!("[hd] tool download done: {files} file(s)")
}

pub fn hd_tool_download_stopped(err: impl Display) -> String {
    format!("[hd] tool download stopped: {err}")
}

pub fn hd_build_started(styles: &str, types: &str, cap: impl Display) -> String {
    format!("[hd] build started: styles {styles}, types {types}, up to {cap} px")
}

pub fn hd_build_done(steps: impl Display, secs: impl Display, details: impl Display) -> String {
    format!("[hd] build done: {steps} step(s) in {secs} s, details in {details}")
}

pub fn hd_build_stopped(err: impl Display) -> String {
    format!("[hd] build stopped: {err}")
}

pub fn hd_python_set(path: impl Display) -> String {
    format!("[hd] Python set to {path}")
}

pub const HD_PYTHON_AUTOMATIC: &str = "[hd] Python set back to automatic";

pub fn hd_upscaler_set(folder: impl Display) -> String {
    format!("[hd] upscaler folder set to {folder}")
}

pub const HD_UPSCALER_AUTOMATIC: &str = "[hd] upscaler folder set back to automatic";

pub fn hd_style_saved(name: &str) -> String {
    format!("[hd] saved style \"{name}\"")
}

pub fn hd_style_removed(name: &str) -> String {
    format!("[hd] removed style \"{name}\"")
}

pub const HD_MAP_LIST_SAVED: &str = "[hd] map list saved";

pub const HD_MAP_LIST_CLEARED: &str = "[hd] map list cleared, every map is built";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_lines_match_their_original_inline_text() {
        assert_eq!(
            hd_build_started("a, b", "c", 2048),
            "[hd] build started: styles a, b, types c, up to 2048 px"
        );
        assert_eq!(hd_style_saved("Crisp"), "[hd] saved style \"Crisp\"");
        assert_eq!(
            HD_MAP_LIST_CLEARED,
            "[hd] map list cleared, every map is built"
        );
    }
}
