//! Launch Preview without patching (#434, step 2), the Studio half.
//!
//! When the game will run with the goldsrc-hooks DLL and can reach the demo
//! by a relative path (`native::preview_in_place`), Launch Preview plays the
//! original demo and opens the in-game DoD Studio window's Highlights tab,
//! which lists the highlights with Go (#565). Nothing is written to disk. In
//! every other case Launch Preview patches a `<stem>_preview.dem` with a
//! bookmark per highlight, as before.

use std::path::Path;
use std::time::{Duration, Instant};

use native::patch::PatcherConfig;
use native::preview_in_place::{OPEN_HIGHLIGHTS, viewdemo_arg, viewdemo_line};
use native::sys::game_remote::{Sent, send_console_commands};

use crate::capture_manager::SerializedStreak;

/// How long a launched game has to open its command pipe.
const PIPE_WAIT: Duration = Duration::from_secs(90);
/// After the pipe answers, how long before the window is opened: the pipe
/// opens while the engine is still starting.
const SETTLE: Duration = Duration::from_secs(5);

/// The `viewdemo` argument for previewing `streaks`' demo in place, or
/// `None` when this preview has to be patched.
pub fn plan(
    config: &PatcherConfig,
    dod_dir: &Path,
    streaks: &[SerializedStreak],
) -> Option<String> {
    config.goldsrc_hooks_dll()?;
    let demo = Path::new(&streaks.first()?.source_demo);
    if !demo.is_file() {
        return None;
    }
    viewdemo_arg(dod_dir, demo)
}

/// Starts the game, then, once it takes commands, plays the original demo and
/// opens the Highlights tab.
///
/// Not `+viewdemo` on the launch line: the engine cuts its command line at
/// every '-', and a relative path to a demo elsewhere almost always has one
/// ("Half-Life", "dod-studio", most demo names). Over the pipe, the line is
/// the console's own, where a quoted argument keeps its dashes.
pub fn launch(app: tauri::AppHandle, config: &PatcherConfig, arg: &str) -> Result<(), String> {
    let before = crate::capture_manager::running_game_pids();
    let mut cmd = config.build_hlae_process("");
    let launcher = cmd
        .spawn()
        .map_err(crate::messages::failed_to_launch_hlae_for_preview)?;
    log::info!("[preview] in place: launched the game for \"{arg}\" (no patched copy)");
    crate::capture_manager::watch_for_error_dialogs(app, launcher);
    let lines = vec![viewdemo_line(arg), OPEN_HIGHLIGHTS.to_string()];
    std::thread::spawn(move || send_when_ready(&before, &lines));
    Ok(())
}

/// Sends the preview to a running game. `Ok(Some(line))` when one took it,
/// `Ok(None)` when none takes commands.
pub fn send_to_running(pids: &[u32], arg: &str) -> Result<Option<String>, String> {
    let lines = [viewdemo_line(arg), OPEN_HIGHLIGHTS.to_string()];
    for &pid in pids {
        match send_console_commands(pid, &lines)
            .map_err(crate::messages::failed_to_send_to_running_game)?
        {
            Sent::Delivered => {
                log::info!(
                    "[preview] in place: sent \"{}\" to the running game (pid {pid})",
                    lines[0]
                );
                return Ok(Some(lines[0].clone()));
            }
            Sent::NotListening => continue,
        }
    }
    Ok(None)
}

/// Waits for the game launched after `before` was listed to open its pipe,
/// then sends it `lines`.
fn send_when_ready(before: &[u32], lines: &[String]) {
    let started = Instant::now();
    while started.elapsed() < PIPE_WAIT {
        std::thread::sleep(Duration::from_millis(500));
        let fresh: Vec<u32> = crate::capture_manager::running_game_pids()
            .into_iter()
            .filter(|pid| !before.contains(pid))
            .collect();
        for pid in fresh {
            // An empty message checks the pipe without running anything.
            if matches!(send_console_commands(pid, &[]), Ok(Sent::Delivered)) {
                // The pipe opens before the hook has wrapped `viewdemo`, which
                // is what lets the Highlights tab know the demo.
                std::thread::sleep(SETTLE);
                let sent = send_console_commands(pid, lines);
                log::info!("[preview] in place: {lines:?} -> pid {pid}: {sent:?}");
                return;
            }
        }
    }
    log::warn!(
        "[preview] in place: the game never opened its command pipe; the preview was not started"
    );
}
