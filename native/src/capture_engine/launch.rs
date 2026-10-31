//! Starting a batch's game: what goes on its command line, what is sent to
//! it once it takes commands, and the HLAE launcher (#545's `Launcher`).

use super::CaptureJob;
use crate::log_markdown;
use crate::patch::runtime_batch::RuntimeBatch;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The demo a patched batch starts on: its first job, which is the primer when
/// `build_batch_queue` planned one and dodstudio_chain_01 when it didn't
/// (`patch::builder::plays_primer`, #365). Read from the plan rather than
/// decided again, so the start can't name a demo the batch never wrote. Named
/// as the copy into `dod/` names the file.
fn first_demo_stem(jobs: &[CaptureJob]) -> Option<String> {
    let stem = jobs.first()?.patched_demo_path.file_stem()?;
    Some(stem.to_string_lossy().replace('-', "_"))
}

/// How the batch's game is started.
///
/// The extra arguments MUST be embedded in HLAE's `-cmdLine`, not appended
/// to its own command: HLAE's `-customLoader` argument parsing
/// (`ProcessArgsCustomLoader` in advancedfx/advancedfx's `hlae/Program.cs`)
/// reads exactly six flags and silently drops everything else on its own
/// argv — only text folded into this string reaches `hl.exe`, via the single
/// `-cmdLine` value `build_hlae_process` composes.
///
/// HLAE's alpha flag (`-afxForceAlpha8 1`, the only one that matters, #69)
/// lives in `build_hlae_process` itself, so every `-customLoader` launch
/// this app makes carries it, not just a capture batch.
///
/// The first demo starts over the hook's pipe once the game is up, so its
/// map loads after config.cfg/movie.cfg set the user's brightness and gamma
/// (`game_handoff`). With the hook that demo is dodstudio_chain_01 itself,
/// without it the primer (#365). A batch the hook runs starts the same way,
/// with its batch file.
///
/// OBS mode records the game's own sound output, which DirectSound silences
/// while the game isn't the active window, so it plays through waveOut
/// instead (`obs::GAME_ARGS`).
#[derive(Debug, PartialEq)]
pub(super) struct LaunchPlan {
    /// Folded into the game's command line.
    pub(super) extra_args: String,
    /// Sent as soon as the game takes commands.
    pub(super) early_lines: Vec<String>,
    /// Sent once the game has settled: what starts the batch. Empty when the
    /// launch line already does.
    pub(super) first_lines: Vec<String>,
    /// The open game the batch runs in instead of starting one (#725).
    pub(super) reuse_game: Option<u32>,
}

impl LaunchPlan {
    pub(super) fn new(
        config: &crate::patch::PatcherConfig,
        runtime_batch: Option<&RuntimeBatch>,
        jobs: &[CaptureJob],
    ) -> Self {
        let with_start = |args: &str| {
            let args = &if config.capture_mode == crate::patch::CaptureMode::Obs {
                format!("{} {args}", crate::obs::GAME_ARGS)
            } else {
                args.to_string()
            };
            if config.keep_game_behind {
                format!("{} {args}", crate::patch::builder::START_BEHIND_ARG)
            } else {
                args.to_string()
            }
        };
        let (extra_args, early_lines, first_lines) = match runtime_batch {
            Some(batch) => (
                with_start("+exec dodstudio_helper.cfg"),
                crate::patch::runtime_batch::early_lines(config.keep_game_behind),
                crate::patch::runtime_batch::start_lines(batch),
            ),
            None => {
                let (args, lines) = crate::game_handoff::first_demo_start(
                    &first_demo_stem(jobs).unwrap_or_default(),
                    config.goldsrc_hooks_dll().is_some(),
                );
                (with_start(&args), Vec::new(), lines)
            }
        };
        // A batch in the game that is already open (#725): it stops what
        // that game was doing and loads the helper cfg a launch gets from
        // `+exec`, ahead of the usual early lines.
        let reuse_game = runtime_batch.and_then(|b| b.reuse_game);
        let early_lines = if reuse_game.is_some() {
            crate::patch::batch_reuse::REUSE_LINES
                .iter()
                .map(|l| l.to_string())
                .chain(early_lines)
                .collect()
        } else {
            early_lines
        };
        let early_lines = crate::game_handoff::with_player_overrides(
            early_lines,
            config.player_overrides,
            config.goldsrc_hooks_dll().is_some(),
        );
        Self {
            extra_args,
            early_lines,
            first_lines,
            reuse_game,
        }
    }

