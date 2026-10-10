//! The settings a game takes only when it starts, and whether a running game
//! was started with the ones DoD Studio has now (#666).
//!
//! `PatcherConfig::build_hlae_process` puts these on the command line, and
//! [`compare`] reads them back off the running `hl.exe`. Both go through
//! [`LaunchSettings`] and destructure it without `..`, so a launch argument
//! added to the struct fails to compile until both sides handle it.
//!
//! The running game's side comes from its own process: the image path gives
//! the install, and `-w`/`-h` are on its command line because HLAE starts
//! `hl.exe` with the `-cmdLine` text. That works for a game started in an
//! earlier Studio session too, with nothing to record.

use serde::Serialize;
use std::path::PathBuf;

/// What `build_hlae_process` launches with that the game only reads at start.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSettings {
    /// `hl.exe`, passed as `-programPath`: which install the game runs from.
    pub game_path: String,
    /// `-w` on the game's command line.
    pub width: i32,
    /// `-h` on the game's command line.
    pub height: i32,
    /// HLAE's `AfxHookGoldSrc.dll`, passed as `-hookDllPath`.
    pub afx_hook_dll: PathBuf,
    /// DoD Studio's hook DLL, a second `-hookDllPath` when one is found.
    pub goldsrc_hooks_dll: Option<PathBuf>,
}

/// A launch setting the running game differs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchDifference {
    /// It runs from another `hl.exe`.
    Install,
    /// It was started at another resolution.
    Resolution,
}

/// One side of a mismatch, as the frontend words it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GameLaunch {
    /// The install's folder name, or its whole folder when both sides' folder
    /// names are the same.
    pub install: String,
    /// The `hl.exe` path.
    pub exe: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// Whether a game is running, and whether it was started with the launch
/// settings DoD Studio has now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RunningGameCheck {
    /// No game is running.
    None,
    /// Game `pid` was started with these settings, or with nothing that can
    /// be read as different: reuse it.
    Match { pid: u32 },
    /// Game `pid` was started with other settings, which only take effect
    /// when the game starts again.
    Mismatch {
        pid: u32,
        running: GameLaunch,
        wanted: GameLaunch,
        differs: Vec<LaunchDifference>,
    },
}

/// A running `hl.exe` as read from the process list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunningGame {
    pub pid: u32,
    /// Its image path, when Windows gave one.
    pub exe: Option<String>,
    /// Its command line, one argument per entry.
    pub cmd: Vec<String>,
}

/// Whether a game is running now, and whether it was started with `want`.
#[cfg(not(target_arch = "wasm32"))]
pub fn check_running_game(want: &LaunchSettings) -> RunningGameCheck {
    // Both sides resolved on disk, so a junction or short name in one
    // spelling does not read as another install.
    let canonical = |p: &str| {
        std::fs::canonicalize(unquote(p))
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.to_string())
    };
    let want = LaunchSettings {
        game_path: canonical(&want.game_path),
        ..want.clone()
    };
    let games: Vec<RunningGame> = crate::sys::process::running_games()
        .into_iter()
        .map(|g| RunningGame {
            exe: g.exe.as_deref().map(canonical),
            ..g
        })
        .collect();
    check(&want, &games)
}

/// The answer for `games`: the first one started with `want` is reused, and
/// only when none was does the first one count as a mismatch.
pub fn check(want: &LaunchSettings, games: &[RunningGame]) -> RunningGameCheck {
    let mut first_mismatch = None;
    for game in games {
        match compare(want, game) {
            RunningGameCheck::Match { pid } => return RunningGameCheck::Match { pid },
            mismatch if first_mismatch.is_none() => first_mismatch = Some(mismatch),
            _ => {}
        }
    }
    first_mismatch.unwrap_or(RunningGameCheck::None)
}

