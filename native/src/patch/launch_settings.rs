//! The settings a game takes only when it starts, and whether a running game
//! was started with the ones DoD Studio has now (#666).
//!
//! `PatcherConfig::build_hlae_process` puts these on the command line, and
//! [`compare`] reads them back off the running `hl.exe`. Both go through
//! [`LaunchSettings`] and destructure it without `..`, so a launch argument
//! added to the struct fails to compile until both sides handle it.
//!
//! The running game's side comes from its own process: the image path gives
//! the install, and `-game`/`-w`/`-h` are on its command line because HLAE
//! starts `hl.exe` with the `-cmdLine` text. That works for a game started in an
//! earlier Studio session too, with nothing to record.
//!
//! The hook DLL is the exception: HLAE passes it on its own command line and
//! exits, so the game's module list says which file it was and the id baked
//! into the DLL says which build (`sys::hook_build`, #1143). The game loads a
//! copy (`sys::hook_copy`, #1146), so the file beside the copy names the one
//! the user picked, and builds are compared by id rather than by path.

use serde::Serialize;
use std::path::PathBuf;

/// What `build_hlae_process` launches with that the game only reads at start.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSettings {
    /// `hl.exe`, passed as `-programPath`: which install the game runs from.
    pub game_path: String,
    /// `-game` on the game's command line: the mod folder beside `hl.exe`
    /// (#402).
    pub game_folder: String,
    /// `-w` on the game's command line.
    pub width: i32,
    /// `-h` on the game's command line.
    pub height: i32,
    /// HLAE's `AfxHookGoldSrc.dll`, passed as `-hookDllPath`.
    pub afx_hook_dll: PathBuf,
    /// DoD Studio's hook DLL, when one is found: the file Studio ships or the
    /// user picked. The second `-hookDllPath` is a copy of it (#1146).
    pub goldsrc_hooks_dll: Option<PathBuf>,
}

/// A DoD Studio hook DLL: its path and the build id baked into it
/// (`sys::hook_build`), `None` for a build from before ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookDll {
    pub path: PathBuf,
    pub build: Option<String>,
}

/// The hook DLL a running game has loaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LoadedHook {
    /// Its modules or the DLL's image could not be read. The default, so a
    /// game nobody looked at is never taken to have the right DLL.
    #[default]
    Unreadable,
    /// It has none: started by something other than DoD Studio.
    Absent,
    Loaded(HookDll),
}

/// A launch setting the running game differs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchDifference {
    /// It runs from another `hl.exe`.
    Install,
    /// It was started at another resolution.
    Resolution,
    /// It runs another game folder (`-game`, #402).
    GameFolder,
    /// It has the hook DLL from another path (#1143).
    HookDll,
    /// It has another build of the hook DLL at the same path: the file was
    /// replaced while the game ran (#1143).
    HookBuild,
    /// Which hook DLL it has could not be read (#1143).
    HookUnreadable,
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
    /// `-game`'s folder, when it can be read.
    pub game_folder: Option<String>,
    /// The hook DLL's path, when it is known and Studio launches with one.
    pub hook_dll: Option<String>,
    /// The hook DLL's folder name, or its whole folder when both sides'
    /// folder names are the same, as `install` is worded.
    pub hook_label: Option<String>,
    /// The hook DLL's build id, `None` for a build from before ids.
    pub hook_build: Option<String>,
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
        running: Box<GameLaunch>,
        wanted: Box<GameLaunch>,
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
    /// The hook DLL it has loaded; read only when Studio launches with one.
    pub hook: LoadedHook,
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
    let canonical_hook = |dll: HookDll| HookDll {
        path: PathBuf::from(canonical(&dll.path.to_string_lossy())),
        ..dll
    };
    let want = LaunchSettings {
        game_path: canonical(&want.game_path),
        ..want.clone()
    };
    let games = crate::sys::process::running_games();
    if games.is_empty() {
        return RunningGameCheck::None;
    }
    // Read once, and only with a game to compare it against.
    let want_hook = want
        .goldsrc_hooks_dll
        .as_deref()
        .map(|path| canonical_hook(crate::sys::hook_build::hook_on_disk(path)));
    let games: Vec<RunningGame> = games
        .into_iter()
        .map(|g| {
            let hook = match &want_hook {
                Some(_) => match crate::sys::hook_build::loaded_hook(
                    g.pid,
                    crate::patch::batch_reuse::HOOK_DLL_NAME,
                ) {
                    // A launch copy is named by the file it was copied from.
                    LoadedHook::Loaded(dll) => LoadedHook::Loaded(canonical_hook(HookDll {
                        path: crate::sys::hook_copy::source_of(&dll.path).unwrap_or(dll.path),
                        ..dll
                    })),
                    other => other,
                },
                None => LoadedHook::default(),
            };
            RunningGame {
                exe: g.exe.as_deref().map(canonical),
                hook,
                ..g
            }
        })
        .collect();
    check(&want, want_hook.as_ref(), &games)
}

