use super::*;
use crate::test_support::Scratch;
use native::patch::PatchStage;

/// The drop-rate event carries percentages to one decimal and the take
/// folder as the batch planned it (#172).
#[test]
fn obs_drop_rate_payload_rounds_and_names_the_take() {
    let report = native::obs::ObsDropReport {
        clip: 2,
        take_folder: Some(std::path::PathBuf::from(r"D:\raw\b1")),
        recording: false,
        clip_counts: native::obs::FrameCounters {
            output_total: 1200,
            output_skipped: 407,
            render_total: 1200,
            render_skipped: 3,
        },
        recent: native::obs::FrameCounters::default(),
        lagging: true,
        capture_fps: 120,
    };
    let p = obs_drop_rate_payload(&report);
    assert_eq!(p["skipped_pct"], 33.9);
    assert_eq!(p["missed_pct"], 0.3);
    assert_eq!(p["take_folder"], r"D:\raw\b1");
    assert_eq!(p["recording"], false);
    assert_eq!(p["lagging"], true);
    assert_eq!(p["recent_skipped_pct"], 0.0);
}
/// Render While Capturing (#861) deletes a take once its clip renders,
/// before the batch ends: the block still counts as captured, isn't one
/// Render Studio missed, and writing the capture settings keeps its
/// render history.
#[test]
fn a_take_rendered_and_deleted_while_capturing_verifies_as_captured() {
    use native::hlcr::take_meta::{self, RenderAttempt, RenderOutcome, SessionMeta};
    let scratch = Scratch::new("rendered_while_capturing");
    let block = scratch
        .path()
        .join("session_1")
        .join("dodstudio_chain_01_b0");
    std::fs::create_dir_all(&block).unwrap();
    let attempt = RenderAttempt {
        started_unix_ms: 1,
        stream: "all".into(),
        codec: "prores".into(),
        custom_codec_args: String::new(),
        fps: 120,
        outcome: RenderOutcome::Finished,
        output_path: "D:/clips/a.mov".into(),
        output_size_bytes: Some(10),
        error: None,
    };
    let meta = SessionMeta {
        renders: vec![attempt.clone()],
        ..SessionMeta::new(String::new(), 0)
    };
    take_meta::write(&block, &meta).unwrap();
    let empty = scratch
        .path()
        .join("session_1")
        .join("dodstudio_chain_02_b0");
    std::fs::create_dir_all(&empty).unwrap();

    let manifest = CaptureManifest {
        session_id: "session_1".into(),
        blocks: [&block, &empty]
            .iter()
            .map(|folder| native::patch::types::CaptureBlock {
                demo_name: "d".into(),
                block_index: 0,
                drive_index: 0,
                take_folder: folder.to_path_buf(),
                take_key: String::new(),
                source_streak_indices: vec![],
                start_tick: 0,
                end_tick: 1,
                record_start_tick: 0,
                record_stop_tick: 1,
                ramped_gaps: 0,
                plan: None,
            })
            .collect(),
        capture_fps: 120,
        capture_mode: String::new(),
        agr_fps: 0,
        streak_uids: vec![],
        streak_sources: vec![],
        project_path: None,
        settings: None,
    };
    let verified = verify_capture_takes(&manifest, &mut |_| {});
    assert!(verified[0].captured && verified[0].rendered);
    assert!(!verified[0].renderable);
    assert!(
        !verified[1].captured && !verified[1].rendered,
        "nothing there"
    );

    record_capture_settings(&manifest, &verified);
    let kept = take_meta::read_for_take(&block).unwrap();
    assert_eq!(kept.capture_fps, 120);
    assert_eq!(kept.renders, vec![attempt]);
}

/// The plan a block was built with is written beside its take (#1084),
/// where the take checks read it back.
#[test]
fn a_blocks_plan_is_recorded_beside_its_take() {
    use native::hlcr::take_meta;
    use native::hlcr::take_plan::TakePlan;
    let scratch = Scratch::new("plan_recorded");
    let planned = scratch
        .path()
        .join("session_1")
        .join("dodstudio_chain_01_b0");
    let unplanned = scratch
        .path()
        .join("session_1")
        .join("dodstudio_chain_01_b1");
    for folder in [&planned, &unplanned] {
        std::fs::create_dir_all(folder.join("take0000")).unwrap();
        std::fs::write(folder.join("take0000").join("sound.wav"), b"x").unwrap();
    }
    let plan = TakePlan {
        length_secs: 12.5,
        kill_offsets_secs: vec![4.0, 9.0],
        sped_up: Vec::new(),
    };
    let manifest = CaptureManifest {
        session_id: "session_1".into(),
        blocks: [(&planned, Some(plan.clone())), (&unplanned, None)]
            .into_iter()
            .enumerate()
            .map(
                |(block_index, (folder, plan))| native::patch::types::CaptureBlock {
                    demo_name: "d".into(),
                    block_index,
                    drive_index: 0,
                    take_folder: folder.to_path_buf(),
                    take_key: String::new(),
                    source_streak_indices: vec![],
                    start_tick: 0,
                    end_tick: 1,
                    record_start_tick: 0,
                    record_stop_tick: 1,
                    ramped_gaps: 0,
                    plan,
                },
            )
            .collect(),
        capture_fps: 300,
        capture_mode: String::new(),
        agr_fps: 0,
        streak_uids: vec![],
        streak_sources: vec![],
        project_path: None,
        settings: None,
    };
    let verified = verify_capture_takes(&manifest, &mut |_| {});
    record_capture_settings(&manifest, &verified);
    assert_eq!(take_meta::read_for_take(&planned).unwrap().plan, Some(plan));
    assert_eq!(take_meta::read_for_take(&unplanned).unwrap().plan, None);
}

