// messages.rs
// Rust-side counterpart to studio/src/strings.js: the Tauri command
// layer's own authored user-facing error/status text, centralized so the
// same message can't drift into two different wordings at two call sites
// (see the `background_task_crashed` cluster below, previously duplicated
// verbatim 13 times across 4 files) and shared boilerplate lives in exactly
// one place. Scope matches strings.js's own: text that reaches the frontend
// as a toast/dialog string. Developer-only log::error!/eprintln!
// diagnostics stay inline at their call site, same as strings.js leaves
// console.log/console.error alone. Issue #33, track 2 — a parallel catalog
// to strings.js, not a merge into it (Rust and JS strings are never shared
// cross-language here).
//
// Covers every Tauri command file's error/status strings: lib.rs,
// capture_manager.rs, render_manager.rs, audit_manager.rs, map_manager.rs,
// hd_manager.rs, updater_manager.rs, settings_manager.rs, and
// dir_browser.rs. Not covered: native/analysis errors that bubble straight
// through Tauri commands unwrapped (a separate crate — see issue #650).

use std::fmt::Display;
use std::path::Path;

// ── spawn_blocking join-error flattening ────────────────────────────────────
//
// Two closure shapes appear at Tauri command call sites: one whose closure
// itself returns `Result<T, String>` (needs the outer JoinError flattened
// into that same Result via `?`), and one whose closure returns a plain `T`
// (the JoinError becomes the whole function's error case directly, no `?`
// needed). Each gets its own helper rather than forcing one shape to fit
// the other.

/// For a `spawn_blocking` closure returning `Result<T, String>`. Awaits the
/// handle and flattens `Result<Result<T, String>, JoinError>` into
/// `Result<T, String>`, converting a join failure (the blocking task itself
/// panicked) into the same error-string shape as any other command failure.
pub async fn flatten_spawn_blocking<T>(
    handle: tokio::task::JoinHandle<Result<T, String>>,
) -> Result<T, String> {
    handle.await.map_err(background_task_crashed)?
}

/// For a `spawn_blocking` closure returning a plain `T` (no inner Result).
/// Awaits the handle and turns a join failure into the same error-string
/// shape the rest of the command layer uses.
pub async fn spawn_blocking_result<T>(handle: tokio::task::JoinHandle<T>) -> Result<T, String> {
    handle.await.map_err(background_task_crashed)
}

/// A `spawn_blocking` task that panicked or was cancelled.
pub fn background_task_crashed(err: impl Display) -> String {
    format!("Background task crashed: {}", err)
}

// ── lib.rs ───────────────────────────────────────────────────────────────

/// `default_projects_dir` when Windows reports no Documents folder.
pub const NO_DOCUMENTS_FOLDER: &str = "Could not find the Documents folder";

/// Generic "Failed to write <path>: <err>" — same shape independently
/// authored at least twice (lib.rs's project session save, plus whatever
/// else reads a user-given path); reused rather than re-typed per call site.
pub fn failed_to_write_file(path: &str, err: impl Display) -> String {
    format!("Failed to write {}: {}", path, err)
}

/// See [`failed_to_write_file`] — the read-side counterpart, same reuse
/// rationale (lib.rs's project session load, dir_browser.rs's directory
/// listing).
pub fn failed_to_read_file(path: &str, err: impl Display) -> String {
    format!("Failed to read {}: {}", path, err)
}

pub const HLAE_EXECUTABLE_NOT_FOUND: &str = "HLAE executable not found at the specified path.";
pub const HL_EXECUTABLE_NOT_FOUND: &str = "Half-Life executable not found at the specified path.";
pub const STEAM_NOT_FOUND: &str =
    "Couldn't find Steam to start it. Start Steam yourself, then try again.";

pub fn demo_file_not_found(demo_path: &str) -> String {
    format!("Demo file not found: {}", demo_path)
}

/// The Demo Auditor's renamer was given a folder that isn't there (#469).
pub fn rename_folder_not_found(folder: &str) -> String {
    format!("Folder not found: {folder}")
}

