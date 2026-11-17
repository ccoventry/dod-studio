use super::*;

#[test]
fn dropped_when_patched_lists_each_command_or_says_nothing() {
    assert_eq!(dropped_when_patched(&[]), None);
    let dropped = native::game_config::cfg_scan::demo_filtered_commands(&[
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

#[test]
fn lower_crate_errors_name_the_action_then_the_cause() {
    assert_eq!(
        failed_to_copy_demos_for_batch("no room on D:"),
        "Failed to copy the demos for the batch: no room on D:"
    );
    assert_eq!(
        failed_to_set_blender_exe("not found"),
        "Failed to use that Blender: not found"
    );
}

#[test]
fn a_cancellation_is_never_given_an_action_prefix() {
    // The frontend compares it to the bare word.
    let cancelled = native::hlcr::take_frames::cancelled_error();
    assert_eq!(failed_to(".", &cancelled), cancelled);
    assert_eq!(cancelled, "cancelled");
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

#[test]
fn small_manager_messages_match_their_original_inline_text() {
    assert_eq!(menu_size_check_stopped("x"), "menu size check stopped: x");
    assert_eq!(menu_size_change_stopped("x"), "menu size change stopped: x");
    assert_eq!(
        could_not_list_to_copy("D:/dod", "denied"),
        "Couldn't list D:/dod to copy it: denied"
    );
    assert_eq!(
        MOVIE_COPY_ALREADY_RUNNING,
        "A movie copy is already running"
    );
    assert_eq!(
        OVERVIEW_IMAGE_NOT_RAW_BYTES,
        "the high-quality image did not arrive as raw bytes"
    );
    assert_eq!(
        not_a_sprite_file_name("a.spr"),
        "\"a.spr\" is not a sprite file name"
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