fn names(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("demo{i}.dem")).collect()
}

#[test]
fn the_getting_ready_line_names_each_demo_in_plain_words() {
    let progress = PatchProgress::new(names(3));
    let s = progress.summary(0);
    assert_eq!(s.status, "Getting demos ready: 0 of 3");
    assert!(s.measured);

    // The decal pass reports no percentage: the demo is named, nothing
    // says "decals", and the bar slides.
    progress.set(0, PatchStage::ClearingDecals);
    let s = progress.summary(0);
    assert_eq!(s.status, "Getting demos ready: 1 of 3 (demo1.dem)");
    assert!(!s.measured);

    progress.set(0, PatchStage::Writing { percent: 40 });
    let s = progress.summary(0);
    assert_eq!(s.status, "Getting demos ready: 1 of 3 (demo1.dem, 40%)");
    assert!((s.index - 0.4).abs() < 1e-9);
    assert!(s.measured);

    progress.set(1, PatchStage::ClearingDecals);
    progress.set(2, PatchStage::Writing { percent: 60 });
    assert_eq!(
        progress.summary(0).status,
        "Getting demos ready: 0 of 3 done, 3 at once (50% on average)"
    );

    progress.finish(0);
    progress.finish(2);
    assert_eq!(
        progress.summary(2).status,
        "Getting demos ready: 3 of 3 (demo2.dem)"
    );
    progress.finish(1);
    assert_eq!(progress.summary(3).status, "Getting demos ready: 3 of 3");
}

#[test]
fn the_capture_steps_read_in_plain_words() {
    use crate::messages::{capturing_clip, checking_takes, loading_demo, starting_the_game};
    assert_eq!(starting_the_game(None, 3), "Starting the game…");
    assert_eq!(
        starting_the_game(Some("anzio.dem"), 3),
        "Starting the game for anzio.dem (demo 1 of 3)"
    );
    assert_eq!(
        starting_the_game(Some("anzio.dem"), 1),
        "Starting the game for anzio.dem"
    );
    assert_eq!(loading_demo("anzio.dem"), "Loading anzio.dem…");
    assert_eq!(
        capturing_clip(3, 12, "anzio.dem"),
        "Capturing clip 3 of 12 (anzio.dem)"
    );
    assert_eq!(capturing_clip(3, 12, ""), "Capturing clip 3 of 12");
    assert_eq!(checking_takes(12, 12), "Checking the takes: 12 of 12");
}

#[test]
fn percentages_are_throttled_but_stage_changes_are_not() {
    let progress = PatchProgress::new(names(1));
    assert!(progress.set(0, PatchStage::ClearingDecals));
    assert!(progress.set(0, PatchStage::Writing { percent: 0 }));
    // Within 33ms of the last send.
    assert!(!progress.set(0, PatchStage::Writing { percent: 1 }));
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(progress.set(0, PatchStage::Writing { percent: 2 }));
}

#[test]
fn a_take_folders_size_counts_its_subfolders() {
    let dir = Scratch::new("take_folder_bytes");
    std::fs::create_dir_all(dir.path().join("hud")).unwrap();
    std::fs::write(dir.path().join("sound.wav"), vec![0u8; 100]).unwrap();
    std::fs::write(dir.path().join("hud").join("00000.tga"), vec![0u8; 50]).unwrap();
    assert_eq!(folder_bytes(dir.path()), 150);
    assert_eq!(folder_bytes(&dir.path().join("missing")), 0);
}