// Why a scan skipped a demo, in the user's words (#23). The scanner's own
// error text ("failed to fill whole buffer") still goes to the log.
/// The Demo Analyzer's Cache all, clicked while a run is going (#569).
pub const DEMO_CACHE_ALREADY_RUNNING: &str = "Already caching demos; stop that run first.";
pub const SCAN_FAIL_TOO_SHORT: &str = "File too short to be a demo";
pub const SCAN_FAIL_NOT_A_DEMO: &str = "Not a Half-Life demo (bad header)";
pub const SCAN_FAIL_CORRUPT: &str = "Demo is corrupt partway through";

pub fn scan_path_not_found(path: &str) -> String {
    format!("Not found: {path} (moved or deleted?)")
}

pub fn not_a_directory(path: &str) -> String {
    format!("Not a directory: {}", path)
}

pub fn analyzer_error(err: impl Display) -> String {
    format!("Analyzer error: {}", err)
}

pub fn ffmpeg_could_not_be_resolved(ffmpeg_path: &str) -> String {
    format!(
        "Could not find FFmpeg from \"{}\": no such file, and no ffmpeg.exe on the Windows \
         PATH. Pick your ffmpeg.exe first (Setup's FFmpeg step, or Configuration > Paths > \
         FFmpeg Override Path): HLAE's ini needs a full path.",
        ffmpeg_path
    )
}

// ── blender_manager.rs ──────────────────────────────────────────────────────

pub const BLENDER_NEEDS_GAME_PATH: &str = "Set the Half-Life Executable in Configuration → Paths first: the maps and HD textures come from its dod folder.";
pub const BLENDER_ALREADY_RUNNING: &str =
    "A Blender step is already running. Wait for it, or Cancel it.";
pub const BLENDER_NO_SCRIPTS: &str =
    "This copy of DoD Studio has no Blender scripts (the blender folder in the DoD Studio repo).";
pub const BLENDER_NOT_FOUND: &str = "No Blender found: choose blender.exe (Blender 4.4).";

pub fn blender_not_blender(path: &str) -> String {
    format!("{path} is not blender.exe")
}

// ── hd_manager.rs ───────────────────────────────────────────────────────────

pub const HD_NEEDS_GAME_PATH: &str = "Set the Half-Life Executable in Configuration → Paths first: the HD files live in its dod folder.";
pub const HD_ALREADY_RUNNING: &str =
    "A download or build is already running. Wait for it, or Cancel it.";
pub const HD_COVERAGE_RUNNING: &str = "Studio is already checking which maps each style covers.";
pub const HD_NO_SCRIPTS: &str =
    "This copy of DoD Studio has no HD build scripts (goldsrc-hooks/tools/hd).";
pub const HD_NO_PYTHON: &str =
    "No Python to run the build with: use Download, or choose a python.exe.";

pub fn hd_not_python(path: &str) -> String {
    format!("{path} didn't run as Python 3")
}

// ── capture_manager.rs ──────────────────────────────────────────────────────

pub const CAPTURE_BATCH_ALREADY_RUNNING: &str = "Capture batch already in progress";
pub const NO_STREAKS_IN_PAYLOAD: &str = "No streaks in payload";

pub fn configure_paths_before(action: &str) -> String {
    format!(
        "Configure the HLAE and Half-Life executable paths before {}.",
        action
    )
}

pub const HLAE_NOT_FOUND_AT_CONFIGURED_PATH: &str =
    "HLAE executable not found at the configured path.";
pub const HL_NOT_FOUND_AT_CONFIGURED_PATH: &str =
    "Half-Life executable not found at the configured path.";
pub const NO_HIGHLIGHTS_TO_PREVIEW: &str = "This demo has no highlights to preview.";
pub const FAILED_TO_BUILD_PREVIEW_JOBS: &str = "Failed to build any preview patch jobs.";
pub const CONFIGURE_OBS_PATH_BEFORE_LAUNCHING: &str =
    "Configure the OBS executable path before launching.";
pub const OBS_NOT_FOUND_AT_CONFIGURED_PATH: &str =
    "OBS executable not found at the configured path.";

pub fn obs_connection_test_failed(err: impl Display) -> String {
    format!("OBS connection test failed to run: {}", err)
}

pub fn obs_orphan_check_failed(err: impl Display) -> String {
    format!("OBS orphan check failed to run: {}", err)
}

pub fn obs_orphan_recovery_failed(err: impl Display) -> String {
    format!("OBS orphan recovery failed to run: {}", err)
}

// ── OBS capture test (#646) ──
pub const OBS_TEST_NO_DEMO: &str = "The OBS capture test's demo isn't in DoD Studio's install folder. Reinstall DoD Studio to get it back.";