/// Compares one running game against `want`. A setting that can't be read off
/// the game (no image path, no `-w`/`-h` because something other than Studio
/// started it) is not counted as different: those games reach the reuse
/// paths' existing "not started by DoD Studio" handling instead.
pub fn compare(want: &LaunchSettings, game: &RunningGame) -> RunningGameCheck {
    let LaunchSettings {
        game_path,
        width,
        height,
        // Not compared: HLAE passes the hook DLLs as `-hookDllPath` on its own
        // command line and exits seconds after injecting them
        // (docs/hlae_protocols.md), so by now only `hl.exe`'s loaded-module
        // list holds them, which needs Windows API calls `native` does not
        // make. A changed hook DLL is not detected.
        afx_hook_dll: _,
        goldsrc_hooks_dll: _,
    } = want;

    let mut differs = Vec::new();
    let running_exe = running_exe(game);
    if let Some(exe) = &running_exe
        && normalise_path(exe) != normalise_path(game_path)
    {
        differs.push(LaunchDifference::Install);
    }
    let running_size = resolution_from_cmd(&game.cmd);
    if let Some(size) = running_size
        && size != (*width, *height)
    {
        differs.push(LaunchDifference::Resolution);
    }

    if differs.is_empty() {
        return RunningGameCheck::Match { pid: game.pid };
    }
    let running_exe = running_exe.unwrap_or_default();
    let (running_install, wanted_install) = install_labels(&running_exe, game_path);
    RunningGameCheck::Mismatch {
        pid: game.pid,
        running: GameLaunch {
            install: running_install,
            exe: clean_path(&running_exe),
            width: running_size.map(|s| s.0),
            height: running_size.map(|s| s.1),
        },
        wanted: GameLaunch {
            install: wanted_install,
            exe: clean_path(game_path),
            width: Some(*width),
            height: Some(*height),
        },
        differs,
    }
}

/// The game's `hl.exe`: its image path, or else the first argument of its
/// command line when that names an `.exe`.
fn running_exe(game: &RunningGame) -> Option<String> {
    game.exe
        .clone()
        .filter(|e| !e.trim().is_empty())
        .or_else(|| {
            let first = game.cmd.first()?;
            let first = unquote(first);
            first
                .to_ascii_lowercase()
                .ends_with(".exe")
                .then(|| first.to_string())
        })
}

/// `-w <W> -h <H>` from a command line. Each argument is also split on
/// whitespace, because a launcher that passes the whole `-cmdLine` text as
/// one argument leaves it unsplit. The first of each counts, as with the
/// engine's own parameter lookup. `None` unless both are there.
pub fn resolution_from_cmd(cmd: &[String]) -> Option<(i32, i32)> {
    let tokens: Vec<&str> = cmd
        .iter()
        .flat_map(|a| a.split_whitespace())
        .map(unquote)
        .collect();
    let value_of = |flag: &str| {
        tokens
            .iter()
            .position(|t| t.eq_ignore_ascii_case(flag))
            .and_then(|i| tokens.get(i + 1))
            .and_then(|v| v.parse::<i32>().ok())
    };
    Some((value_of("-w")?, value_of("-h")?))
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"').trim()
}

/// A path as typed or read, without quotes, the `\\?\` prefix
/// `canonicalize` adds, or forward slashes.
fn clean_path(path: &str) -> String {
    let path = unquote(path);
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        let c = if c == '/' { '\\' } else { c };
        // `build_hlae_process` doubles separators in some paths; Windows
        // reads a run of them as one.
        if c == '\\' && out.ends_with('\\') && out.len() > 1 {
            continue;
        }
        out.push(c);
    }
    out
}

/// A path in the form two spellings of the same file share: Windows paths
/// are case-insensitive.
fn normalise_path(path: &str) -> String {
    clean_path(path).to_lowercase()
}

