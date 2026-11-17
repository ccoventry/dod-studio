//! The third-party builds a launch uses, written to the activity log (#1492).
//! Logging only: no feature checks a version, so a build that can't be read
//! changes nothing but the line.

use std::path::Path;

use crate::messages::tool_versions as msg;
use crate::patch::types::PatcherConfig;

/// HLAE's version at `hlae_exe` (its product version, which carries the
/// build's git hash, else its file version) and the engine build of the game
/// at `game_exe`. An empty path is never looked up (a bare file name would
/// be searched along the DLL search path); a missing file reads as `None`.
/// Reads `hw.dll` (a few MB), so call it off the main thread.
fn versions(hlae_exe: &Path, game_exe: &Path) -> (Option<String>, Option<&'static str>) {
    let hlae = if hlae_exe.as_os_str().is_empty() || !hlae_exe.is_file() {
        None
    } else {
        crate::sys::file_version::product_version(hlae_exe)
            .or_else(|| crate::sys::file_version::file_version(hlae_exe))
    };
    let engine = if game_exe.as_os_str().is_empty() || !game_exe.is_file() {
        None
    } else {
        crate::sys::installs::build_of(game_exe)
    };
    (hlae, engine)
}

/// The line [`log_hlae_launch`] writes.
pub fn hlae_launch_line(hlae_exe: &Path, game_exe: &Path) -> String {
    let (hlae, engine) = versions(hlae_exe, game_exe);
    msg::hlae_launch(hlae_exe.display(), hlae.as_deref(), engine)
}

/// Logs HLAE's and the game engine's builds once, just before `config`'s
/// launch starts HLAE.
pub fn log_hlae_launch(config: &PatcherConfig) {
    crate::log_markdown(&hlae_launch_line(
        Path::new(&config.hlae_path),
        Path::new(&config.game_path),
    ));
}

/// Logs the same for a batch that runs in a game that was already open.
pub fn log_reused_game(config: &PatcherConfig) {
    let (hlae_exe, game_exe) = (Path::new(&config.hlae_path), Path::new(&config.game_path));
    let (hlae, engine) = versions(hlae_exe, game_exe);
    crate::log_markdown(&msg::hlae_reused_game(
        hlae_exe.display(),
        hlae.as_deref(),
        engine,
    ));
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
        assert_eq!(
            line,
            "[HLAE] Launching with no-such-folder/HLAE.exe (version unreadable); game engine: build unreadable"
        );
    }

    #[test]
    fn an_empty_path_is_not_looked_up() {
        assert_eq!(versions(Path::new(""), Path::new("")), (None, None));
    }

    #[cfg(windows)]
    #[test]
    fn a_readable_hlae_exe_logs_its_product_version() {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set on Windows");
        let cmd_exe = Path::new(&root).join("System32").join("cmd.exe");
        let expected = crate::sys::file_version::product_version(&cmd_exe)
            .expect("cmd.exe has a ProductVersion");
        let line = hlae_launch_line(&cmd_exe, Path::new("no-such-folder/hl.exe"));
        assert!(line.contains(&format!(" (v{expected}); ")), "{line}");
    }
}