    /// What the poll loop sends the game once it is up, when anything is.
    pub(super) fn handoff(&mut self) -> Option<crate::game_handoff::GameHandoff> {
        (!self.first_lines.is_empty()).then(|| {
            crate::game_handoff::GameHandoff::with_early(
                std::mem::take(&mut self.early_lines),
                std::mem::take(&mut self.first_lines),
            )
        })
    }
}

/// Held until the batch starts in the game (it has stepped aside by then) or
/// `FOREGROUND_LOCK_MAX` passes; dropped on every way out. Not taken when the
/// user wants to watch the game, nor for a game that is already open: it
/// isn't starting.
fn foreground_lock(
    keep_game_behind: bool,
    reuse_game: Option<u32>,
) -> (crate::sys::foreground_lock::ForegroundLock, Instant) {
    let foreground_lock = if keep_game_behind && reuse_game.is_none() {
        crate::sys::foreground_lock::ForegroundLock::take()
    } else {
        crate::sys::foreground_lock::ForegroundLock::none()
    };
    let foreground_locked_at = Instant::now();
    if foreground_lock.locked() {
        log_markdown("[HLAE] The game can't take the foreground while it starts");
    }
    (foreground_lock, foreground_locked_at)
}

/// A started game, for the poll loop and the teardown.
pub(super) struct Launched {
    /// The HLAE launcher; `None` for a batch in the open game.
    pub(super) child: Option<crate::sys::launcher::Launcher>,
    pub(super) handoff: Option<crate::game_handoff::GameHandoff>,
    pub(super) reuse_game: Option<u32>,
    pub(super) foreground_lock: crate::sys::foreground_lock::ForegroundLock,
    pub(super) foreground_locked_at: Instant,
    /// `dod/dod_quit.cfg`, written for the batch.
    pub(super) cfg_path: PathBuf,
    /// The done marker in the export folder, cleared before the launch.
    pub(super) dummy_path: PathBuf,
    /// The games already running when HLAE was started, so the batch's own
    /// is told apart from them (#1213).
    pub(super) games_before_launch: Vec<super::processes::GameProcess>,
}

/// Starts the batch's game (or takes over the open one, #725) and marks the
/// batch's start as pending or not needed. `None` once a launch failure is
/// reported; the quit cfg goes with it, and the batch's `Leftovers` take
/// the demos copied into `dod/` (#1212).
pub(super) fn start_game(
    batch: &super::Batch,
    export_dir: &Path,
    dod_dir: &Path,
) -> Option<Launched> {
    let config = &batch.config;
    let mut plan = LaunchPlan::new(config, batch.runtime_batch.as_ref(), &batch.jobs);
    let handoff = plan.handoff();
    let reuse_game = plan.reuse_game;

    let dummy_path = export_dir.join("DOD_BATCH_DONE");
    let _ = std::fs::remove_dir_all(&dummy_path);

    let mut cmd = config.build_hlae_process(&plan.extra_args);

    let cfg_path = dod_dir.join("dod_quit.cfg");
    std::fs::write(&cfg_path, "quit\n").ok();

    if reuse_game.is_none() {
        log_markdown(&format!(
            "[HLAE] Spawning: {:?} {:?}",
            cmd.get_program(),
            cmd.get_args().collect::<Vec<_>>()
        ));
    }

    let (foreground_lock, foreground_locked_at) =
        foreground_lock(config.keep_game_behind, reuse_game);

    let games_before_launch = {
        use super::processes::Processes;
        super::processes::Live::new().games()
    };
    let child = match start_launcher(&mut cmd, reuse_game, &batch.cancel_token) {
        Ok(child) => child,
        // Cancel while WMI was starting HLAE: the launcher is ended already
        // (`Launcher::spawn_unless_cancelled`), and a game it got as far as
        // starting goes too.
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
            let mut game = super::game::BatchGame::new(games_before_launch, None);
            super::game::kill_hl_exe_and_wait(&mut super::processes::Live::new(), &mut game);
            let _ = batch.tx.send(super::EngineEvent::Cancelled);
            std::fs::remove_file(&cfg_path).ok();
            return None;
        }
        Err(e) => {
            super::abort(
                &batch.tx,
                &format!(
                    "Failed to launch HLAE ({}): {}",
                    cmd.get_program().to_string_lossy(),
                    e
                ),
            );
            std::fs::remove_file(&cfg_path).ok();
            return None;
        }
    };
    batch.batch_start.set(if handoff.is_some() {
        crate::game_handoff::StartState::Pending
    } else {
        crate::game_handoff::StartState::NotNeeded
    });
    Some(Launched {
        child,
        handoff,
        reuse_game,
        foreground_lock,
        foreground_locked_at,
        cfg_path,
        dummy_path,
        games_before_launch,
    })
}