pub fn obs_test_unreadable_demo(path: &Path, err: impl Display) -> String {
    format!("Could not read the test demo {}: {err}", path.display())
}

pub fn obs_test_no_clip(path: &Path, seconds: f32) -> String {
    format!(
        "{} has no kill with {seconds:.0} s of demo around it to record.",
        path.display()
    )
}

pub fn obs_test_no_video(folder: &Path) -> String {
    format!(
        "OBS recorded no video into {}. Check OBS's own window for why.",
        folder.display()
    )
}

pub fn obs_test_not_a_test_folder(folder: &str) -> String {
    format!("{folder} isn't an OBS capture test's folder, so it was not deleted.")
}

pub fn obs_test_delete_failed(folder: &Path, err: impl Display) -> String {
    format!("Could not delete the test clip {}: {err}", folder.display())
}

pub fn failed_to_create_dod_directory(err: impl Display) -> String {
    format!(
        "Failed to create the game folder `{}` beside hl.exe: {}",
        native::shared::paths::game_folder(),
        err
    )
}

pub fn failed_to_patch_preview_demo(source_demo: &str, err: impl Display) -> String {
    format!("Failed to patch preview demo for {}: {}", source_demo, err)
}

pub fn failed_to_write_preview_sidecar(err: impl Display) -> String {
    format!("Failed to write preview sidecar: {}", err)
}

pub fn failed_to_launch_hlae_for_preview(err: impl Display) -> String {
    format!("Failed to launch HLAE for preview: {}", err)
}

pub fn failed_to_launch_hlae(err: impl Display) -> String {
    format!("Failed to launch HLAE: {}", err)
}

pub fn failed_to_launch_obs(err: impl Display) -> String {
    format!("Failed to launch OBS: {}", err)
}

/// The batch status line while the game fast-forwards to, then records, a
/// clip (#76) of the demo `demo` (#952). 1-based.
pub fn capturing_clip(clip: u32, total: u32, demo: &str) -> String {
    if demo.is_empty() {
        format!("Capturing clip {clip} of {total}")
    } else {
        format!("Capturing clip {clip} of {total} ({demo})")
    }
}

/// The batch status line from the click that opens the game until its first
/// demo loads (#952). `first_demo` is the batch's first demo, `demos` how
/// many it plays.
pub fn starting_the_game(first_demo: Option<&str>, demos: usize) -> String {
    match first_demo {
        None => "Starting the game…".to_string(),
        Some(name) if demos > 1 => format!("Starting the game for {name} (demo 1 of {demos})"),
        Some(name) => format!("Starting the game for {name}"),
    }
}

/// The batch status line while the game loads a demo (#952).
pub fn loading_demo(demo: &str) -> String {
    if demo.is_empty() {
        "Loading the next demo…".to_string()
    } else {
        format!("Loading {demo}…")
    }
}

/// The batch status line while the takes are checked on disk after the
/// game closes (#952).
pub fn checking_takes(done: usize, total: usize) -> String {
    format!("Checking the takes: {done} of {total}")
}

/// The batch status line while a demo whose highlights have no demo-player
/// times is read again before a Seek Between Clips batch (#434). 1-based.
pub fn reading_demo_again(demo: &str, index: u32, total: u32) -> String {
    format!("Reading {demo} again for its clip times ({index} of {total})")
}

/// The activity-log line when a Seek Between Clips batch falls back to
/// patching and some of its Initial or Scheduled Commands are ones the
/// game's demo filter drops (#679), or `None` when there are none.
pub fn dropped_when_patched(dropped: &[native::patch::cfg_scan::DemoFiltered]) -> Option<String> {
    if dropped.is_empty() {
        return None;
    }
    let list = dropped
        .iter()
        .map(|d| format!("`{}`", d.command))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "⚠️ **Commands dropped** — a patched batch puts Initial and Scheduled Commands in the demo, \
         and the game drops these from a demo without running them: {list}"
    ))
}

pub fn game_directory_not_found(game_dir: &str) -> String {
    format!("Game directory not found: {}", game_dir)
}

