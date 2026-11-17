//! The third-party builds a launch uses, written to the activity log (#1492).
//! Logging only: no feature checks a version, so a build that can't be read
//! changes nothing but the line.

use std::path::Path;

/// The line [`log_hlae_launch`] writes, for HLAE at `hlae_exe` (whose
/// `AfxHookGoldSrc.dll` sits beside it) and the game at `game_exe`. Reads
/// three files, `hw.dll` among them (a few MB): off the main thread.
pub fn hlae_launch_line(hlae_exe: &Path, game_exe: &Path) -> String {
    let hook_dll = hlae_exe.with_file_name("AfxHookGoldSrc.dll");
    let engine = if game_exe.is_file() {
        crate::sys::installs::build_of(game_exe)
    } else {
        None
    };
    crate::messages::tool_versions::hlae_launch(
        hlae_exe.display(),
        crate::sys::file_version::file_version(hlae_exe).as_deref(),
        crate::sys::file_version::file_version(&hook_dll).as_deref(),
        engine,
    )
}

/// Logs [`hlae_launch_line`] once, just before HLAE is started.
pub fn log_hlae_launch(hlae_exe: &Path, game_exe: &Path) {
    crate::log_markdown(&hlae_launch_line(hlae_exe, game_exe));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_hlae_and_game_log_that_nothing_could_be_read() {
        let line = hlae_launch_line(
            Path::new("no-such-folder/HLAE.exe"),
            Path::new("no-such-folder/hl.exe"),
        );
        assert!(line.contains("(version unreadable); AfxHookGoldSrc.dll (version unreadable)"));
        assert!(line.ends_with("game engine: build unreadable"));
    }

    #[cfg(windows)]
    #[test]
    fn a_readable_hlae_exe_logs_its_file_version() {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set on Windows");
        let cmd_exe = Path::new(&root).join("System32").join("cmd.exe");
        let line = hlae_launch_line(&cmd_exe, Path::new("no-such-folder/hl.exe"));
        assert!(line.contains(" (v"), "{line}");
    }
}