fn sample_payload() -> CapturePayload {
    CapturePayload {
        hlae_path: "C:/hlae/hlae.exe".to_string(),
        game_path: "C:/dod/hl.exe".to_string(),
        ffmpeg_override_path: None,
        goldsrc_hooks_dll_path: None,
        resolution_width: 1920,
        resolution_height: 1080,
        ffmpeg_capture: false,
        ffmpeg_capture_codec: String::new(),
        capture_mode: String::new(),
        obs_host: String::new(),
        obs_port: 0,
        obs_password: String::new(),
        obs_recording_quality: String::new(),
        save_local_patched_copy: false,
        hook_runs_batch: false,
        reuse_game_pid: None,
        keep_game_behind: true,
        streaks: Vec::new(),
        streak_uids: Vec::new(),
        project_path: None,
        pre_roll_seconds: 2.0,
        post_roll_seconds: 0.6,
        capture_directories: vec!["D:/capture".to_string()],
        capture_fps: 300,
        obs_capture_fps: 120,
        agr_fps: 0,
        drives: vec!["D:/capture".to_string(), "E:/capture".to_string()],
        record_start_lead: 0.0,
        record_stop_trail: 0.0,
        initial_delay: 3.0,
        fast_forward_speed: 0.05,
        auto_clear_logs: false,
        auto_clear_previews: false,
        auto_clear_temp_demos: false,
        session_id: "session_test".to_string(),
        init_commands: vec!["exec autoexec".to_string()],
        picture_fov: None,
        custom_commands: vec![
            CustomCommandPayload {
                command: "say after".to_string(),
                relation: "After".to_string(),
                offset_seconds: 1.0,
            },
            CustomCommandPayload {
                command: "say unrecognized".to_string(),
                relation: "Sideways".to_string(),
                offset_seconds: 1.0,
            },
        ],
        decal_flush: None,
        decal_ring_limit: None,
        kill_gaps: Default::default(),
    }
}

/// `fast_forward_speed`'s serde default silently drifted to 10.0 here while
/// every other default (settings_manager, patch::types, the frontend's own
/// JS fallback) agrees on 0.05 — a 200x-too-fast fallback that only bites
/// a payload missing the field entirely (an old cached payload, a CLI/test
/// caller), but should still agree with everywhere else. See issue #29.
#[test]
fn test_capture_payload_fast_forward_speed_default_matches_settings_default() {
    let mut value = serde_json::to_value(sample_payload()).unwrap();
    value.as_object_mut().unwrap().remove("fast_forward_speed");
    let payload: CapturePayload = serde_json::from_value(value).unwrap();
    assert_eq!(payload.fast_forward_speed, 0.05);
}

#[test]
fn test_config_from_payload_maps_scalar_fields() {
    let payload = sample_payload();
    let cfg = config_from_payload(&payload);

    assert_eq!(cfg.hlae_path, payload.hlae_path);
    assert_eq!(cfg.game_path, payload.game_path);
    assert_eq!(cfg.resolution_width, 1920);
    assert_eq!(cfg.resolution_height, 1080);
    assert_eq!(cfg.capture_fps, 300);
    assert_eq!(cfg.session_id, "session_test");
    assert_eq!(cfg.init_commands, vec!["exec autoexec".to_string()]);
    assert_eq!(cfg.capture_directories, vec![PathBuf::from("D:/capture")]);
}

#[test]
fn test_config_from_payload_carries_agr_mode_and_its_fps() {
    let mut payload = sample_payload();
    payload.capture_mode = "agr".to_string();
    payload.agr_fps = 60;
    let cfg = config_from_payload(&payload);
    assert_eq!(cfg.capture_mode, native::patch::CaptureMode::Agr);
    assert_eq!(cfg.effective_agr_fps(), 60);
    assert!(!cfg.ffmpeg_capture);

    // Absent from an older frontend, or 0 (an empty box): AGR's own 30,
    // never Capture FPS.
    let mut value = serde_json::to_value(sample_payload()).unwrap();
    value.as_object_mut().unwrap().remove("agr_fps");
    let payload: CapturePayload = serde_json::from_value(value).unwrap();
    assert_eq!(payload.capture_fps, 300);
    assert_eq!(config_from_payload(&payload).effective_agr_fps(), 30);
    let mut payload = sample_payload();
    payload.agr_fps = 0;
    assert_eq!(config_from_payload(&payload).effective_agr_fps(), 30);
}

#[test]
fn the_frontend_mirrors_the_default_agr_fps() {
    let js = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/agr_rate.js"),
    )
    .unwrap();
    let line = format!(
        "export const DEFAULT_AGR_FPS = {};",
        native::patch::DEFAULT_AGR_FPS
    );
    assert!(
        js.contains(&line),
        "studio/src/agr_rate.js must say `{line}`"
    );
}

#[test]
fn test_config_from_payload_decal_flush_overrides_only_when_sent() {
    // Absent means "leave the pipeline default alone", so a frontend that
    // knows nothing about decals still gets the flush.
    let defaults = PatcherConfig::default();
    let mut payload = sample_payload();

    let cfg = config_from_payload(&payload);
    assert_eq!(cfg.decal_flush, defaults.decal_flush);
    assert_eq!(cfg.decal_ring_limit, defaults.decal_ring_limit);

    payload.decal_flush = Some(false);
    payload.decal_ring_limit = Some(64);
    let cfg = config_from_payload(&payload);
    assert!(!cfg.decal_flush);
    assert_eq!(cfg.decal_ring_limit, 64);
}