/// Same text independently authored twice (`resolve_preview_env` and
/// `resolve_dod_dir_for_sweep`) — both derive `dod_dir` from `hl.exe`'s
/// parent and fail identically when that parent can't be resolved.
pub const COULD_NOT_RESOLVE_DOD_DIRECTORY: &str =
    "Could not resolve the game folder next to hl.exe";
pub const FAILED_TO_BUILD_PREVIEW_PATCH_JOB: &str = "Failed to build the preview patch job";

pub fn failed_to_send_to_running_game(e: impl std::fmt::Display) -> String {
    format!("Could not send the preview to the running game: {e}")
}

// ── review_manager.rs (#623) ────────────────────────────────────────────────
pub fn failed_to_send_review(e: impl std::fmt::Display) -> String {
    format!("Could not send the review to the game: {e}")
}
pub fn could_not_write_review_queue(e: impl std::fmt::Display) -> String {
    format!("Could not save the list of highlights to review: {e}")
}
pub const REVIEW_GAME_NOT_FROM_STUDIO: &str = "Day of Defeat is open, but DoD Studio didn't start it, so it can't play the review. Close the game and press Review highlights again.";
/// Review reached a game started with other launch settings without the
/// frontend's ask first (#666).
pub const REVIEW_GAME_LAUNCH_SETTINGS_DIFFER: &str = "Day of Defeat is open with other launch settings than DoD Studio's (another install or resolution). Close the game and press Review highlights again.";
pub const GAME_DID_NOT_CLOSE: &str = "Day of Defeat didn't close within 10 seconds, so nothing was started. Close it yourself, then try again.";
pub const REVIEW_GAME_DID_NOT_START: &str = "Day of Defeat didn't open within two minutes. Start it with Launch Game, then press Review highlights again.";
pub const COULD_NOT_RESOLVE_PREVIEW_FILE_STEM: &str =
    "Could not resolve the preview demo's file stem";

pub fn failed_to_read_dod_directory(err: impl Display) -> String {
    format!("Failed to read dod directory: {}", err)
}

/// The batch status line from the moment the batch starts until a step
/// with a count says more (#952): checking the demos are unchanged, reading
/// any again, copying them beside the game.
pub const GETTING_DEMOS_READY: &str = "Getting demos ready…";

/// The batch status line while demos are patched (#75), in plain words
/// (#952): `running` is each in-flight demo's name and its percent written,
/// `None` while it is in a step that reports none (the decal pass, whose
/// name stays out of the line).
pub fn preparing_status(done: u32, total: usize, running: &[(&str, Option<u8>)]) -> String {
    let head = "Getting demos ready";
    match running {
        [] => format!("{head}: {done} of {total}"),
        [(name, percent)] => {
            let number = (done as usize + 1).min(total);
            match percent {
                Some(p) => format!("{head}: {number} of {total} ({name}, {p}%)"),
                None => format!("{head}: {number} of {total} ({name})"),
            }
        }
        _ => {
            let writing: Vec<u8> = running.iter().filter_map(|(_, p)| *p).collect();
            let at_once = running.len();
            if writing.is_empty() {
                format!("{head}: {done} of {total} done, {at_once} at once")
            } else {
                let average = writing.iter().map(|&p| p as usize).sum::<usize>() / writing.len();
                format!("{head}: {done} of {total} done, {at_once} at once ({average}% on average)")
            }
        }
    }
}

// ── render_manager.rs ────────────────────────────────────────────────────

pub const RENDER_SCAN_ALREADY_RUNNING: &str =
    "A scan for takes is already running — wait for it to finish.";
pub const RENDER_BATCH_ALREADY_IN_PROGRESS: &str = "Render batch already in progress";
pub const NOTHING_QUEUED_TO_RENDER: &str = "Nothing queued to render — scan for takes first.";
pub const SKIP_ONLY_FOR_OBS_TAKE: &str = "Skip (keep original) is only available for a captured OBS take (its own audio, not a HUD/alpha clip).";

/// Start on a recovered batch whose autosave lacks something a render needs
/// (#770): the rows show, but only a fresh scan can fill `what` in.
pub fn recovered_batch_needs_scan(what: &str) -> String {
    format!(
        "recovered batch: scan again to set {} (Clear Queue, then Scan for Takes).",
        what
    )
}

pub fn no_such_job(job_id: &str) -> String {
    format!("No such job: {}", job_id)
}

pub fn job_not_queued(job_id: &str, status: impl Display) -> String {
    format!(
        "Job {} is {} — only a Queued job's codec can be changed",
        job_id, status
    )
}

