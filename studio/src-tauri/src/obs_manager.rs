// obs_manager.rs
// OBS from Configuration: Test OBS Connection, stopping a recording a crashed
// batch left running, and Launch OBS.

use std::path::Path;

use serde::Serialize;
use tauri::Manager;

// ── OBS ────────────────────────────────────────────────────────────────────────

/// What the settings panel shows after a connection test.
///
/// Flattens `ObsPreflight` plus the scene list into one round trip, because the
/// panel needs all of it at once and a second `invoke` would be a second chance
/// to fail.
#[derive(Debug, Serialize)]
pub struct ObsConnectionReport {
    pub connected: bool,
    /// Populated when `connected` is false. Already phrased for the user —
    /// `ObsError`'s `Display` distinguishes a wrong password from a version
    /// problem, and the two want different actions.
    pub error: Option<String>,
    pub obs_version: String,
    pub websocket_version: String,
    pub missing_requests: Vec<String>,
    pub recording: bool,
    pub streaming: bool,
    pub record_directory: String,
    pub canvas: String,
    pub output: String,
    pub fps: f64,
    pub current_scene: String,
    pub current_profile: String,
    pub scene_collection: String,
    pub warnings: Vec<String>,
}

impl ObsConnectionReport {
    fn failed(e: impl std::fmt::Display) -> Self {
        Self {
            connected: false,
            error: Some(e.to_string()),
            obs_version: String::new(),
            websocket_version: String::new(),
            missing_requests: Vec::new(),
            recording: false,
            streaming: false,
            record_directory: String::new(),
            canvas: String::new(),
            output: String::new(),
            fps: 0.0,
            current_scene: String::new(),
            current_profile: String::new(),
            scene_collection: String::new(),
            warnings: Vec::new(),
        }
    }
}

/// Tests the OBS connection, provisions (and switches into) dod-studio' own
/// profile and scene, and reports everything the settings panel shows.
///
/// **Not read-only**, unlike its old contract — this is the "validated" hook
/// point `obs::provision::ensure_dod_studio_setup` runs from (see that
/// module's docs): creates/repairs the dod-studio profile, scene and sources
/// if needed, and switches into them. Refuses first if OBS is already
/// recording or streaming under whatever the user's own profile is, so this
/// never mutates live state out from under something unrelated. Called on
/// every proactive check (mode switch, startup, manual Test Connection
/// click) as well as before a real batch — see main.js's
/// `runObsConnectionTest`.
///
/// `game_width`/`game_height`/`obs_capture_fps` are what the profile's video
/// settings get set to.
#[tauri::command]
pub async fn obs_test_connection(
    host: String,
    port: u16,
    password: String,
    game_width: i32,
    game_height: i32,
    obs_capture_fps: i32,
    obs_recording_quality: Option<String>,
) -> Result<ObsConnectionReport, String> {
    let quality = native::patch::ObsRecordingQuality::from_str_id(
        obs_recording_quality.as_deref().unwrap_or_default(),
    );
    let resolved_host = if host.is_empty() {
        "127.0.0.1".to_string()
    } else {
        host
    };
    let resolved_port = if port == 0 { 4455 } else { port };
    let url = format!("ws://{resolved_host}:{resolved_port}");
    tokio::task::spawn_blocking(move || {
        let mut client = match native::obs::ObsClient::connect(&url, &password) {
            Ok(c) => c,
            // A bare socket error ("actively refused it (os error 10061)") makes
            // the user guess whether that means OBS is closed or just
            // unreachable. `is_transport` narrows it to "never got past the
            // socket/handshake" (as opposed to OBS answering and refusing), and
            // for that case whether the process is even running is a fact we
            // can just check instead of leaving it to the raw OS text.
            Err(e) if e.is_transport() => {
                let msg = if is_obs_process_running() {
                    format!(
                        "OBS is running, but dod-studio can't reach it at {resolved_host}:{resolved_port}. Check Tools -> WebSocket Server Settings is enabled and the port matches."
                    )
                } else {
                    "OBS isn't running. Launch it (or use Launch OBS above), then try again.".to_string()
                };
                return ObsConnectionReport::failed(msg);
            }
            Err(e) => return ObsConnectionReport::failed(e),
        };
        if let Err(e) = client.refuse_if_busy() {
            return ObsConnectionReport::failed(e);
        }
        if let Err(e) = native::obs::provision::ensure_dod_studio_setup(
            &mut client,
            game_width,
            game_height,
            obs_capture_fps,
            1,
            quality,
        ) {
            return ObsConnectionReport::failed(e);
        }
        let preflight = match client.preflight(game_width, game_height) {
            Ok(p) => p,
            Err(e) => return ObsConnectionReport::failed(e),
        };
        ObsConnectionReport {
            connected: true,
            error: None,
            obs_version: preflight.obs_version,
            websocket_version: preflight.websocket_version,
            missing_requests: preflight.missing_requests,
            recording: preflight.recording,
            streaming: preflight.streaming,
            record_directory: preflight.record_directory,
            canvas: format!("{}x{}", preflight.canvas_width, preflight.canvas_height),
            output: format!("{}x{}", preflight.output_width, preflight.output_height),
            fps: preflight.fps,
            current_scene: preflight.current_scene,
            current_profile: preflight.current_profile,
            scene_collection: preflight.scene_collection,
            warnings: preflight.warnings,
        }
    })
    .await
    .map_err(crate::messages::obs_connection_test_failed)
}

