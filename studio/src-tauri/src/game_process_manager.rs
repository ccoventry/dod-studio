// game_process_manager.rs
// The game, HLAE and Steam as processes: Launch Game, reusing or closing a
// game that is already running, the running-process guard, and watching a
// launch for the game's error boxes.

use std::path::Path;

use native::log_markdown;
use native::patch::PatcherConfig;
use native::patch::launch_settings::RunningGameCheck;
use serde::Deserialize;
use tauri::{Emitter, Manager};

/// After a preview or Launch Game: reports any error box the game, the HLAE
/// launcher or its injector shows (`native::sys::dialogs`) as an
/// `external_error` event, each box once. Report only, unlike a batch: the
/// user is at the game, so the box stays for them to close. Stops once the
/// launcher has exited and no game has been running for 5 seconds.
pub(crate) fn watch_for_error_dialogs(
    app: tauri::AppHandle,
    mut launcher: native::sys::launcher::Launcher,
) {
    std::thread::spawn(move || {
        let launcher_pid = launcher.id();
        let mut launcher_running = true;
        let mut reported: Vec<isize> = Vec::new();
        let mut last_game_seen = std::time::Instant::now();
        let mut sys = native::sys::process::snapshot();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            native::sys::process::refresh(&mut sys);
            if launcher_running && !matches!(launcher.exit_status(&sys), Ok(None)) {
                launcher_running = false;
            }
            let game_pids = native::sys::process::game_pids_in(&sys);
            let mut launcher_pids = native::sys::process::injector_pids_in(&sys, launcher_pid);
            if launcher_running {
                launcher_pids.push(launcher_pid);
            }
            if !game_pids.is_empty() {
                last_game_seen = std::time::Instant::now();
            }
            for dialog in native::sys::dialogs::error_dialogs(&game_pids, &launcher_pids) {
                if reported.contains(&dialog.window) {
                    continue;
                }
                reported.push(dialog.window);
                log_markdown(&format!(
                    "[launch] Error box from PID {}: {}",
                    dialog.pid,
                    dialog.full_text()
                ));
                let _ = app.emit("external_error", dialog.summary());
            }
            if !launcher_running
                && launcher_pids.is_empty()
                && last_game_seen.elapsed() > std::time::Duration::from_secs(5)
            {
                break;
            }
        }
    });
}

/// Process ids of every running `hl.exe` -- the games Studio could be talking
/// to. `hlae.exe` is left out: it is the launcher, not the game.
pub(crate) fn running_game_pids() -> Vec<u32> {
    native::sys::process::game_pids()
}

// ── Standalone Game Launch ──────────────────────────────────────────────────────
//
// Boots the GoldSrc engine environment through HLAE without a demo loaded —
// same `build_hlae_process` plumbing as the preview launchers
// (demo_preview.rs), minus any `+playdemo`/`+viewdemo` engine arg. Reads the persisted `AppSettings`
// (not an IPC payload) since this is a standalone action triggered from the
// global actions area rather than the per-demo detail pane.

/// Launches HLAE directly against `hl.exe` with no demo loaded, applying the
/// persisted resolution/HUD configuration from `AppSettings`. Deliberately
/// does not apply Initial Commands: this button is for quickly opening
/// HLAE+DoD, not for reproducing a capture batch's config state, and Initial
/// Commands lack the guaranteed-last-word property here that they have in a
/// real batch (no demo to STUFFTEXT-inject into, so they would just be raw
/// command-line args that config.cfg/movie.cfg can still override).
#[tauri::command]
pub async fn launch_standalone_game(app: tauri::AppHandle) -> Result<(), String> {
    let settings = saved_settings(&app);

    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        if settings.hlae_path.trim().is_empty() || settings.hl_path.trim().is_empty() {
            return Err(crate::messages::configure_paths_before("launching"));
        }
        if !Path::new(&settings.hlae_path).is_file() {
            return Err(crate::messages::HLAE_NOT_FOUND_AT_CONFIGURED_PATH.to_string());
        }
        if !Path::new(&settings.hl_path).is_file() {
            return Err(crate::messages::HL_NOT_FOUND_AT_CONFIGURED_PATH.to_string());
        }

        let patcher_config = launch_config(&settings, &LaunchRequest::default());
        let mut cmd = patcher_config.build_hlae_process("");
        let launcher = native::sys::launcher::Launcher::spawn(&mut cmd)
            .map_err(crate::messages::failed_to_launch_hlae)?;
        watch_for_error_dialogs(app, launcher);
        // Configuration > Players' box (#428), once the game takes commands.
        crate::players_tab::send_when_listening(settings.players.use_in_game);

        Ok(())
    }))
    .await
}