/// Names for the two installs: each `hl.exe`'s folder name, or the whole
/// folder for both when the names alone would read the same.
fn install_labels(running: &str, wanted: &str) -> (String, String) {
    let folder = |p: &str| {
        let p = clean_path(p);
        p.rsplit_once('\\')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or(p)
    };
    let name = |dir: &str| dir.rsplit('\\').next().unwrap_or(dir).to_string();
    let (running_dir, wanted_dir) = (folder(running), folder(wanted));
    let (running_name, wanted_name) = (name(&running_dir), name(&wanted_dir));
    if running_name.is_empty() || running_name.eq_ignore_ascii_case(&wanted_name) {
        (running_dir, wanted_dir)
    } else {
        (running_name, wanted_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRE: &str = r"C:\Games\Half-Life - PRE-Anniversary for Movies\hl.exe";
    const POST: &str = r"C:\Games\Half-Life - POST-Anniversary for Movies\hl.exe";

    fn want(game_path: &str, width: i32, height: i32) -> LaunchSettings {
        LaunchSettings {
            game_path: game_path.to_string(),
            width,
            height,
            afx_hook_dll: PathBuf::from(r"C:\HLAE\AfxHookGoldSrc.dll"),
            goldsrc_hooks_dll: None,
        }
    }

    /// The command line HLAE starts `hl.exe` with, built the way
    /// `build_hlae_process` builds `-cmdLine`.
    fn game(pid: u32, exe: &str, width: i32, height: i32) -> RunningGame {
        let mut cmd = vec![format!("\"{exe}\"")];
        cmd.extend(
            format!("-game dod -insecure -addons -demoedit -windowed -w {width} -h {height} -gl -condebug")
                .split(' ')
                .map(String::from),
        );
        RunningGame {
            pid,
            exe: Some(exe.to_string()),
            cmd,
        }
    }

    #[test]
    fn the_same_install_and_resolution_is_reused() {
        assert_eq!(
            compare(&want(PRE, 1920, 1080), &game(7, PRE, 1920, 1080)),
            RunningGameCheck::Match { pid: 7 }
        );
    }

    #[test]
    fn paths_compare_without_case_slashes_quotes_or_the_verbatim_prefix() {
        let typed = "c:/games/half-life - pre-anniversary for movies/HL.EXE";
        let mut running = game(7, &format!(r"\\?\{PRE}"), 1920, 1080);
        assert_eq!(
            compare(&want(typed, 1920, 1080), &running),
            RunningGameCheck::Match { pid: 7 }
        );
        running.exe = Some(format!("\"{PRE}\""));
        let doubled = PRE.replace('\\', r"\\");
        assert_eq!(
            compare(&want(&doubled, 1920, 1080), &running),
            RunningGameCheck::Match { pid: 7 }
        );
    }

    #[test]
    fn another_install_and_resolution_are_both_named() {
        let RunningGameCheck::Mismatch {
            pid,
            running,
            wanted,
            differs,
        } = compare(&want(POST, 3440, 1440), &game(7, PRE, 1920, 1080))
        else {
            panic!("expected a mismatch");
        };
        assert_eq!(pid, 7);
        assert_eq!(
            differs,
            vec![LaunchDifference::Install, LaunchDifference::Resolution]
        );
        assert_eq!(running.install, "Half-Life - PRE-Anniversary for Movies");
        assert_eq!(wanted.install, "Half-Life - POST-Anniversary for Movies");
        assert_eq!(running.exe, PRE);
        assert_eq!((running.width, running.height), (Some(1920), Some(1080)));
        assert_eq!((wanted.width, wanted.height), (Some(3440), Some(1440)));
    }

    #[test]
    fn a_resolution_change_alone_is_a_mismatch() {
        let check = compare(&want(PRE, 2560, 1440), &game(7, PRE, 1920, 1080));
        assert!(matches!(
            check,
            RunningGameCheck::Mismatch { ref differs, .. } if differs == &[LaunchDifference::Resolution]
        ));
    }

    #[test]
    fn same_folder_names_fall_back_to_the_whole_folder() {
        let a = r"C:\Steam\steamapps\common\Half-Life\hl.exe";
        let b = r"D:\Steam\steamapps\common\Half-Life\hl.exe";
        assert_eq!(
            install_labels(a, b),
            (
                r"C:\Steam\steamapps\common\Half-Life".to_string(),
                r"D:\Steam\steamapps\common\Half-Life".to_string()
            )
        );
    }

    #[test]
    fn resolution_is_read_from_split_or_whole_command_lines() {
        let split: Vec<String> = ["hl.exe", "-w", "1920", "-h", "1080"]
            .map(String::from)
            .to_vec();
        assert_eq!(resolution_from_cmd(&split), Some((1920, 1080)));
        let whole = vec![
            PRE.to_string(),
            "-game dod -windowed -w 3440 -h 1440 -gl".to_string(),
        ];
        assert_eq!(resolution_from_cmd(&whole), Some((3440, 1440)));
        let quoted = vec![
            "-w".to_string(),
            "\"800\"".to_string(),
            "-H".to_string(),
            "600".to_string(),
        ];
        assert_eq!(resolution_from_cmd(&quoted), Some((800, 600)));
        // The engine reads the first of each.
        let twice: Vec<String> = ["-w", "1024", "-h", "768", "-w", "640"]
            .map(String::from)
            .to_vec();
        assert_eq!(resolution_from_cmd(&twice), Some((1024, 768)));
    }

    #[test]
    fn a_missing_or_bad_resolution_reads_as_unknown() {
        let none: Vec<String> = ["hl.exe", "-game", "dod"].map(String::from).to_vec();
        assert_eq!(resolution_from_cmd(&none), None);
        let half: Vec<String> = ["-w", "1920"].map(String::from).to_vec();
        assert_eq!(resolution_from_cmd(&half), None);
        let bad: Vec<String> = ["-w", "wide", "-h", "1080"].map(String::from).to_vec();
        assert_eq!(resolution_from_cmd(&bad), None);
        let trailing: Vec<String> = ["-h", "1080", "-w"].map(String::from).to_vec();
        assert_eq!(resolution_from_cmd(&trailing), None);
    }

    /// A game Studio didn't start has no `-w`/`-h`; only the install is
    /// compared, so the reuse paths' own handling of such a game still runs.
    #[test]
    fn an_unreadable_setting_is_not_a_difference() {
        let steam_started = RunningGame {
            pid: 3,
            exe: Some(PRE.to_string()),
            cmd: vec![PRE.to_string(), "-game".into(), "dod".into()],
        };
        assert_eq!(
            compare(&want(PRE, 1920, 1080), &steam_started),
            RunningGameCheck::Match { pid: 3 }
        );
        let unreadable = RunningGame {
            pid: 4,
            ..Default::default()
        };
        assert_eq!(
            compare(&want(PRE, 1920, 1080), &unreadable),
            RunningGameCheck::Match { pid: 4 }
        );
    }

    #[test]
    fn without_an_image_path_the_quoted_first_argument_names_the_install() {
        let mut running = game(5, PRE, 1920, 1080);
        running.exe = None;
        assert!(matches!(
            compare(&want(POST, 1920, 1080), &running),
            RunningGameCheck::Mismatch { ref differs, .. } if differs == &[LaunchDifference::Install]
        ));
        assert_eq!(
            compare(&want(PRE, 1920, 1080), &running),
            RunningGameCheck::Match { pid: 5 }
        );
    }

    #[test]
    fn a_matching_game_wins_over_a_mismatched_one() {
        let w = want(PRE, 1920, 1080);
        assert_eq!(check(&w, &[]), RunningGameCheck::None);
        let games = [game(1, POST, 1920, 1080), game(2, PRE, 1920, 1080)];
        assert_eq!(check(&w, &games), RunningGameCheck::Match { pid: 2 });
        let games = [game(1, POST, 1920, 1080), game(2, POST, 800, 600)];
        assert!(matches!(
            check(&w, &games),
            RunningGameCheck::Mismatch { pid: 1, .. }
        ));
    }

    /// The check reads back what `build_hlae_process` writes: a game started
    /// with a config's own command line matches that config.
    #[test]
    fn a_game_started_by_build_hlae_process_matches_its_config() {
        let cfg = crate::patch::PatcherConfig {
            game_path: PRE.to_string(),
            resolution_width: 2560,
            resolution_height: 1440,
            ..Default::default()
        };
        let cmd = cfg.build_hlae_process("+viewdemo x");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = args.iter().position(|a| a == "-cmdLine").unwrap();
        let running = RunningGame {
            pid: 9,
            exe: Some(PRE.to_string()),
            cmd: vec![PRE.to_string(), args[at + 1].clone()],
        };
        assert_eq!(
            compare(&cfg.launch_settings(), &running),
            RunningGameCheck::Match { pid: 9 }
        );
        let mut other = cfg.clone();
        other.resolution_width = 1920;
        assert!(matches!(
            compare(&other.launch_settings(), &running),
            RunningGameCheck::Mismatch { .. }
        ));
    }
}