#[test]
fn test_config_from_payload_primary_media_dir_is_first_drive() {
    let payload = sample_payload();
    let cfg = config_from_payload(&payload);
    // Capture Output's first entry is the sole source of primary_media_dir —
    // there's no separate "Primary Media Dir" field.
    assert_eq!(cfg.primary_media_dir, Some(PathBuf::from("D:/capture")));
}

#[test]
fn test_config_from_payload_no_drives_leaves_primary_media_dir_none() {
    let mut payload = sample_payload();
    payload.drives = Vec::new();
    let cfg = config_from_payload(&payload);
    assert_eq!(cfg.primary_media_dir, None);
}

#[test]
fn test_config_from_payload_custom_command_relation_falls_back_to_before() {
    let payload = sample_payload();
    let cfg = config_from_payload(&payload);

    assert_eq!(cfg.custom_commands.len(), 2);
    assert_eq!(cfg.custom_commands[0].command, "say after");
    assert_eq!(cfg.custom_commands[0].relation, CommandRelation::After);
    // An unrecognised relation string must fail safe to Before rather than
    // rejecting the whole batch payload (see CustomCommandPayload's doc comment).
    assert_eq!(cfg.custom_commands[1].relation, CommandRelation::Before);
}

/// A `.cfg` inside its own scratch directory. The guard comes back with
/// the path and has to be held: dropping it removes the file.
fn write_temp_cfg(tag: &str, content: &str) -> (Scratch, String) {
    let dir = Scratch::new(format_args!("cfgimport_{tag}"));
    let path = dir.join("import.cfg");
    std::fs::write(&path, content).unwrap();
    let text = path.to_string_lossy().to_string();
    (dir, text)
}

fn read_cfg_commands_blocking(path: String) -> Result<Vec<String>, String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(read_cfg_commands(path))
}

#[test]
fn test_read_cfg_commands_strips_blank_lines_and_full_line_comments() {
    let (_dir, path) = write_temp_cfg(
        "basic",
        "// header comment\nmirv_fov 90\n\n  mirv_movie_fps 300  \n// trailing comment\nsensitivity 3\n",
    );
    let commands = read_cfg_commands_blocking(path.clone()).unwrap();
    assert_eq!(
        commands,
        vec!["mirv_fov 90", "mirv_movie_fps 300", "sensitivity 3"]
    );
}

#[test]
fn test_read_cfg_commands_on_an_all_comment_file_is_empty() {
    let (_dir, path) = write_temp_cfg("empty", "// just a header\n// nothing else\n");
    let commands = read_cfg_commands_blocking(path.clone()).unwrap();
    assert!(commands.is_empty());
}

#[test]
fn test_read_cfg_commands_missing_file_errs() {
    let dir = Scratch::absent("cfgimport_missing");
    let path = dir.join("nothing.cfg").to_string_lossy().to_string();
    assert!(read_cfg_commands_blocking(path).is_err());
}

#[test]
fn test_read_cfg_commands_collapses_column_alignment_padding() {
    // A hand-aligned .cfg — cvar and value padded into columns with extra
    // spaces so they line up in a text editor.
    let (_dir, path) = write_temp_cfg(
        "aligned",
        "r_decals               \"0\"\ncl_hud_objectives\t\t\"1\"\n",
    );
    let commands = read_cfg_commands_blocking(path.clone()).unwrap();
    assert_eq!(commands, vec!["r_decals \"0\"", "cl_hud_objectives \"1\""]);
}

#[test]
fn test_read_cfg_commands_preserves_whitespace_inside_quotes() {
    // A quoted value's own spaces (a path, say) are content, not
    // alignment padding, and must survive verbatim.
    let (_dir, path) = write_temp_cfg(
        "quoted_spaces",
        "mirv_movie_filename    \"F:\\DICE  WSOD25\\02 Audio Video\\clip\"\n",
    );
    let commands = read_cfg_commands_blocking(path.clone()).unwrap();
    assert_eq!(
        commands,
        vec!["mirv_movie_filename \"F:\\DICE  WSOD25\\02 Audio Video\\clip\""]
    );
}

#[test]
fn a_command_too_long_for_a_demo_frame_is_refused_in_either_list() {
    let long = format!("echo {}", "x".repeat(59)); // 64 bytes
    let err = first_banned_command_error(std::slice::from_ref(&long), &[]);
    assert!(err.is_some_and(|e| e.contains("64 bytes")));

    let scheduled = vec![CustomCommandPayload {
        command: long,
        relation: "Before".to_string(),
        offset_seconds: 1.0,
    }];
    assert!(first_banned_command_error(&[], &scheduled).is_some());

    let fits = format!("echo {}", "x".repeat(58)); // 63 bytes
    assert!(first_banned_command_error(&[fits], &[]).is_none());
}