pub fn job_still_rendering(job_id: &str) -> String {
    format!("Job {} is still rendering — cancel it first", job_id)
}

// ── take_delete_manager.rs (#644) ───────────────────────────────────────

pub fn take_not_listed(take_folder: &str) -> String {
    format!(
        "{} isn't in the Render tab's list, so it was not deleted",
        take_folder
    )
}

pub fn take_is_rendering(take_folder: &str) -> String {
    format!(
        "{} is rendering. Cancel it first, then delete the take.",
        take_folder
    )
}

pub const NO_TAKES_TO_DELETE: &str = "No takes were picked to delete.";

// ── audit_manager.rs ─────────────────────────────────────────────────────

/// A path as Explorer shows it: the audit's canonical paths carry a `\\?\`
/// (or `\\?\UNC\`) prefix, which the Duplicates table hides too.
fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text
    }
}

pub fn failed_to_delete_audit_file(path: &Path, err: impl Display) -> String {
    format!("Failed to delete {}: {}", shown(path), err)
}

/// A copy kept because it isn't byte for byte the copy its group keeps:
/// the audit's key reads only the first 64 KiB.
pub fn copy_differs_from_kept(copy: &Path, keep: &Path, offset: u64) -> String {
    format!(
        "Not deleted: {} is not an exact copy of {} (they differ from byte {}), so it may be a different demo",
        shown(copy),
        shown(keep),
        offset
    )
}

pub fn kept_copy_missing(copy: &Path, keep: &Path) -> String {
    format!(
        "Not deleted: {} could not be checked against the copy being kept, {}, which is no longer there",
        shown(copy),
        shown(keep)
    )
}

pub fn copy_is_the_kept_copy(copy: &Path) -> String {
    format!("Not deleted: {} is the copy being kept", shown(copy))
}

pub fn copy_compare_failed(copy: &Path, keep: &Path, err: impl Display) -> String {
    format!(
        "Not deleted: {} could not be compared with {}: {}",
        shown(copy),
        shown(keep),
        err
    )
}

/// The Duplicates delete only removes `.dem` files (#844).
pub fn not_a_demo_file(path: &Path) -> String {
    format!(
        "Not deleted: {} is not a demo file, or is no longer there",
        shown(path)
    )
}

pub fn audit_folder_not_found(path: &Path) -> String {
    format!("Folder not found: {}", path.display())
}

pub const NO_AUDIT_FOLDERS: &str = "Add a folder to check, or choose All drives.";

pub fn path_no_longer_exists(path: &str) -> String {
    format!("Path no longer exists: {}", path)
}

pub fn failed_to_open_explorer(err: impl Display) -> String {
    format!("Failed to open Explorer: {}", err)
}

// These three are the macOS and Linux arms of `audit_manager::reveal_in_explorer`
// (its `#[cfg(target_os = "macos")]` and `#[cfg(not(any(windows, macos)))]`
// blocks). They are genuinely reachable there, but every build this repo
// actually produces is Windows, so rustc compiles their only call sites out and
// reports them as dead. Allowed rather than gated, so the tests below keep
// covering the wording on every platform.
#[allow(dead_code)]
pub fn failed_to_open_finder(err: impl Display) -> String {
    format!("Failed to open Finder: {}", err)
}

#[allow(dead_code)]
pub const NO_PARENT_DIRECTORY_FOR_PATH: &str = "No parent directory for path";

#[allow(dead_code)]
pub fn failed_to_open_folder(err: impl Display) -> String {
    format!("Failed to open folder: {}", err)
}

// ── map_manager.rs ───────────────────────────────────────────────────────

pub fn no_map_folder_beside_exe(game_path: &str) -> String {
    format!(
        "No map folder beside `{}` — maps are expected at `<hl.exe folder>/{}/maps`",
        game_path,
        native::shared::paths::game_folder()
    )
}

pub fn map_check_failed(err: impl Display) -> String {
    format!("Map check failed: {}", err)
}

pub fn config_scan_failed(err: impl Display) -> String {
    format!("Config scan failed: {}", err)
}

pub fn map_download_failed(err: impl Display) -> String {
    format!("Map download failed: {}", err)
}

// ── updater_manager.rs ───────────────────────────────────────────────────