/// Starts the HLAE launcher, or for a batch in the open game, nothing.
fn start_launcher(
    cmd: &mut std::process::Command,
    reuse_game: Option<u32>,
    cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<Option<crate::sys::launcher::Launcher>> {
    match reuse_game {
        Some(pid) => {
            log_markdown(&format!(
                "[HLAE] Running the batch in the game that is already open (PID {pid}), not starting one (#725)"
            ));
            Ok(None)
        }
        None => {
            let c = crate::sys::launcher::Launcher::spawn_unless_cancelled(cmd, cancel)?;
            log_markdown(&format!("[HLAE] Spawned (PID {})", c.id()));
            Ok(Some(c))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::{CaptureMode, PatcherConfig};
    use std::path::PathBuf;

    /// The batch starts on whatever the plan put first: the primer without the
    /// hook DLL, dodstudio_chain_01 with it (#365).
    #[test]
    fn the_batch_starts_on_its_first_planned_demo() {
        let job = |name: &str| CaptureJob {
            patched_demo_path: PathBuf::from("game").join("dod").join(name),
        };
        let with_primer = [job("dodstudio_primer.dem"), job("dodstudio_chain_01.dem")];
        assert_eq!(
            first_demo_stem(&with_primer).as_deref(),
            Some("dodstudio_primer")
        );
        let without = [job("dodstudio_chain_01.dem"), job("dodstudio_chain_02.dem")];
        assert_eq!(
            first_demo_stem(&without).as_deref(),
            Some("dodstudio_chain_01")
        );
        assert_eq!(first_demo_stem(&[]), None);
    }

    /// A config whose hook DLL is `hooks` (a file that exists or not), so
    /// `goldsrc_hooks_dll()` answers without looking at the real install.
    fn config(hooks: &std::path::Path) -> PatcherConfig {
        PatcherConfig {
            goldsrc_hooks_dll_path: Some(hooks.display().to_string()),
            hlae_path: hooks.with_file_name("HLAE.exe").display().to_string(),
            keep_game_behind: false,
            ..Default::default()
        }
    }

    fn batch(reuse_game: Option<u32>) -> RuntimeBatch {
        RuntimeBatch {
            file: PathBuf::from("D:/batch.txt"),
            source_copies: Vec::new(),
            other_drives_in_place: false,
            reuse_game,
        }
    }

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn a_hook_run_batch_starts_with_its_file_once_the_game_is_up() {
        let dir = crate::test_support::Scratch::new("capture_engine_plan_hook");
        let mut cfg = config(&dir.path().join("hooks.dll"));
        cfg.keep_game_behind = true;
        let plan = LaunchPlan::new(&cfg, Some(&batch(None)), &[]);
        assert_eq!(
            plan,
            LaunchPlan {
                extra_args: "-dodstudio_start_behind +exec dodstudio_helper.cfg".into(),
                early_lines: lines(&[
                    "ds_capture_run_in_background 1",
                    "ds_capture_stay_behind 1",
                    "ds_capture_batch away",
                ]),
                first_lines: lines(&["ds_capture_batch start \"D:/batch.txt\""]),
                reuse_game: None,
            }
        );
    }

    #[test]
    fn obs_mode_plays_through_waveout_after_the_stay_behind_flag() {
        let dir = crate::test_support::Scratch::new("capture_engine_plan_obs");
        let mut cfg = config(&dir.path().join("hooks.dll"));
        cfg.capture_mode = CaptureMode::Obs;
        cfg.keep_game_behind = true;
        let plan = LaunchPlan::new(&cfg, Some(&batch(None)), &[]);
        assert_eq!(
            plan.extra_args,
            format!(
                "-dodstudio_start_behind {} +exec dodstudio_helper.cfg",
                crate::obs::GAME_ARGS
            )
        );
        cfg.keep_game_behind = false;
        let plan = LaunchPlan::new(&cfg, Some(&batch(None)), &[]);
        assert_eq!(
            plan.extra_args,
            format!("{} +exec dodstudio_helper.cfg", crate::obs::GAME_ARGS)
        );
        assert_eq!(
            plan.early_lines,
            lines(&["ds_capture_run_in_background 1", "ds_capture_stay_behind 0"])
        );
    }

    /// A batch in the open game resets it first and starts nothing (#725).
    #[test]
    fn a_batch_in_the_open_game_resets_it_ahead_of_the_early_lines() {
        let dir = crate::test_support::Scratch::new("capture_engine_plan_reuse");
        let cfg = config(&dir.path().join("hooks.dll"));
        let plan = LaunchPlan::new(&cfg, Some(&batch(Some(4242))), &[]);
        assert_eq!(plan.reuse_game, Some(4242));
        let reset = crate::patch::batch_reuse::REUSE_LINES.len();
        assert_eq!(
            plan.early_lines[..reset],
            lines(crate::patch::batch_reuse::REUSE_LINES)[..]
        );
        assert_eq!(
            plan.early_lines[reset..],
            lines(&["ds_capture_run_in_background 1", "ds_capture_stay_behind 0"])[..]
        );
    }

    /// A patched batch: with the hook DLL its first demo goes over the pipe,
    /// without it on the launch line, where nothing is left to send.
    #[test]
    fn a_patched_batchs_first_demo_goes_over_the_pipe_only_with_the_hook() {
        let dir = crate::test_support::Scratch::new("capture_engine_plan_patched");
        let hooks = dir.path().join("hooks.dll");
        let jobs = [CaptureJob {
            patched_demo_path: dir.path().join("dodstudio-chain-01.dem"),
        }];
        let mut cfg = config(&hooks);
        cfg.player_overrides = Some(true);

        // No hook DLL (the file isn't there, nor beside the config's HLAE):
        // the launch line plays the demo, and the Players box needs the hook.
        if cfg.goldsrc_hooks_dll().is_none() {
            let mut plan = LaunchPlan::new(&cfg, None, &jobs);
            assert_eq!(
                plan.extra_args,
                "+exec dodstudio_helper.cfg +playdemo dodstudio_chain_01"
            );
            assert!(plan.early_lines.is_empty());
            assert!(plan.handoff().is_none());
        }

        std::fs::write(&hooks, b"dll").unwrap();
        let mut plan = LaunchPlan::new(&cfg, None, &jobs);
        assert_eq!(plan.extra_args, "+exec dodstudio_helper.cfg");
        assert_eq!(plan.first_lines, lines(&["playdemo dodstudio_chain_01"]));
        assert_eq!(plan.early_lines, lines(&["ds_player_overrides 1"]));
        let handoff = plan.handoff().expect("lines to send");
        assert_eq!(handoff.lines, lines(&["playdemo dodstudio_chain_01"]));
    }
}