#[test]
fn a_banned_command_in_init_commands_is_refused() {
    let err = first_banned_command_error(&["mirv_recordmovie_start".to_string()], &[]);
    assert!(err.is_some(), "must be refused");
    assert!(err.unwrap().contains("mirv_recordmovie_start"));
}

#[test]
fn mirv_movie_filename_is_no_longer_refused_in_init_commands() {
    // Inert in Initial Commands (see
    // cfg_scan::NOOP_IN_INIT_COMMANDS), only dangerous once scheduled.
    let err = first_banned_command_error(&["mirv_movie_filename foo".to_string()], &[]);
    assert!(err.is_none(), "{:?}", err);
}

#[test]
fn mirv_movie_filename_is_still_refused_when_scheduled() {
    let scheduled = vec![CustomCommandPayload {
        command: "mirv_movie_filename foo".to_string(),
        relation: "Before".to_string(),
        offset_seconds: 1.0,
    }];
    let err = first_banned_command_error(&[], &scheduled);
    assert!(err.is_some(), "must be refused");
    assert!(err.unwrap().contains("mirv_movie_filename"));
}

#[test]
fn a_banned_command_in_scheduled_commands_is_refused() {
    let scheduled = vec![CustomCommandPayload {
        command: "host_framerate 0.05".to_string(),
        relation: "Before".to_string(),
        offset_seconds: 1.0,
    }];
    let err = first_banned_command_error(&[], &scheduled);
    assert!(err.is_some(), "must be refused");
    assert!(err.unwrap().contains("host_framerate"));
}

#[test]
fn tier_2_and_tier_3_commands_are_not_refused() {
    let err = first_banned_command_error(
        &[
            "mirv_movie_fps 500".to_string(),
            "r_decals \"256\"".to_string(),
            "mirv_fov 90".to_string(),
        ],
        &[],
    );
    assert!(err.is_none(), "{:?}", err);
}

#[test]
fn a_scheduled_only_banned_command_is_fine_in_init_commands() {
    // r_decals/mirv_fov/gl_widescreenfov are exactly how the decal flush
    // is meant to be configured when set once at demo load.
    let err = first_banned_command_error(
        &[
            "r_decals 512".to_string(),
            "mirv_fov 105".to_string(),
            "gl_widescreenfov 1".to_string(),
        ],
        &[],
    );
    assert!(err.is_none(), "{:?}", err);
}

#[test]
fn a_scheduled_only_banned_command_is_refused_when_scheduled() {
    let scheduled = vec![CustomCommandPayload {
        command: "mirv_fov 105".to_string(),
        relation: "Before".to_string(),
        offset_seconds: 1.0,
    }];
    let err = first_banned_command_error(&[], &scheduled);
    assert!(err.is_some(), "must be refused");
    assert!(err.unwrap().contains("mirv_fov"));
}

#[test]
fn clean_commands_are_not_refused() {
    let err = first_banned_command_error(&["sensitivity 3".to_string()], &[]);
    assert!(err.is_none());
}

#[test]
fn the_clip_tally_counts_recorded_clips_across_demos() {
    let mut clips = ClipTally::default();
    clips.demo_loading(3);
    clips.fast_forward_to(1, 3);
    assert_eq!((clips.done, clips.through_current_demo), (0, 3));
    clips.fast_forward_to(3, 3);
    assert_eq!(clips.done, 2);

    // The next demo: all three of the first are recorded.
    clips.demo_loading(2);
    assert_eq!((clips.done, clips.through_current_demo), (3, 5));
    clips.fast_forward_to(2, 2);
    assert_eq!(clips.done, 4);

    clips.session_finished();
    assert_eq!(clips.done, 5);
}

fn cut_short_manifest(order: &[(&str, usize)]) -> CaptureManifest {
    CaptureManifest {
        session_id: "s".into(),
        blocks: order
            .iter()
            .map(|&(demo, block_index)| native::patch::types::CaptureBlock {
                demo_name: demo.into(),
                block_index,
                drive_index: 0,
                take_folder: std::path::PathBuf::from(format!("{demo}_b{block_index}")),
                take_key: format!("s/{demo}_b{block_index}"),
                source_streak_indices: vec![],
                start_tick: 0,
                end_tick: 0,
                // Every demo's clips start at the same ticks, so a tick
                // alone can't tell two demos' blocks apart.
                record_start_tick: 100 * (block_index as i32 + 1),
                record_stop_tick: 0,
                ramped_gaps: 0,
                plan: None,
            })
            .collect(),
        capture_fps: 300,
        capture_mode: "frames".into(),
        agr_fps: 0,
        streak_uids: vec![],
        streak_sources: vec![],
        project_path: None,
        settings: None,
    }
}