// ── Reusing a running game (#666) ─────────────────────────────────────────────
//
// Some settings only take effect when the game starts (which install, the
// resolution). Every path that would reuse a running game first checks it
// was started with the settings a launch would use now; one that wasn't is
// closed and started again, after the frontend asks.

pub(crate) fn saved_settings(app: &tauri::AppHandle) -> crate::settings_manager::AppSettings {
    let settings_state = app.state::<crate::settings_manager::SettingsManager>();
    let guard = settings_state
        .inner
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    guard.clone()
}

/// Launch settings a caller has that may not be saved yet: Launch Preview's
/// paths, a capture batch's payload. Anything left out comes from the saved
/// settings, as Launch Game and Review use them.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct LaunchRequest {
    #[serde(default)]
    pub hlae_path: Option<String>,
    #[serde(default)]
    pub game_path: Option<String>,
    #[serde(default)]
    pub goldsrc_hooks_dll_path: Option<String>,
    #[serde(default)]
    pub resolution_width: Option<i32>,
    #[serde(default)]
    pub resolution_height: Option<i32>,
}

/// The config a launch from `settings` with `request` on top starts the game
/// with: Launch Game launches from exactly this, and the running-game check
/// compares against it.
pub(crate) fn launch_config(
    settings: &crate::settings_manager::AppSettings,
    request: &LaunchRequest,
) -> PatcherConfig {
    let pick = |given: &Option<String>, saved: &str| {
        given
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| saved.to_string())
    };
    PatcherConfig {
        hlae_path: pick(&request.hlae_path, &settings.hlae_path),
        game_path: pick(&request.game_path, &settings.hl_path),
        resolution_width: request
            .resolution_width
            .unwrap_or(settings.resolution_width),
        resolution_height: request
            .resolution_height
            .unwrap_or(settings.resolution_height),
        ffmpeg_capture: settings.ffmpeg_capture,
        ffmpeg_capture_codec: native::patch::CaptureCodec::from_str_id(
            &settings.ffmpeg_capture_codec,
        ),
        goldsrc_hooks_dll_path: crate::updater_manager::allowed_hook_dll(
            request
                .goldsrc_hooks_dll_path
                .clone()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| settings.goldsrc_hooks_dll_path.clone()),
        ),
        ..PatcherConfig::default()
    }
}

/// Whether a game is running, and whether it was started with the launch
/// settings `request` (over the saved ones) would start one with.
#[tauri::command]
pub async fn check_running_game(
    app: tauri::AppHandle,
    request: Option<LaunchRequest>,
) -> Result<RunningGameCheck, String> {
    let launch = launch_config(&saved_settings(&app), &request.unwrap_or_default());
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let check = native::patch::launch_settings::check_running_game(&launch.launch_settings());
        if let RunningGameCheck::Mismatch {
            pid,
            running,
            wanted,
            differs,
        } = &check
        {
            log_markdown(&format!(
                "[launch] Game {pid} was started with other settings ({differs:?}): {} at {:?}x{:?}; Studio is set to {} at {}x{}{}",
                running.exe,
                running.width,
                running.height,
                wanted.exe,
                launch.resolution_width,
                launch.resolution_height,
                hook_dll_log(running, wanted, differs),
            ));
        }
        Ok(check)
    }))
    .await
}

