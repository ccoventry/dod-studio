//! A Seek Between Clips batch in a game that is already open (#725), instead
//! of Force Relaunch.
//!
//! Such a batch runs entirely over the hook's pipe (`runtime_batch`): Studio
//! sends a few settings and `ds_capture_batch start "<file>"`, and the hook
//! loads each demo itself. A game Studio started takes those commands just as
//! a fresh one does, as long as nothing a launch fixes differs. That is:
//!
//! - **Seek Between Clips.** A patched batch starts the game on its first
//!   patched demo from the launch line, so it always launches.
//! - **Not OBS mode.** OBS mode starts the game with its sound through
//!   waveOut (`obs::GAME_ARGS`), which only a launch sets. The other modes add
//!   nothing to the launch line a batch would not add over the pipe.
//! - **The install, game folder and resolution** (`launch_settings`, #666):
//!   the caller asks about a game started with other ones before this.
//! - **The hook DLL Studio is set to**, read off the game's own module list
//!   (`sys::modules`): HLAE passes it on its own command line and exits, so
//!   nothing else says which file it was. A game started from Steam has none;
//!   one started with another build of it might not know `ds_capture_batch`.
//! - **HLAE is in it** (`AfxHookGoldSrc.dll`, read off the same list): every
//!   capture mode records through HLAE's `mirv_` commands.
//! - **Its command pipe answers**, so the batch can be handed over.
//!
//! What a launch guarantees and a reuse doesn't is listed in
//! `docs/features.md` (3.3); `REUSE_LINES` resets what the batch relies on.

use std::path::{Path, PathBuf};

use crate::patch::CaptureMode;
use crate::patch::launch_settings::RunningGameCheck;

/// What the decision reads, gathered by the caller.
#[derive(Debug, Clone)]
pub struct ReuseFacts<'a> {
    /// Seek Between Clips is on.
    pub hook_runs_batch: bool,
    pub capture_mode: CaptureMode,
    /// `launch_settings::check_running_game` for the launch Studio would make.
    pub running: &'a RunningGameCheck,
    /// The hook DLL Studio would launch with (`PatcherConfig::goldsrc_hooks_dll`).
    pub hook_dll: Option<&'a Path>,
    /// The running game's modules (`sys::modules::loaded_modules`), `None`
    /// when they could not be read.
    pub modules: Option<&'a [PathBuf]>,
    /// Its command pipe took an empty message.
    pub pipe_answers: bool,
}

/// The hook DLL's file name, as the game has it loaded.
pub const HOOK_DLL_NAME: &str = "dodstudio_goldsrc_hooks.dll";

/// HLAE's DLL in the game, as the game has it loaded.
pub const HLAE_DLL_NAME: &str = "AfxHookGoldSrc.dll";

fn named<'a>(modules: &'a [PathBuf], name: &str) -> Vec<&'a PathBuf> {
    modules
        .iter()
        .filter(|m| {
            m.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })
        .collect()
}

/// The game to run the batch in, or why a launch is needed (one sentence
/// for the activity log, starting lower case).
pub fn game_to_reuse(facts: &ReuseFacts) -> Result<u32, String> {
    let pid = match facts.running {
        RunningGameCheck::Match { pid } => *pid,
        RunningGameCheck::None => return Err("no game is running".into()),
        RunningGameCheck::Mismatch { .. } => {
            return Err("the running game was started with other launch settings".into());
        }
    };
    if !facts.hook_runs_batch {
        return Err(
            "Seek Between Clips is off, and a patched batch starts the game on its first demo"
                .into(),
        );
    }
    if facts.capture_mode == CaptureMode::Obs {
        return Err(
            "OBS mode starts the game with its sound through waveOut, which only a fresh start sets"
                .into(),
        );
    }
    let Some(wanted) = facts.hook_dll else {
        return Err("Studio has no hook DLL to run the batch with".into());
    };
    let Some(modules) = facts.modules else {
        return Err(format!("the game's DLLs (PID {pid}) could not be read"));
    };
    let loaded = named(modules, HOOK_DLL_NAME);
    if loaded.is_empty() {
        return Err("the running game has no DoD Studio hook DLL (not started by Studio)".into());
    }
    if !loaded.iter().any(|m| same_file(m, wanted)) {
        return Err(format!(
            "the running game has the hook DLL {}, and Studio is set to {}",
            loaded[0].display(),
            wanted.display()
        ));
    }
    if named(modules, HLAE_DLL_NAME).is_empty() {
        return Err(
            "the running game has no HLAE in it, which every capture records through".into(),
        );
    }
    if !facts.pipe_answers {
        return Err("the running game does not take commands from Studio".into());
    }
    Ok(pid)
}

/// The same file, resolved on disk when both can be, else by case-blind text.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a
            .to_string_lossy()
            .replace('/', "\\")
            .eq_ignore_ascii_case(&b.to_string_lossy().replace('/', "\\")),
    }
}

/// Sent to a reused game ahead of the batch's own early lines: stop whatever
/// it is doing (a review, an earlier batch, a demo), and load the helper cfg
/// with the batch's aliases, which a launch gets from `+exec` on its command
/// line.
pub const REUSE_LINES: &[&str] = &[
    "ds_demo_review stop",
    "ds_capture_batch stop",
    "disconnect",
    "exec dodstudio_helper.cfg",
];