fn on_disk(manifest: &CaptureManifest) -> Vec<VerifiedBlock> {
    manifest
        .blocks
        .iter()
        .map(|b| VerifiedBlock {
            take_key: b.take_key.clone(),
            take_folder: b.take_folder.to_string_lossy().into_owned(),
            demo_name: b.demo_name.clone(),
            block_index: b.block_index,
            source_streak_indices: vec![],
            captured: true,
            renderable: true,
            rendered: false,
            bytes: 1,
            cut_short: false,
            cut_by: None,
            short_take: None,
            started_late_ms: None,
            clip_verdict: None,
            ramped_gaps: 0,
            not_reached: false,
        })
        .collect()
}

/// Marker counts with no ticks (a marker that names none).
fn counts(started: usize, stopped: usize) -> Records {
    Records {
        started,
        stopped,
        ..Records::default()
    }
}

#[test]
fn blocks_are_put_in_recording_order_demo_by_demo() {
    // Blocks out of `block_index` order still come out in it.
    let manifest = cut_short_manifest(&[("a", 2), ("a", 0), ("a", 1), ("b", 1), ("b", 0)]);
    assert_eq!(recording_order(&manifest.blocks), vec![1, 2, 0, 4, 3]);
}

/// #1081: Cancel while the third clip recorded leaves a partial take,
/// which is not a captured clip.
#[test]
fn the_clip_a_cancel_cut_short_is_not_captured() {
    let manifest = cut_short_manifest(&[("a", 2), ("a", 0), ("a", 1), ("b", 1), ("b", 0)]);
    let mut blocks = on_disk(&manifest);
    mark_cut_short(&manifest, &mut blocks, counts(3, 2), CutBy::Cancel);
    let cut: Vec<_> = blocks.iter().map(|b| (b.captured, b.cut_short)).collect();
    // Recording order a0, a1, a2: a2 (manifest position 0) was recording.
    assert_eq!(
        cut,
        vec![
            (false, true),
            (true, false),
            (true, false),
            (true, false),
            (true, false)
        ]
    );
    assert!(!blocks[0].renderable);

    // Cancelled between clips, or with no markers read: nothing changes.
    for (started, stopped) in [(2, 2), (0, 0)] {
        let mut blocks = on_disk(&manifest);
        mark_cut_short(
            &manifest,
            &mut blocks,
            counts(started, stopped),
            CutBy::Cancel,
        );
        assert!(blocks.iter().all(|b| b.captured && !b.cut_short));
    }
    // A clip cut short before it wrote anything is just missing.
    let mut blocks = on_disk(&manifest);
    blocks[3].captured = false;
    mark_cut_short(&manifest, &mut blocks, counts(5, 4), CutBy::Cancel);
    assert!(!blocks[3].captured && !blocks[3].cut_short);
    assert_eq!(blocks[3].cut_by, None);
}

/// The game closing mid-clip cuts it short like a Cancel, and the row
/// says the game closed: the same clip, a different reason.
#[test]
fn a_clip_the_game_closing_cut_short_is_not_captured_and_says_why() {
    let manifest = cut_short_manifest(&[("a", 0), ("a", 1), ("b", 0)]);
    let mut blocks = on_disk(&manifest);
    mark_cut_short(&manifest, &mut blocks, counts(2, 1), CutBy::GameClosed);
    assert!(!blocks[1].captured && !blocks[1].renderable && blocks[1].cut_short);
    assert_eq!(blocks[1].cut_by, Some(CutBy::GameClosed));
    assert!(
        blocks
            .iter()
            .enumerate()
            .all(|(i, b)| i == 1 || (b.captured && b.cut_by.is_none()))
    );
    let row = serde_json::to_value(&blocks[1]).unwrap();
    assert_eq!(row["cut_by"], "game_closed");
}

/// A batch that failed in the game checks its takes, under an outcome
/// the next start's catch-up (#545) leaves alone; one that failed before
/// a demo played has nothing to check.
#[test]
fn a_batch_that_failed_in_the_game_is_ended_like_a_cancel() {
    let mut records = Records::default();
    assert!(!records.reached_game(), "nothing played yet");
    records.demo_started(1);
    assert!(records.reached_game());

    let closed = BatchEnd::failed("hl.exe ended", records, true);
    assert_eq!(
        (closed.outcome, closed.error, closed.cut_by),
        ("error", true, CutBy::GameClosed)
    );
    assert_ne!(closed.outcome, "planned");
    let stalled = BatchEnd::failed("stalled", records, false);
    assert_eq!(stalled.cut_by, CutBy::Stopped);
    let cancelled = BatchEnd::cancelled(records);
    assert_eq!((cancelled.error, cancelled.cut_by), (false, CutBy::Cancel));
}