pub fn unknown_update_channel(channel: &str) -> String {
    format!("Unknown update channel: {}", channel)
}

pub fn invalid_updater_endpoint_url(err: impl Display) -> String {
    format!("Invalid updater endpoint URL: {}", err)
}

pub fn failed_to_set_updater_endpoint(err: impl Display) -> String {
    format!("Failed to set updater endpoint: {}", err)
}

pub fn failed_to_build_updater(err: impl Display) -> String {
    format!("Failed to build updater: {}", err)
}

pub const NO_UPDATE_AVAILABLE_TO_INSTALL: &str =
    "No update available to install — check for updates first";

pub const LOCAL_BUILD_CANNOT_INSTALL_UPDATE: &str = "This is a local or debug build, so updates aren't installed from here: installing would replace your installed DoD Studio, not this copy";

// ── settings_manager.rs ──────────────────────────────────────────────────

pub fn failed_to_serialize_settings(err: impl Display) -> String {
    format!("Failed to serialize settings: {}", err)
}

pub fn failed_to_write_settings_file(path: &std::path::Path, err: impl Display) -> String {
    format!("Failed to write settings file {}: {}", path.display(), err)
}

/// Pins every function/constant above against the exact `format!`/literal it
/// replaced at its original call site (text copied verbatim from the pre-PR
/// source, not re-derived), so this refactor can't have silently changed any
/// user-visible wording.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_when_patched_lists_each_command_or_says_nothing() {
        assert_eq!(dropped_when_patched(&[]), None);
        let dropped = native::patch::cfg_scan::demo_filtered_commands(&[
            "exec movie.cfg".to_string(),
            "sensitivity 2".to_string(),
            "quit".to_string(),
        ]);
        let line = dropped_when_patched(&dropped).unwrap();
        assert!(line.ends_with("`exec movie.cfg`, `quit`"), "{line}");
    }

    #[test]
    fn lib_rs_messages_match_their_original_inline_text() {
        // failed_to_write_file/failed_to_read_file were originally
        // project_session_write_failed/project_session_read_failed —
        // generalized (part 2) once dir_browser.rs turned out to need the
        // identical "Failed to read {}: {}" shape independently.
        assert_eq!(
            failed_to_write_file("proj.json", "disk full"),
            format!("Failed to write {}: {}", "proj.json", "disk full")
        );
        assert_eq!(
            failed_to_read_file("proj.json", "not found"),
            format!("Failed to read {}: {}", "proj.json", "not found")
        );
        assert_eq!(
            HLAE_EXECUTABLE_NOT_FOUND,
            "HLAE executable not found at the specified path."
        );
        assert_eq!(
            HL_EXECUTABLE_NOT_FOUND,
            "Half-Life executable not found at the specified path."
        );
        assert_eq!(
            demo_file_not_found("demo.dem"),
            format!("Demo file not found: {}", "demo.dem")
        );
        assert_eq!(
            not_a_directory("C:/some/file.txt"),
            format!("Not a directory: {}", "C:/some/file.txt")
        );
        assert_eq!(
            analyzer_error("bad header"),
            format!("Analyzer error: {}", "bad header")
        );
        assert_eq!(
            ffmpeg_could_not_be_resolved("ffmpeg"),
            format!(
                "Could not find FFmpeg from \"{}\": no such file, and no ffmpeg.exe on the Windows \
                 PATH. Pick your ffmpeg.exe first (Setup's FFmpeg step, or Configuration > Paths > \
                 FFmpeg Override Path): HLAE's ini needs a full path.",
                "ffmpeg"
            )
        );
    }

    #[test]
    fn capture_manager_messages_match_their_original_inline_text() {
        assert_eq!(
            CAPTURE_BATCH_ALREADY_RUNNING,
            "Capture batch already in progress"
        );
        assert_eq!(NO_STREAKS_IN_PAYLOAD, "No streaks in payload");
        assert_eq!(
            configure_paths_before("previewing"),
            format!(
                "Configure the HLAE and Half-Life executable paths before {}.",
                "previewing"
            )
        );
        assert_eq!(
            configure_paths_before("launching"),
            format!(
                "Configure the HLAE and Half-Life executable paths before {}.",
                "launching"
            )
        );
        assert_eq!(
            HLAE_NOT_FOUND_AT_CONFIGURED_PATH,
            "HLAE executable not found at the configured path."
        );
        assert_eq!(
            HL_NOT_FOUND_AT_CONFIGURED_PATH,
            "Half-Life executable not found at the configured path."
        );
        assert_eq!(
            NO_HIGHLIGHTS_TO_PREVIEW,
            "This demo has no highlights to preview."
        );
        assert_eq!(
            FAILED_TO_BUILD_PREVIEW_JOBS,
            "Failed to build any preview patch jobs."
        );
        assert_eq!(
            CONFIGURE_OBS_PATH_BEFORE_LAUNCHING,
            "Configure the OBS executable path before launching."
        );
        assert_eq!(
            OBS_NOT_FOUND_AT_CONFIGURED_PATH,
            "OBS executable not found at the configured path."
        );
        assert_eq!(
            obs_connection_test_failed("timeout"),
            format!("OBS connection test failed to run: {}", "timeout")
        );
        assert_eq!(
            obs_orphan_check_failed("timeout"),
            format!("OBS orphan check failed to run: {}", "timeout")
        );
        assert_eq!(
            obs_orphan_recovery_failed("timeout"),
            format!("OBS orphan recovery failed to run: {}", "timeout")
        );
        assert_eq!(
            failed_to_create_dod_directory("access denied"),
            "Failed to create the game folder `dod` beside hl.exe: access denied"
        );
        assert_eq!(
            failed_to_patch_preview_demo("demo1.dem", "bad frame"),
            format!(
                "Failed to patch preview demo for {}: {}",
                "demo1.dem", "bad frame"
            )
        );
        assert_eq!(
            failed_to_write_preview_sidecar("disk full"),
            format!("Failed to write preview sidecar: {}", "disk full")
        );
        assert_eq!(
            failed_to_launch_hlae_for_preview("not found"),
            format!("Failed to launch HLAE for preview: {}", "not found")
        );
        assert_eq!(
            failed_to_launch_hlae("not found"),
            format!("Failed to launch HLAE: {}", "not found")
        );
        assert_eq!(
            failed_to_launch_obs("not found"),
            format!("Failed to launch OBS: {}", "not found")
        );
        assert_eq!(
            game_directory_not_found("C:/games/dod"),
            format!("Game directory not found: {}", "C:/games/dod")
        );
        assert_eq!(
            COULD_NOT_RESOLVE_DOD_DIRECTORY,
            "Could not resolve the game folder next to hl.exe"
        );
        assert_eq!(
            FAILED_TO_BUILD_PREVIEW_PATCH_JOB,
            "Failed to build the preview patch job"
        );
        assert_eq!(
            failed_to_send_to_running_game("pipe closed"),
            "Could not send the preview to the running game: pipe closed"
        );
        assert_eq!(
            COULD_NOT_RESOLVE_PREVIEW_FILE_STEM,
            "Could not resolve the preview demo's file stem"
        );
        assert_eq!(
            failed_to_read_dod_directory("access denied"),
            format!("Failed to read dod directory: {}", "access denied")
        );
    }

    #[test]
    fn render_manager_messages_match_their_original_inline_text() {
        assert_eq!(
            RENDER_BATCH_ALREADY_IN_PROGRESS,
            "Render batch already in progress"
        );
        assert_eq!(
            NOTHING_QUEUED_TO_RENDER,
            "Nothing queued to render — scan for takes first."
        );
        assert_eq!(
            SKIP_ONLY_FOR_OBS_TAKE,
            "Skip (keep original) is only available for a captured OBS take (its own audio, not a HUD/alpha clip)."
        );
        assert_eq!(no_such_job("job-1"), format!("No such job: {}", "job-1"));
        assert_eq!(
            job_not_queued("job-1", "Rendering"),
            format!(
                "Job {} is {} — only a Queued job's codec can be changed",
                "job-1", "Rendering"
            )
        );
        assert_eq!(
            job_still_rendering("job-1"),
            format!("Job {} is still rendering — cancel it first", "job-1")
        );
    }

    #[test]
    fn audit_manager_messages_match_their_original_inline_text() {
        assert_eq!(
            failed_to_delete_audit_file(Path::new("demo.dem"), "in use"),
            format!("Failed to delete {}: {}", "demo.dem", "in use")
        );
        assert_eq!(
            not_a_demo_file(Path::new("a.txt")),
            "Not deleted: a.txt is not a demo file, or is no longer there"
        );
        // The audit's canonical paths show without their `\\?\` prefix.
        assert_eq!(
            copy_is_the_kept_copy(Path::new(r"\\?\C:\d\a.dem")),
            r"Not deleted: C:\d\a.dem is the copy being kept"
        );
        assert_eq!(
            not_a_demo_file(Path::new(r"\\?\UNC\server\share\a.dem")),
            r"Not deleted: \\server\share\a.dem is not a demo file, or is no longer there"
        );
        assert_eq!(
            audit_folder_not_found(Path::new("X:")),
            "Folder not found: X:"
        );
        assert_eq!(
            path_no_longer_exists("demo.dem"),
            format!("Path no longer exists: {}", "demo.dem")
        );
        assert_eq!(
            failed_to_open_explorer("not found"),
            format!("Failed to open Explorer: {}", "not found")
        );
        assert_eq!(
            failed_to_open_finder("not found"),
            format!("Failed to open Finder: {}", "not found")
        );
        assert_eq!(NO_PARENT_DIRECTORY_FOR_PATH, "No parent directory for path");
        assert_eq!(
            failed_to_open_folder("not found"),
            format!("Failed to open folder: {}", "not found")
        );
    }

    #[test]
    fn map_manager_messages_match_their_original_inline_text() {
        assert_eq!(
            no_map_folder_beside_exe("C:/games/dod/hl.exe"),
            format!(
                "No map folder beside `{}` — maps are expected at `<hl.exe folder>/dod/maps`",
                "C:/games/dod/hl.exe"
            )
        );
        assert_eq!(
            map_check_failed("panic"),
            format!("Map check failed: {}", "panic")
        );
        assert_eq!(
            config_scan_failed("panic"),
            format!("Config scan failed: {}", "panic")
        );
        assert_eq!(
            map_download_failed("panic"),
            format!("Map download failed: {}", "panic")
        );
    }

    #[test]
    fn updater_manager_messages_match_their_original_inline_text() {
        assert_eq!(
            unknown_update_channel("nightly"),
            format!("Unknown update channel: {}", "nightly")
        );
        assert_eq!(
            invalid_updater_endpoint_url("bad url"),
            format!("Invalid updater endpoint URL: {}", "bad url")
        );
        assert_eq!(
            failed_to_set_updater_endpoint("bad url"),
            format!("Failed to set updater endpoint: {}", "bad url")
        );
        assert_eq!(
            failed_to_build_updater("bad config"),
            format!("Failed to build updater: {}", "bad config")
        );
        assert_eq!(
            NO_UPDATE_AVAILABLE_TO_INSTALL,
            "No update available to install — check for updates first"
        );
    }

    #[test]
    fn settings_manager_messages_match_their_original_inline_text() {
        assert_eq!(
            failed_to_serialize_settings("bad value"),
            format!("Failed to serialize settings: {}", "bad value")
        );
        assert_eq!(
            failed_to_write_settings_file(std::path::Path::new("C:/settings.json"), "disk full"),
            "Failed to write settings file C:/settings.json: disk full"
        );
    }

    // Every non-panicking call site's spawn_blocking wrapping was already
    // exercised implicitly by the crate's existing capture_manager/map_manager
    // tests once this PR rewired them through these two helpers — what's
    // unique to test here is the join-error text itself, which only surfaces
    // if the spawned closure panics.
    #[tokio::test]
    async fn flatten_spawn_blocking_reports_a_panic_as_a_crashed_task() {
        let handle: tokio::task::JoinHandle<Result<(), String>> =
            tokio::task::spawn_blocking(|| panic!("boom"));
        let err = flatten_spawn_blocking(handle).await.unwrap_err();
        assert!(
            err.starts_with("Background task crashed: "),
            "expected the \"Background task crashed: {{}}\" prefix, got {err:?}"
        );
    }

    #[tokio::test]
    async fn spawn_blocking_result_reports_a_panic_as_a_crashed_task() {
        let handle: tokio::task::JoinHandle<()> = tokio::task::spawn_blocking(|| panic!("boom"));
        let err = spawn_blocking_result(handle).await.unwrap_err();
        assert!(
            err.starts_with("Background task crashed: "),
            "expected the \"Background task crashed: {{}}\" prefix, got {err:?}"
        );
    }
}