/// The hook DLL part of the activity log's mismatch line (#1143), empty when
/// the hook DLL is not what differs.
fn hook_dll_log(
    running: &native::patch::launch_settings::GameLaunch,
    wanted: &native::patch::launch_settings::GameLaunch,
    differs: &[native::patch::launch_settings::LaunchDifference],
) -> String {
    use native::patch::launch_settings::LaunchDifference;
    let side = |launch: &native::patch::launch_settings::GameLaunch| {
        format!(
            "hook build {} ({})",
            launch
                .hook_build
                .as_deref()
                .unwrap_or("from before build ids"),
            launch.hook_dll.as_deref().unwrap_or("?"),
        )
    };
    if differs.contains(&LaunchDifference::HookUnreadable) {
        format!(
            "; which hook DLL the game has could not be read, Studio would launch {}",
            side(wanted)
        )
    } else if differs.contains(&LaunchDifference::HookDll)
        || differs.contains(&LaunchDifference::HookBuild)
    {
        format!(
            "; the game runs {}, Studio would launch {}",
            side(running),
            side(wanted)
        )
    } else {
        String::new()
    }
}

/// The open game a capture batch can run in instead of Force Relaunch
/// (#725), by PID, or `None` when it needs a fresh game. The activity log
/// says why not (`native::patch::batch_reuse`).
#[tauri::command]
pub async fn running_game_for_batch(
    app: tauri::AppHandle,
    request: Option<LaunchRequest>,
    capture_mode: String,
    hook_runs_batch: bool,
) -> Result<Option<u32>, String> {
    let launch = launch_config(&saved_settings(&app), &request.unwrap_or_default());
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use native::patch::batch_reuse::{ReuseFacts, game_to_reuse};
        let running = native::patch::launch_settings::check_running_game(&launch.launch_settings());
        let pid = match &running {
            RunningGameCheck::Match { pid } | RunningGameCheck::Mismatch { pid, .. } => Some(*pid),
            RunningGameCheck::None => None,
        };
        let modules = pid.and_then(native::sys::modules::loaded_modules);
        let pipe_answers = pid.is_some_and(|pid| {
            matches!(
                native::sys::game_remote::send_console_commands(pid, &[]),
                Ok(native::sys::game_remote::Sent::Delivered)
            )
        });
        let hook_dll = launch.goldsrc_hooks_dll();
        let verdict = game_to_reuse(&ReuseFacts {
            hook_runs_batch,
            capture_mode: native::patch::CaptureMode::from_str_id(&capture_mode),
            running: &running,
            hook_dll: hook_dll.as_deref(),
            modules: modules.as_deref(),
            pipe_answers,
        });
        match &verdict {
            Ok(pid) => log_markdown(&format!(
                "[launch] The batch runs in the game that is already open (PID {pid}) (#725)"
            )),
            Err(why) => log_markdown(&format!(
                "[launch] The open game can't take the batch — {why}; asking to start it again"
            )),
        }
        Ok(verdict.ok())
    }))
    .await
}

/// How long a game gets to close before the restart gives up.
const CLOSE_GAME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Closes game `pid` so it can start again with the settings Studio has now,
/// and resolves once it has gone.
#[tauri::command]
pub async fn close_running_game(pid: u32) -> Result<(), String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        log_markdown(&format!(
            "[launch] Closing game {pid} to start it with the current launch settings"
        ));
        if native::sys::process::close_game(pid, CLOSE_GAME_TIMEOUT) {
            Ok(())
        } else {
            Err(crate::messages::GAME_DID_NOT_CLOSE.to_string())
        }
    }))
    .await
}