/// The answer for `games`: the first one started with `want` is reused, and
/// only when none was does the first one count as a mismatch. `want_hook`:
/// the hook DLL Studio launches with and its build, read off disk.
pub fn check(
    want: &LaunchSettings,
    want_hook: Option<&HookDll>,
    games: &[RunningGame],
) -> RunningGameCheck {
    let mut first_mismatch = None;
    for game in games {
        match compare(want, want_hook, game) {
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
/// paths' existing "not started by DoD Studio" handling instead; so is a
/// game with no hook DLL. The hook DLL is compared only when Studio launches
/// with one (`want_hook`: the file `goldsrc_hooks_dll` names, read off disk).
pub fn compare(
    want: &LaunchSettings,
    want_hook: Option<&HookDll>,
    game: &RunningGame,
) -> RunningGameCheck {
    let LaunchSettings {
        game_path,
        game_folder,
        width,
        height,
        // Not compared: a game without HLAE is left to the reuse paths
        // (`batch_reuse`), and which HLAE build injected it can't be told.
        afx_hook_dll: _,
        // Compared through `want_hook`, which carries its build as well.
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
    // Folder names are case-insensitive on Windows, as the engine's own
    // `-game` lookup is.
    let running_folder = game_folder_from_cmd(&game.cmd);
    if let Some(folder) = &running_folder
        && !folder.eq_ignore_ascii_case(game_folder)
    {
        differs.push(LaunchDifference::GameFolder);
    }
    if let Some(difference) = hook_difference(want_hook, &game.hook) {
        differs.push(difference);
    }

    if differs.is_empty() {
        return RunningGameCheck::Match { pid: game.pid };
    }
    let running_exe = running_exe.unwrap_or_default();
    let (running_install, wanted_install) = install_labels(&running_exe, game_path);
    let running_hook = match (want_hook, &game.hook) {
        (Some(_), LoadedHook::Loaded(dll)) => Some(dll),
        _ => None,
    };
    let path_text = |dll: &HookDll| clean_path(&dll.path.to_string_lossy());
    let (running_hook_label, wanted_hook_label) = match (running_hook, want_hook) {
        (Some(running), Some(wanted)) => {
            let (r, w) = install_labels(&path_text(running), &path_text(wanted));
            (Some(r), Some(w))
        }
        (None, Some(wanted)) => {
            let (w, _) = install_labels(&path_text(wanted), "");
            (None, Some(w))
        }
        _ => (None, None),
    };
    RunningGameCheck::Mismatch {
        pid: game.pid,
        running: Box::new(GameLaunch {
            install: running_install,
            exe: clean_path(&running_exe),
            width: running_size.map(|s| s.0),
            height: running_size.map(|s| s.1),
            game_folder: running_folder,
            hook_dll: running_hook.map(path_text),
            hook_label: running_hook_label,
            hook_build: running_hook.and_then(|d| d.build.clone()),
        }),
        wanted: Box::new(GameLaunch {
            install: wanted_install,
            exe: clean_path(game_path),
            width: Some(*width),
            height: Some(*height),
            game_folder: Some(game_folder.clone()),
            hook_dll: want_hook.map(path_text),
            hook_label: wanted_hook_label,
            hook_build: want_hook.and_then(|d| d.build.clone()),
        }),
        differs,
    }
}

/// How the game's hook DLL differs from the one Studio launches with, if it
/// does. One build id is the same DLL, whichever folder each came from
/// (#1146: the game runs a copy, and two test folders may hold one build).
/// Builds from before ids are told apart by path alone, so two of them at
/// one path count as the same.
fn hook_difference(want: Option<&HookDll>, loaded: &LoadedHook) -> Option<LaunchDifference> {
    let want = want?;
    match loaded {
        LoadedHook::Absent => None,
        LoadedHook::Unreadable => Some(LaunchDifference::HookUnreadable),
        LoadedHook::Loaded(dll) if dll.build.is_some() && dll.build == want.build => None,
        LoadedHook::Loaded(dll) => {
            let same_path = normalise_path(&dll.path.to_string_lossy())
                == normalise_path(&want.path.to_string_lossy());
            if !same_path {
                Some(LaunchDifference::HookDll)
            } else if dll.build != want.build {
                Some(LaunchDifference::HookBuild)
            } else {
                None
            }
        }
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

/// `-game <mod>` from a command line, split the way [`resolution_from_cmd`]
/// splits it; the first counts. `None` when it isn't there: the engine then
/// runs `valve`, which no DoD Studio launch does, so such a game was started
/// by something else and is left to the reuse paths' own handling.
pub fn game_folder_from_cmd(cmd: &[String]) -> Option<String> {
    let tokens: Vec<&str> = cmd
        .iter()
        .flat_map(|a| a.split_whitespace())
        .map(unquote)
        .collect();
    let at = tokens
        .iter()
        .position(|t| t.eq_ignore_ascii_case("-game"))?;
    tokens
        .get(at + 1)
        .filter(|v| !v.is_empty() && !v.starts_with(['-', '+']))
        .map(|v| v.to_string())
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
            game_folder: "dod".to_string(),
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
            format!("-game dod -insecure -addons -demoedit -windowed -w {width} -h {height} -afxForceAlpha8 1 -condebug")
                .split(' ')
                .map(String::from),
        );
        RunningGame {
            pid,
            exe: Some(exe.to_string()),
            cmd,
            hook: LoadedHook::Absent,
        }
    }

    #[test]
    fn the_same_install_and_resolution_is_reused() {
        assert_eq!(
            compare(&want(PRE, 1920, 1080), None, &game(7, PRE, 1920, 1080)),
            RunningGameCheck::Match { pid: 7 }
        );
    }

    #[test]
    fn paths_compare_without_case_slashes_quotes_or_the_verbatim_prefix() {
        let typed = "c:/games/half-life - pre-anniversary for movies/HL.EXE";
        let mut running = game(7, &format!(r"\\?\{PRE}"), 1920, 1080);
        assert_eq!(
            compare(&want(typed, 1920, 1080), None, &running),
            RunningGameCheck::Match { pid: 7 }
        );
        running.exe = Some(format!("\"{PRE}\""));
        let doubled = PRE.replace('\\', r"\\");
        assert_eq!(
            compare(&want(&doubled, 1920, 1080), None, &running),
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
        } = compare(&want(POST, 3440, 1440), None, &game(7, PRE, 1920, 1080))
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

    /// A `dod_movies` game left running is not reused for a `dod` launch
    /// (#402): the game folder, like the resolution, only applies at start.
    #[test]
    fn another_game_folder_is_a_mismatch() {
        let mut running = game(7, PRE, 1920, 1080);
        for arg in running.cmd.iter_mut() {
            if arg == "dod" {
                *arg = "dod_movies".to_string();
            }
        }
        let RunningGameCheck::Mismatch {
            running,
            wanted,
            differs,
            ..
        } = compare(&want(PRE, 1920, 1080), None, &running)
        else {
            panic!("expected a mismatch");
        };
        assert_eq!(differs, vec![LaunchDifference::GameFolder]);
        assert_eq!(running.game_folder.as_deref(), Some("dod_movies"));
        assert_eq!(wanted.game_folder.as_deref(), Some("dod"));

        // Same folder in other case: the same folder on Windows.
        let mut upper = game(8, PRE, 1920, 1080);
        for arg in upper.cmd.iter_mut() {
            if arg == "dod" {
                *arg = "DOD".to_string();
            }
        }
        assert_eq!(
            compare(&want(PRE, 1920, 1080), None, &upper),
            RunningGameCheck::Match { pid: 8 }
        );
    }

    #[test]
    fn the_game_folder_is_read_from_split_or_whole_command_lines() {
        let split: Vec<String> = ["hl.exe", "-game", "dod_movies", "-w", "800"]
            .map(String::from)
            .to_vec();
        assert_eq!(game_folder_from_cmd(&split).as_deref(), Some("dod_movies"));
        let whole = vec![PRE.to_string(), "-GAME \"dod_1_2\" -insecure".to_string()];
        assert_eq!(game_folder_from_cmd(&whole).as_deref(), Some("dod_1_2"));
        let none: Vec<String> = ["hl.exe", "-w", "800"].map(String::from).to_vec();
        assert_eq!(game_folder_from_cmd(&none), None);
        let dangling: Vec<String> = ["hl.exe", "-game", "-insecure"].map(String::from).to_vec();
        assert_eq!(game_folder_from_cmd(&dangling), None);
    }

    #[test]
    fn a_resolution_change_alone_is_a_mismatch() {
        let check = compare(&want(PRE, 2560, 1440), None, &game(7, PRE, 1920, 1080));
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
            "-game dod -windowed -w 3440 -h 1440 -afxForceAlpha8 1".to_string(),
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
            hook: LoadedHook::Absent,
        };
        assert_eq!(
            compare(&want(PRE, 1920, 1080), None, &steam_started),
            RunningGameCheck::Match { pid: 3 }
        );
        let unreadable = RunningGame {
            pid: 4,
            ..Default::default()
        };
        assert_eq!(
            compare(&want(PRE, 1920, 1080), None, &unreadable),
            RunningGameCheck::Match { pid: 4 }
        );
    }

    #[test]
    fn without_an_image_path_the_quoted_first_argument_names_the_install() {
        let mut running = game(5, PRE, 1920, 1080);
        running.exe = None;
        assert!(matches!(
            compare(&want(POST, 1920, 1080), None, &running),
            RunningGameCheck::Mismatch { ref differs, .. } if differs == &[LaunchDifference::Install]
        ));
        assert_eq!(
            compare(&want(PRE, 1920, 1080), None, &running),
            RunningGameCheck::Match { pid: 5 }
        );
    }

    #[test]
    fn a_matching_game_wins_over_a_mismatched_one() {
        let w = want(PRE, 1920, 1080);
        assert_eq!(check(&w, None, &[]), RunningGameCheck::None);
        let games = [game(1, POST, 1920, 1080), game(2, PRE, 1920, 1080)];
        assert_eq!(check(&w, None, &games), RunningGameCheck::Match { pid: 2 });
        let games = [game(1, POST, 1920, 1080), game(2, POST, 800, 600)];
        assert!(matches!(
            check(&w, None, &games),
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
            hook: LoadedHook::Absent,
        };
        assert_eq!(
            compare(&cfg.launch_settings(), None, &running),
            RunningGameCheck::Match { pid: 9 }
        );
        let mut other = cfg.clone();
        other.resolution_width = 1920;
        assert!(matches!(
            compare(&other.launch_settings(), None, &running),
            RunningGameCheck::Mismatch { .. }
        ));
    }

    const HOOK_A: &str = r"C:\Repos\dod-studio\local\test-660\dodstudio_goldsrc_hooks.dll";
    const HOOK_B: &str = r"C:\Repos\dod-studio\local\test-660b\dodstudio_goldsrc_hooks.dll";

    fn hook(path: &str, build: Option<&str>) -> HookDll {
        HookDll {
            path: PathBuf::from(path),
            build: build.map(String::from),
        }
    }

    fn game_with_hook(loaded: LoadedHook) -> RunningGame {
        RunningGame {
            hook: loaded,
            ..game(7, PRE, 1920, 1080)
        }
    }

    fn differs_with_hook(want_hook: &HookDll, loaded: LoadedHook) -> Vec<LaunchDifference> {
        match compare(
            &want(PRE, 1920, 1080),
            Some(want_hook),
            &game_with_hook(loaded),
        ) {
            RunningGameCheck::Mismatch { differs, .. } => differs,
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    #[test]
    fn the_same_hook_dll_and_build_is_reused_whatever_the_path_spelling() {
        let typed = HOOK_A.to_lowercase().replace('\\', "/");
        let loaded = LoadedHook::Loaded(hook(&format!(r"\\?\{HOOK_A}"), Some("0.10.0+abc")));
        assert_eq!(
            compare(
                &want(PRE, 1920, 1080),
                Some(&hook(&typed, Some("0.10.0+abc"))),
                &game_with_hook(loaded)
            ),
            RunningGameCheck::Match { pid: 7 }
        );
    }

    #[test]
    fn a_hook_dll_from_another_path_is_named_with_both_paths_and_folders() {
        let loaded = LoadedHook::Loaded(hook(HOOK_A, Some("0.10.0+aaa")));
        let RunningGameCheck::Mismatch {
            running,
            wanted,
            differs,
            ..
        } = compare(
            &want(PRE, 1920, 1080),
            Some(&hook(HOOK_B, Some("0.10.0+bbb"))),
            &game_with_hook(loaded),
        )
        else {
            panic!("expected a mismatch");
        };
        assert_eq!(differs, vec![LaunchDifference::HookDll]);
        assert_eq!(running.hook_dll.as_deref(), Some(HOOK_A));
        assert_eq!(wanted.hook_dll.as_deref(), Some(HOOK_B));
        assert_eq!(running.hook_label.as_deref(), Some("test-660"));
        assert_eq!(wanted.hook_label.as_deref(), Some("test-660b"));
        assert_eq!(running.hook_build.as_deref(), Some("0.10.0+aaa"));
        assert_eq!(wanted.hook_build.as_deref(), Some("0.10.0+bbb"));
    }

    /// One build in two folders (a test DLL copied to another folder, or the
    /// game's launch copy named by its source) is the same DLL (#1146).
    #[test]
    fn the_same_build_from_another_folder_is_reused() {
        let loaded = LoadedHook::Loaded(hook(HOOK_A, Some("0.10.0+abc")));
        assert_eq!(
            compare(
                &want(PRE, 1920, 1080),
                Some(&hook(HOOK_B, Some("0.10.0+abc"))),
                &game_with_hook(loaded)
            ),
            RunningGameCheck::Match { pid: 7 }
        );
        // Without ids, only the path can tell them apart.
        let old = LoadedHook::Loaded(hook(HOOK_A, None));
        assert_eq!(
            differs_with_hook(&hook(HOOK_B, None), old),
            vec![LaunchDifference::HookDll]
        );
    }

    /// The file was rebuilt in place (#1146) or renamed away and a new build
    /// put at its path (#1143) while the game ran.
    #[test]
    fn another_build_at_the_same_path_is_a_mismatch() {
        let loaded = LoadedHook::Loaded(hook(HOOK_A, Some("0.10.0+aaa")));
        assert_eq!(
            differs_with_hook(&hook(HOOK_A, Some("0.10.0+bbb")), loaded),
            vec![LaunchDifference::HookBuild]
        );
    }

    #[test]
    fn a_build_without_an_id_differs_from_one_with_an_id() {
        let old = LoadedHook::Loaded(hook(HOOK_A, None));
        assert_eq!(
            differs_with_hook(&hook(HOOK_A, Some("0.10.0+bbb")), old),
            vec![LaunchDifference::HookBuild]
        );
    }

    #[test]
    fn two_builds_without_ids_at_the_same_path_are_reused() {
        let old = LoadedHook::Loaded(hook(HOOK_A, None));
        assert_eq!(
            compare(
                &want(PRE, 1920, 1080),
                Some(&hook(HOOK_A, None)),
                &game_with_hook(old)
            ),
            RunningGameCheck::Match { pid: 7 }
        );
    }

    #[test]
    fn a_game_whose_hook_dll_cannot_be_read_is_not_reused() {
        assert_eq!(
            differs_with_hook(&hook(HOOK_A, Some("x")), LoadedHook::Unreadable),
            vec![LaunchDifference::HookUnreadable]
        );
    }

    /// A game Steam started has no hook DLL: left to the reuse paths' "not
    /// started by DoD Studio" handling, as before #1143.
    #[test]
    fn a_game_without_a_hook_dll_is_not_a_hook_difference() {
        assert_eq!(
            compare(
                &want(PRE, 1920, 1080),
                Some(&hook(HOOK_A, Some("x"))),
                &game_with_hook(LoadedHook::Absent)
            ),
            RunningGameCheck::Match { pid: 7 }
        );
    }

    #[test]
    fn without_a_hook_dll_set_the_game_s_is_not_compared() {
        for loaded in [
            LoadedHook::Unreadable,
            LoadedHook::Loaded(hook(HOOK_A, Some("x"))),
        ] {
            assert_eq!(
                compare(&want(PRE, 1920, 1080), None, &game_with_hook(loaded)),
                RunningGameCheck::Match { pid: 7 }
            );
        }
    }
}