/// What a start-up orphan check found.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ObsOrphanReport {
    /// Whether OBS answered at all. `false` is the ordinary case — OBS is
    /// simply not running — and the UI says nothing about it.
    pub reachable: bool,
    pub recording: bool,
    pub directory: String,
    /// Whether the folder OBS is recording into could only be one of ours.
    pub ours: bool,
}

/// Asks OBS whether it is still recording a dod-studio take from a previous run.
///
/// Read-only, and quiet about failure on purpose: this runs at start-up, where
/// "OBS is not running" is the normal answer and not something to report.
///
/// The case it exists for is the one no `Drop` can cover — a panic (release
/// builds abort rather than unwind), a hard kill, a `tauri dev` restart, a
/// power cut. In all of those the process is simply gone and OBS keeps
/// recording until the disk fills.
#[tauri::command]
pub async fn obs_check_orphan(
    host: String,
    port: u16,
    password: String,
) -> Result<ObsOrphanReport, String> {
    let cfg = obs_config(host, port, password);
    tokio::task::spawn_blocking(move || match native::obs::check_orphan(&cfg) {
        Ok(report) => ObsOrphanReport {
            reachable: true,
            recording: report.recording,
            directory: report.directory,
            ours: report.ours,
        },
        Err(_) => ObsOrphanReport {
            reachable: false,
            recording: false,
            directory: String::new(),
            ours: false,
        },
    })
    .await
    .map_err(crate::messages::obs_orphan_check_failed)
}

/// Stops an orphaned recording and folds its file into the take folder.
///
/// Refuses any directory that is not shaped like one of ours, so a user who
/// was recording something of their own keeps it. Called only after
/// `obs_check_orphan` has reported `ours`, and asked of the user first.
#[tauri::command]
pub async fn obs_recover_orphan(
    host: String,
    port: u16,
    password: String,
) -> Result<String, String> {
    let cfg = obs_config(host, port, password);
    tokio::task::spawn_blocking(move || match native::obs::recover_orphan(&cfg) {
        Ok(Some(video)) => Ok(video.to_string_lossy().to_string()),
        Ok(None) => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    })
    .await
    .map_err(crate::messages::obs_orphan_recovery_failed)?
}

/// Settings-shaped values from the frontend, with the same defaults the
/// settings file uses so an empty field means "the usual" rather than "".
fn obs_config(host: String, port: u16, password: String) -> native::patch::ObsConfig {
    native::patch::ObsConfig {
        host: if host.is_empty() {
            "127.0.0.1".to_string()
        } else {
            host
        },
        port: if port == 0 { 4455 } else { port },
        password,
        // Only connection checks and recovery use this; neither provisions.
        recording_quality: Default::default(),
    }
}

/// What "Launch OBS" passes OBS. `--disable-updater` stops OBS's "New update
/// available" box, which otherwise opens in front of the user's windows while
/// a batch keeps the game behind them (#645). Not `--minimize-to-tray`: OBS's
/// websocket works from the tray (measured, OBS 32.2.2), but this button is
/// there to open OBS, often to set that websocket up in the first place.
const OBS_LAUNCH_ARGS: &[&str] = &["--disable-updater"];

/// Launches OBS Studio from the configured `obs_exe_path`, spawn-and-forget —
/// same shape as `launch_standalone_game`, minus the whole HLAE/hl.exe
/// process-building step, since this just starts the user's own program.
/// No lifecycle tracking: OBS is the user's software, not ours to manage,
/// and this button exists only so the user doesn't have to alt-tab to a
/// shortcut before configuring the connection above it.
#[tauri::command]
pub async fn launch_obs(app: tauri::AppHandle) -> Result<(), String> {
    let settings_state = app.state::<crate::settings_manager::SettingsManager>();
    let obs_exe_path = {
        let guard = settings_state
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        guard.obs_exe_path.clone()
    };

    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        if obs_exe_path.trim().is_empty() {
            return Err(crate::messages::CONFIGURE_OBS_PATH_BEFORE_LAUNCHING.to_string());
        }
        if !Path::new(&obs_exe_path).is_file() {
            return Err(crate::messages::OBS_NOT_FOUND_AT_CONFIGURED_PATH.to_string());
        }
        let mut cmd = std::process::Command::new(&obs_exe_path);
        // Without this OBS inherits dod-studio' own CWD instead of its own
        // install directory, and fails to find its own relative-pathed data
        // (locale/en-US.ini, etc.) — measured, not theoretical. Same fix
        // `build_hlae_process` already applies for HLAE/hl.exe.
        if let Some(parent) = Path::new(&obs_exe_path).parent() {
            cmd.current_dir(parent);
        }
        cmd.args(OBS_LAUNCH_ARGS);
        cmd.spawn().map_err(crate::messages::failed_to_launch_obs)?;
        Ok(())
    }))
    .await
}

/// True if an OBS Studio process is currently running, under any of its
/// installed binary names. Used to turn a bare "could not connect" into a
/// deterministic answer instead of asking the user to interpret a raw OS
/// socket error themselves.
fn is_obs_process_running() -> bool {
    native::sys::process::is_running(&["obs64.exe", "obs32.exe", "obs.exe"])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OBS's update box opens in front of the user's windows; the minimised
    /// tray start would hide the window this button exists to open.
    #[test]
    fn launch_obs_disables_the_updater_and_keeps_the_window() {
        assert!(OBS_LAUNCH_ARGS.contains(&"--disable-updater"));
        assert!(!OBS_LAUNCH_ARGS.contains(&"--minimize-to-tray"));
    }
}