/// #1434: only a batch that stopped early says what ended it.
#[test]
fn a_complete_batch_names_nothing_as_what_ended_it() {
    assert_eq!(BatchEnd::complete().ended_by(), None);
    assert_eq!(
        BatchEnd::cancelled(Records::default()).ended_by(),
        Some(CutBy::Cancel)
    );
    assert_eq!(
        BatchEnd::failed("x", Records::default(), true).ended_by(),
        Some(CutBy::GameClosed)
    );
}

/// A demo the hook's batch skipped sends no markers (#861), so the count
/// falls behind: the tick the last `START_RECORD` named finds the clip.
#[test]
fn the_cut_short_clip_is_found_by_its_tick_after_a_skipped_demo() {
    let mut manifest = cut_short_manifest(&[("a", 1), ("a", 0), ("b", 1), ("b", 0)]);
    manifest.blocks[2].record_start_tick = 1200;
    manifest.blocks[3].record_start_tick = 1100;
    // Demo a skipped: b0 recorded whole, b1 (tick 1200) was recording.
    let records = Records {
        started: 2,
        stopped: 1,
        last_start_tick: Some(1200),
        ..Records::default()
    };
    let mut blocks = on_disk(&manifest);
    mark_cut_short(&manifest, &mut blocks, records, CutBy::Cancel);
    let cut: Vec<_> = blocks.iter().map(|b| b.cut_short).collect();
    assert_eq!(cut, vec![false, false, true, false]);
}

/// Two demos' clips can start at the same tick: the search starts at the
/// count, never before it.
#[test]
fn a_tick_an_earlier_demo_shares_does_not_pick_its_clip() {
    let manifest = cut_short_manifest(&[("a", 1), ("a", 0), ("b", 1), ("b", 0)]);
    // a0 and a1 recorded whole; b0 (tick 100, like a0) was recording.
    let records = Records {
        started: 3,
        stopped: 2,
        last_start_tick: Some(100),
        ..Records::default()
    };
    let mut blocks = on_disk(&manifest);
    mark_cut_short(&manifest, &mut blocks, records, CutBy::Cancel);
    let cut: Vec<_> = blocks.iter().map(|b| b.cut_short).collect();
    assert_eq!(cut, vec![false, false, false, true]);
}