// ── Running Process Guard ───────────────────────────────────────────────────────
//
// Launching a fresh HLAE Game Capture preview against a demo while a prior
// `hl.exe`/`hlae.exe` instance is still alive corrupts the new session (the
// old instance holds the game's `dod` directory and console). These two
// commands back the pre-flight "Half-Life Preview Detector" modal: check
// before launching, and let the user force-kill stragglers instead of
// hunting them down in Task Manager.

/// The game and its launcher: what Launch Preview and Kill Engine look for.
const ENGINE_PROCESS_NAMES: &[&str] = &["hl.exe", "hlae.exe"];

/// True if any `hl.exe` or `hlae.exe` process is currently running.
#[tauri::command(async)]
pub fn check_engine_processes() -> bool {
    native::sys::process::is_running(ENGINE_PROCESS_NAMES)
}

/// Steam's state before a game launch: "not_running", "signed_out" or
/// "ready". `hl.exe` started without Steam exits straight away, reported
/// only as "Failed to initalize authentication interface".
#[tauri::command]
pub async fn steam_state() -> Result<String, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(|| {
        Ok(native::sys::steam::state().as_str().to_string())
    }))
    .await
}

/// Starts Steam from where it recorded its own install. Steam outlives
/// DoD Studio, so the child is not tracked.
///
/// Handed to `explorer.exe`, which passes it to the running shell and exits,
/// so Steam's parent is the shell rather than DoD Studio. Started directly,
/// Steam joined any job object DoD Studio runs in -- `cargo run`, under
/// `npm run tauri dev`, puts it in one that kills every process inside when
/// cargo exits -- so closing or rebuilding Studio closed Steam too.
#[tauri::command(async)]
pub fn start_steam() -> Result<(), String> {
    let exe = native::sys::steam::steam_exe().ok_or(crate::messages::STEAM_NOT_FOUND)?;
    // The registry spells it `c:/program files (x86)/steam/steam.exe`, and
    // explorer.exe reads a leading `/` in an argument as a switch.
    let exe = exe.to_string_lossy().replace('/', "\\");
    std::process::Command::new("explorer.exe")
        .arg(exe)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{} ({e})", crate::messages::STEAM_NOT_FOUND))
}

/// Aggressively terminates every running `hl.exe`/`hlae.exe` instance.
#[tauri::command(async)]
pub fn kill_engine_processes() -> Result<(), String> {
    let sys = native::sys::process::snapshot();
    for process in sys.processes().values() {
        if native::sys::process::is_named(process, ENGINE_PROCESS_NAMES) && !process.kill() {
            log_markdown(&format!(
                "[engine] Failed to kill engine process pid={}",
                process.pid()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The running-game check compares against what a launch would use: the
    /// caller's unsaved values over the saved settings, blanks ignored (#666).
    #[test]
    fn launch_config_puts_the_callers_values_over_the_saved_ones() {
        let saved = crate::settings_manager::AppSettings {
            hlae_path: "C:/hlae/hlae.exe".into(),
            hl_path: "C:/pre/hl.exe".into(),
            goldsrc_hooks_dll_path: Some("C:/hooks.dll".into()),
            resolution_width: 1920,
            resolution_height: 1080,
            ..Default::default()
        };
        let cfg = launch_config(&saved, &LaunchRequest::default());
        assert_eq!(cfg.game_path, "C:/pre/hl.exe");
        assert_eq!((cfg.resolution_width, cfg.resolution_height), (1920, 1080));
        assert_eq!(cfg.goldsrc_hooks_dll_path.as_deref(), Some("C:/hooks.dll"));

        let request = LaunchRequest {
            game_path: Some("C:/post/hl.exe".into()),
            hlae_path: Some("  ".into()),
            resolution_width: Some(3440),
            resolution_height: Some(1440),
            ..Default::default()
        };
        let cfg = launch_config(&saved, &request);
        assert_eq!(cfg.game_path, "C:/post/hl.exe");
        assert_eq!(cfg.hlae_path, "C:/hlae/hlae.exe");
        assert_eq!((cfg.resolution_width, cfg.resolution_height), (3440, 1440));
    }
}