/// Sent to a reused game when its batch ends or is cancelled: the batch's
/// recording is stopped and the demo closed, so the game is left at its menu
/// rather than closed. A Cancel can land mid-clip or mid-fast-forward, so the
/// helper cfg's own aliases put back what those set: `sys_record_stop` ends
/// an AGR recording and its `host_framerate` (`ds_capture_batch stop` only
/// stops `mirv_recordmovie`), and `sys_normal_speed` undoes
/// `sys_fast_forward` (`r_norefresh 1`, `host_framerate 0.1`, `fps_max`).
pub const LEAVE_LINES: &[&str] = &[
    "ds_capture_batch stop",
    "sys_record_stop",
    "mirv_recordmovie_stop",
    "sys_normal_speed",
    "disconnect",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::launch_settings::{GameLaunch, LaunchDifference};

    const DLL: &str = r"C:\dod-studio\local\test\dodstudio_goldsrc_hooks.dll";

    fn modules() -> Vec<PathBuf> {
        vec![
            PathBuf::from(r"C:\Games\Half-Life\hl.exe"),
            PathBuf::from(r"C:\HLAE\AfxHookGoldSrc.dll"),
            PathBuf::from(DLL.to_ascii_uppercase()),
        ]
    }

    fn facts<'a>(running: &'a RunningGameCheck, modules: &'a [PathBuf]) -> ReuseFacts<'a> {
        ReuseFacts {
            hook_runs_batch: true,
            capture_mode: CaptureMode::DirectToVideo,
            running,
            hook_dll: Some(Path::new(DLL)),
            modules: Some(modules),
            pipe_answers: true,
        }
    }

    #[test]
    fn a_studio_game_with_the_same_hook_dll_is_reused() {
        let running = RunningGameCheck::Match { pid: 42 };
        let m = modules();
        assert_eq!(game_to_reuse(&facts(&running, &m)), Ok(42));
        // AGR and frame sequences add nothing to the launch line either.
        for mode in [CaptureMode::Agr, CaptureMode::FrameSequence] {
            let f = ReuseFacts {
                capture_mode: mode,
                ..facts(&running, &m)
            };
            assert_eq!(game_to_reuse(&f), Ok(42));
        }
    }

    #[test]
    fn anything_a_launch_fixes_needs_a_launch() {
        let running = RunningGameCheck::Match { pid: 42 };
        let m = modules();
        let refused = |f: ReuseFacts| game_to_reuse(&f).unwrap_err();
        assert!(
            refused(ReuseFacts {
                hook_runs_batch: false,
                ..facts(&running, &m)
            })
            .contains("Seek Between Clips is off")
        );
        assert!(
            refused(ReuseFacts {
                capture_mode: CaptureMode::Obs,
                ..facts(&running, &m)
            })
            .contains("OBS")
        );
        assert!(
            refused(ReuseFacts {
                hook_dll: None,
                ..facts(&running, &m)
            })
            .contains("no hook DLL")
        );
        assert!(
            refused(ReuseFacts {
                modules: None,
                ..facts(&running, &m)
            })
            .contains("could not be read")
        );
        assert!(
            refused(ReuseFacts {
                pipe_answers: false,
                ..facts(&running, &m)
            })
            .contains("does not take commands")
        );
    }

    #[test]
    fn a_game_without_the_hook_dll_or_with_another_one_is_not_reused() {
        let running = RunningGameCheck::Match { pid: 42 };
        let none = vec![PathBuf::from(r"C:\Games\Half-Life\hl.exe")];
        assert!(
            game_to_reuse(&facts(&running, &none))
                .unwrap_err()
                .contains("no DoD Studio hook DLL")
        );
        let other = vec![PathBuf::from(r"C:\older\dodstudio_goldsrc_hooks.dll")];
        let e = game_to_reuse(&facts(&running, &other)).unwrap_err();
        assert!(e.contains(r"C:\older\dodstudio_goldsrc_hooks.dll"), "{e}");
        assert!(e.contains("Studio is set to"), "{e}");
        // Our hook DLL without HLAE: nothing would record.
        let no_hlae = vec![PathBuf::from(DLL)];
        assert!(
            game_to_reuse(&facts(&running, &no_hlae))
                .unwrap_err()
                .contains("no HLAE")
        );
    }

    #[test]
    fn no_game_or_one_with_other_launch_settings_is_not_reused() {
        let m = modules();
        assert!(
            game_to_reuse(&facts(&RunningGameCheck::None, &m))
                .unwrap_err()
                .contains("no game")
        );
        let launch = GameLaunch {
            install: "x".into(),
            exe: "x".into(),
            width: None,
            height: None,
            game_folder: None,
        };
        let mismatch = RunningGameCheck::Mismatch {
            pid: 42,
            running: launch.clone(),
            wanted: launch,
            differs: vec![LaunchDifference::Resolution],
        };
        assert!(
            game_to_reuse(&facts(&mismatch, &m))
                .unwrap_err()
                .contains("other launch settings")
        );
    }

    #[test]
    fn the_lines_are_ones_the_pipe_takes() {
        for line in REUSE_LINES.iter().chain(LEAVE_LINES) {
            assert!(
                crate::sys::game_remote::check_command(line).is_ok(),
                "{line}"
            );
        }
    }
}