/// A take at `folder` as HLAE writes a frame sequence: `frames` BMPs in
/// `take0000/all`, and `audio_secs` of 8-bit mono 1 kHz audio beside them.
fn bmp_take(folder: &Path, frames: usize, audio_secs: u32) {
    let take = folder.join("take0000");
    let stream = take.join("all");
    std::fs::create_dir_all(&stream).unwrap();
    for i in 0..frames {
        std::fs::write(stream.join(format!("{i:05}.bmp")), b"bmp").unwrap();
    }
    let data = audio_secs * 1000;
    let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    for field in [16u32, 1 | (1 << 16), 1000, 1000, 1 | (8 << 16)] {
        wav.extend_from_slice(&field.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data.to_le_bytes());
    wav.resize(wav.len() + data as usize, 0);
    std::fs::write(take.join("sound.wav"), wav).unwrap();
}

/// #1105: a take whose video FFmpeg stopped writing mid-clip, while the
/// game wrote the audio to the end. It is a partial take, like a
/// Cancel's: not captured, not renderable, so nothing renders or deletes
/// it.
#[test]
fn a_take_that_stopped_writing_early_is_not_captured() {
    let scratch = Scratch::new("short_take_1105");
    let whole = scratch.path().join("chain_01_b0");
    let short = scratch.path().join("chain_01_b1");
    let obs = scratch.path().join("chain_01_b2");
    // 10 fps: the whole take has 3.0 s of video to 3 s of audio, the
    // short one 0.5 s to 3 s.
    bmp_take(&whole, 30, 3);
    bmp_take(&short, 5, 3);
    let obs_stream = obs.join("take0000").join("all");
    std::fs::create_dir_all(&obs_stream).unwrap();
    std::fs::write(obs_stream.join("video.mp4"), b"mp4").unwrap();
    let mut manifest = cut_short_manifest(&[("chain_01", 0), ("chain_01", 1), ("chain_01", 2)]);
    manifest.capture_fps = 10;
    for (block, folder) in manifest.blocks.iter_mut().zip([&whole, &short, &obs]) {
        block.take_folder = folder.to_path_buf();
    }

    let verified = verify_capture_takes_settling(&manifest, None, &mut |_| {});
    assert!(verified[0].captured && verified[0].renderable && !verified[0].cut_short);
    assert!(!verified[1].captured && !verified[1].renderable && verified[1].cut_short);
    assert_eq!(
        verified[1].short_take,
        Some(native::hlcr::take_length::ShortTake {
            video_secs: 0.5,
            audio_secs: 3.0
        })
    );
    // An OBS take's length isn't read: it isn't judged.
    assert!(verified[2].captured && !verified[2].cut_short);
}

/// A clip Cancel cut short is short too; the reason it gives is Cancel.
#[test]
fn a_short_take_cancel_cut_gives_cancel_as_the_reason() {
    let manifest = cut_short_manifest(&[("a", 0), ("a", 1)]);
    let mut blocks = on_disk(&manifest);
    blocks[1].captured = false;
    blocks[1].renderable = false;
    blocks[1].cut_short = true;
    blocks[1].short_take = Some(native::hlcr::take_length::ShortTake {
        video_secs: 1.0,
        audio_secs: 9.0,
    });
    mark_cut_short(&manifest, &mut blocks, counts(2, 1), CutBy::Cancel);
    assert!(blocks[1].cut_short && !blocks[1].captured);
    assert_eq!(blocks[1].short_take, None);
    // A short take Cancel didn't touch keeps its own reason.
    let mut blocks = on_disk(&manifest);
    blocks[0].captured = false;
    blocks[0].cut_short = true;
    blocks[0].short_take = Some(native::hlcr::take_length::ShortTake {
        video_secs: 1.0,
        audio_secs: 9.0,
    });
    mark_cut_short(&manifest, &mut blocks, counts(2, 1), CutBy::Cancel);
    assert!(blocks[0].cut_short && blocks[0].short_take.is_some());
    assert!(blocks[1].cut_short && blocks[1].short_take.is_none());
}

/// A take still being finished when the batch ends looks short at first:
/// with a settle it is looked at again, and a whole one is captured.
#[test]
fn a_take_finished_during_the_settle_is_not_short() {
    let scratch = Scratch::new("short_take_settle_1105");
    let folder = scratch.path().join("chain_01_b0");
    bmp_take(&folder, 5, 3);
    let mut manifest = cut_short_manifest(&[("chain_01", 0)]);
    manifest.capture_fps = 10;
    manifest.blocks[0].take_folder = folder.clone();
    let stream = folder.join("take0000").join("all");
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        for i in 5..30 {
            std::fs::write(stream.join(format!("{i:05}.bmp")), b"bmp").unwrap();
        }
    });
    let verified = verify_capture_takes_settling(
        &manifest,
        Some(std::time::Duration::from_millis(500)),
        &mut |_| {},
    );
    writer.join().unwrap();
    assert!(verified[0].captured && verified[0].short_take.is_none());
}

/// A demo that ended or stopped playing mid-clip leaves a `START_RECORD`
/// with no `STOP_RECORD`, and the hook's batch goes on to the next demo:
/// a Cancel there between clips must not mark a whole clip cut short.
#[test]
fn a_clip_an_earlier_demo_left_recording_is_not_cut_short_by_a_later_cancel() {
    let manifest = cut_short_manifest(&[("a", 0), ("a", 1), ("b", 0), ("b", 1)]);
    let mut records = Records::default();
    records.demo_started(1);
    records.clip_started(Some(100));
    records.clip_stopped();
    // a1 starts, then demo a stops playing.
    records.clip_started(Some(200));
    records.demo_started(2);
    records.clip_started(Some(100));
    records.clip_stopped();
    // Cancelled between b0 and b1.
    let mut blocks = on_disk(&manifest);
    mark_cut_short(&manifest, &mut blocks, records, CutBy::Cancel);
    assert!(blocks.iter().all(|b| b.captured && !b.cut_short));

    // Cancelled while b1 recorded: b1, not anything in demo a.
    records.clip_started(Some(200));
    let mut blocks = on_disk(&manifest);
    mark_cut_short(&manifest, &mut blocks, records, CutBy::Cancel);
    let cut: Vec<_> = blocks.iter().map(|b| b.cut_short).collect();
    assert_eq!(cut, vec![false, false, false, true]);
}

/// A skipped demo sends no markers, and its clip may start at the tick
/// the recording one does: the demo `DEMO_START` named decides.
#[test]
fn a_skipped_demo_sharing_the_tick_does_not_take_the_cut() {
    let manifest = cut_short_manifest(&[("a", 0), ("a", 1), ("b", 0), ("b", 1)]);
    let mut records = Records::default();
    // Demo a skipped before it played: b0 recording when cancelled.
    records.demo_started(2);
    records.clip_started(Some(100));
    let mut blocks = on_disk(&manifest);
    mark_cut_short(&manifest, &mut blocks, records, CutBy::Cancel);
    let cut: Vec<_> = blocks.iter().map(|b| b.cut_short).collect();
    assert_eq!(cut, vec![false, false, true, false]);
}
