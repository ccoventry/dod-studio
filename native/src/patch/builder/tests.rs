use super::*;
use crate::test_support::Scratch;

#[test]
fn playdemo_safe_stem_leaves_short_alphanumeric_names_alone() {
    assert_eq!(playdemo_safe_stem("demo1"), "demo1");
}

#[test]
fn playdemo_safe_stem_replaces_hyphens_that_truncate_the_goldsrc_cmdline() {
    // Confirmed live: GoldSrc's startup command-line tokenizer treats an
    // embedded "-" as the start of a new launch parm, so this exact stem
    // loaded as bare "wsod25" via +viewdemo instead of the real demo.
    let stem = playdemo_safe_stem("wsod25-po_r3_sf-warchyld_ih_m2_thunder_h1");
    assert!(
        !stem.contains('-'),
        "sanitized stem must not contain a hyphen: {stem}"
    );
    assert!(stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
}

#[test]
fn playdemo_safe_stem_stays_under_the_documented_length_limit() {
    // docs/goldsrc_dod_quirks.md: playdemo/viewdemo targets must stay
    // under ~40 characters. This name alone is 41 before "_preview".
    let stem = playdemo_safe_stem("wsod25-po_r3_sf-warchyld_ih_m2_thunder_h1");
    assert!(
        stem.len() + "_preview".len() < 40,
        "stem + _preview suffix must stay under the limit: {stem}"
    );
}

#[test]
fn playdemo_safe_stem_disambiguates_names_sharing_a_long_prefix() {
    let a = playdemo_safe_stem("a_very_long_shared_prefix_that_overflows_team1");
    let b = playdemo_safe_stem("a_very_long_shared_prefix_that_overflows_team2");
    assert_ne!(
        a, b,
        "two different overflowing names must not collide: {a} vs {b}"
    );
}

#[test]
fn test_build_batch_queue_merging() {
    let mut config = PatcherConfig::default(); // pre = 200, post = 60
    let temp_game_path = std::env::temp_dir().join("dod_test_mock");
    std::fs::create_dir_all(temp_game_path.join("dod")).expect("Failed to create dummy dod dir");
    config.game_path = temp_game_path.to_string_lossy().to_string();
    config.primary_media_dir = Some(temp_game_path.clone());
    let raw_streaks = vec![
        CaptureStreak {
            start_tick: 1000,
            end_tick: 1200,
            source_demo: "demo1.dem".to_string(),
            target_player: None,
            kill_count: 3,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: Vec::new(),
            start_index: 0,
            end_index: 2,
            total_demo_frames: 3000,
            demo_fps: 100.0,
            viewdemo_times: Vec::new(),
            frame_times: std::sync::Arc::new(Vec::new()),
            match_start_tick: None,
            source_key: None,
            status: Default::default(),
        },
        CaptureStreak {
            start_tick: 1300,
            end_tick: 1500,
            source_demo: "demo1.dem".to_string(),
            target_player: None,
            kill_count: 3,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: Vec::new(),
            start_index: 0,
            end_index: 2,
            total_demo_frames: 3000,
            demo_fps: 100.0,
            viewdemo_times: Vec::new(),
            frame_times: std::sync::Arc::new(Vec::new()),
            match_start_tick: None,
            source_key: None,
            status: Default::default(),
        },
        CaptureStreak {
            start_tick: 2000,
            end_tick: 2200,
            source_demo: "demo1.dem".to_string(),
            target_player: None,
            kill_count: 3,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: Vec::new(),
            start_index: 0,
            end_index: 2,
            total_demo_frames: 3000,
            demo_fps: 100.0,
            viewdemo_times: Vec::new(),
            frame_times: std::sync::Arc::new(Vec::new()),
            match_start_tick: None,
            source_key: None,
            status: Default::default(),
        },
    ];

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    assert_eq!(jobs.len(), 2);

    // Every patched demo lands directly in dod/ now -- see issue #8.
    let expected_dod_dir = std::path::Path::new(&config.game_path)
        .parent()
        .unwrap()
        .join("dod");

    let primer = &jobs[0];
    assert_eq!(
        primer.output_demo,
        expected_dod_dir.join("dodstudio_primer.dem")
    );
    assert_eq!(primer.streaks.len(), 0);

    let job = &jobs[1];
    assert_eq!(job.source_demo, "demo1.dem");
    assert_eq!(
        job.output_demo,
        expected_dod_dir.join("dodstudio_chain_01.dem")
    );
    assert_eq!(job.streaks.len(), 2);
    assert_eq!(job.streaks[0].start_tick, 1000);
    assert_eq!(job.streaks[0].end_tick, 1500); // Merged 1000-1200 and 1300-1500
    assert_eq!(job.streaks[1].start_tick, 2000);
    assert_eq!(job.streaks[1].end_tick, 2200);

    // The merge above collapsed raw streaks 0 and 1 into one recording
    // block — the manifest has to say so, or a finished take can't be
    // traced back to every highlight it actually covers.
    assert_eq!(job.blocks.len(), 2, "one block per merged recording");
    assert_eq!(job.blocks[0].block_index, 0);
    assert_eq!(job.blocks[0].source_streak_indices, vec![0, 1]);
    assert_eq!(job.blocks[1].block_index, 1);
    assert_eq!(job.blocks[1].source_streak_indices, vec![2]);

    // Block bounds mirror the merged streak, and the take folder/key follow
    // the naming the helper cfg's _route_N alias writes to.
    assert_eq!(job.blocks[0].start_tick, 1000);
    assert_eq!(job.blocks[0].end_tick, 1500);
    assert_eq!(job.blocks[0].demo_name, "dodstudio_chain_01");
    assert!(
        job.blocks[0].take_folder.ends_with("dodstudio_chain_01_b0"),
        "expected take folder to end with dodstudio_chain_01_b0, got {:?}",
        job.blocks[0].take_folder
    );
    assert!(
        job.blocks[0].take_key.ends_with("/dodstudio_chain_01_b0"),
        "expected take key to end with /dodstudio_chain_01_b0, got {:?}",
        job.blocks[0].take_key
    );

    // The primer never records anything, so it must carry no blocks.
    assert!(primer.blocks.is_empty());
}

#[test]
fn fast_forward_uses_ff_speed_and_falls_back_when_it_is_not_positive() {
    let mut config = PatcherConfig::default();
    config.fast_forward_speed = 0.1;
    assert_eq!(fast_forward_host_framerate(&config), 0.1);
    for bad in [0.0, -1.0, f32::NAN] {
        config.fast_forward_speed = bad;
        assert_eq!(fast_forward_host_framerate(&config), 0.05, "{bad}");
    }
}

#[test]
fn the_helper_cfg_fast_forwards_at_the_configured_speed() {
    let mut config = PatcherConfig::default();
    let game = std::env::temp_dir().join("dod_test_ff_speed_helper_cfg");
    std::fs::create_dir_all(game.join("dod")).expect("dummy dod dir");
    // game_path is hl.exe; the helper cfg lands in the dod/ beside it.
    config.game_path = game.join("hl.exe").to_string_lossy().to_string();
    config.primary_media_dir = Some(game.clone());
    config.fast_forward_speed = 0.1;

    let streak = CaptureStreak {
        start_tick: 1000,
        end_tick: 1200,
        source_demo: "demo1.dem".to_string(),
        target_player: None,
        kill_count: 3,
        timeline_string: String::new(),
        duration_string: String::new(),
        player_index: 0,
        kills: Vec::new(),
        start_index: 0,
        end_index: 2,
        total_demo_frames: 3000,
        demo_fps: 100.0,
        viewdemo_times: Vec::new(),
        frame_times: std::sync::Arc::new(Vec::new()),
        match_start_tick: None,
        source_key: None,
        status: Default::default(),
    };
    build_batch_queue(vec![streak], &config, &std::collections::HashMap::new()).unwrap();

    let cfg = std::fs::read_to_string(game.join("dod").join("dodstudio_helper.cfg"))
        .expect("helper cfg written");
    let _ = std::fs::remove_dir_all(&game);
    assert!(
        cfg.contains("alias sys_fast_forward \"host_framerate 0.1\""),
        "{cfg}"
    );
}

/// Every scheduled command `build_batch_queue` emits is an alias name built
/// from `demo_name`, and `engine.rs` refuses anything longer than
/// `MAX_CONSOLE_CMD_SAFE_LEN` when it writes the ConsoleCommand frame (#454)
/// -- a too-long name would fail the whole patch. The
/// `dodstudio_` prefix (#197) made all of these 9 bytes longer, so pin it.
#[test]
fn every_injected_command_fits_a_console_command_frame() {
    let mut config = PatcherConfig::default();
    let temp_game_path = std::env::temp_dir().join("dod_test_cbuf_len");
    std::fs::create_dir_all(temp_game_path.join("dod")).expect("dummy dod dir");
    config.game_path = temp_game_path.to_string_lossy().to_string();
    config.primary_media_dir = Some(temp_game_path.clone());

    let raw_streaks = vec![CaptureStreak {
        start_tick: 1000,
        end_tick: 1200,
        source_demo: "demo1.dem".to_string(),
        target_player: None,
        kill_count: 3,
        timeline_string: String::new(),
        duration_string: String::new(),
        player_index: 0,
        kills: Vec::new(),
        start_index: 0,
        end_index: 2,
        total_demo_frames: 3000,
        demo_fps: 100.0,
        viewdemo_times: Vec::new(),
        frame_times: std::sync::Arc::new(Vec::new()),
        match_start_tick: None,
        source_key: None,
        status: Default::default(),
    }];

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();

    let total_commands: usize = jobs.iter().map(|j| j.scheduled_commands.len()).sum();
    assert!(
        total_commands > 0,
        "nothing to check -- the fixture produced no scheduled commands at all"
    );

    for job in &jobs {
        for (tick, cmd) in &job.scheduled_commands {
            assert!(
                cmd.len() < crate::patch::MAX_CONSOLE_CMD_SAFE_LEN,
                "scheduled command {cmd:?} at tick {tick} is {} bytes, at or over the {}-byte command field a ConsoleCommand frame holds",
                cmd.len(),
                crate::patch::MAX_CONSOLE_CMD_SAFE_LEN
            );
        }
    }
}

#[test]
fn test_drive_headroom_omits_drive_0_when_no_block_is_routed_there() {
    // Patched demo files land directly in dod/ now (see issue #8), not
    // on any capture_directories entry, so drive 0 no longer needs a
    // special unconditional include -- it should be omitted from
    // drive_headroom exactly like any other drive that received no
    // real recording block.
    let mut config = mock_config();
    let temp_drive = std::env::temp_dir().join("dod_test_headroom_drive0_no_block");
    std::fs::create_dir_all(&temp_drive).expect("failed to create dummy capture drive");
    config.capture_directories = vec![temp_drive.clone()];

    let (jobs, drive_headroom) =
        build_batch_queue(Vec::new(), &config, &std::collections::HashMap::new()).unwrap();

    assert!(jobs.is_empty(), "no streaks should produce no jobs");
    assert!(
        drive_headroom.is_empty(),
        "no block was ever routed to drive 0, so it shouldn't be reported at all, got {:?}",
        drive_headroom
    );
}

#[test]
fn test_drive_headroom_includes_drive_0_when_a_block_actually_lands_there() {
    let mut config = mock_config();
    let temp_drive = std::env::temp_dir().join("dod_test_headroom_drive0_with_block");
    std::fs::create_dir_all(&temp_drive).expect("failed to create dummy capture drive");
    config.capture_directories = vec![temp_drive.clone()];
    let raw_streaks = vec![streak_with_kills(1000, 1200, &[1000, 1200])];

    let (_jobs, drive_headroom) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();

    assert_eq!(
        drive_headroom.len(),
        1,
        "the one real block routes to drive 0, so it should be reported"
    );
    assert_eq!(drive_headroom[0].path, temp_drive);
    assert!(
        drive_headroom[0].free_bytes > 0 && drive_headroom[0].free_bytes < u64::MAX,
        "expected a real free-byte count for an existing directory, got {}",
        drive_headroom[0].free_bytes
    );
}

#[test]
fn test_patched_demos_land_directly_in_dod_not_on_a_capture_directory() {
    // See issue #8: capture_directories is only about where recorded
    // video blocks land now, never these small demo files -- both the
    // primer and every chained job's output_demo must resolve under
    // the game's own dod/ folder (derived from game_path), regardless
    // of what capture_directories is configured to.
    let mut config = mock_config();
    config.capture_directories = vec![std::env::temp_dir().join("dod_test_unused_capture_dir")];
    let raw_streaks = vec![streak_with_kills(1000, 1200, &[1000, 1200])];

    let (jobs, _drive_headroom) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();

    let expected_dod_dir = std::path::Path::new(&config.game_path)
        .parent()
        .unwrap()
        .join("dod");
    for job in &jobs {
        assert_eq!(
            job.output_demo.parent(),
            Some(expected_dod_dir.as_path()),
            "expected {:?} to land in dod/, not a capture_directories entry",
            job.output_demo
        );
    }
}

#[test]
fn test_drive_headroom_omits_unconfigured_drives_when_capture_directories_is_empty() {
    // No capture directories configured at all -> allocate_blocks_first_fit_decreasing
    // runs against the single u64::MAX sentinel drive (index 0), but
    // there's no real path at config.capture_directories[0] to report a
    // headroom entry for, so the returned vector must come back empty
    // rather than panicking on the out-of-bounds lookup.
    let config = mock_config(); // capture_directories left at its Vec::new() default
    let raw_streaks = vec![streak_with_kills(1000, 1200, &[1000, 1200])];

    let (jobs, drive_headroom) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();

    assert_eq!(jobs.len(), 2, "primer + one chained job");
    assert!(
        drive_headroom.is_empty(),
        "no configured capture directories means nothing to report headroom for, got {:?}",
        drive_headroom
    );
}

#[test]
fn workspace_guard_drop_actually_removes_chain_demos_when_auto_clear_is_on() {
    // Verifies the fix for issue #12: the cleanup filter and the
    // output-demo naming in `build_batch_queue` disagreed, so the
    // filter never matched and these files were never cleaned up
    // regardless of the auto_clear_temp_demos setting. Both sides now
    // go through `CHAIN_DEMO_PREFIX`/`is_chain_demo_filename` (#197),
    // which is what keeps them from drifting apart again.
    let exit_trigger = std::env::temp_dir().join("dod_test_workspace_guard_exit_trigger");
    let dod_dir = exit_trigger.parent().unwrap().join("dod");
    std::fs::create_dir_all(&dod_dir).unwrap();
    std::fs::create_dir_all(&exit_trigger).unwrap();
    let chain_demo = dod_dir.join("dodstudio_chain_01.dem");
    std::fs::write(&chain_demo, b"fake demo bytes").unwrap();

    {
        let _guard = WorkspaceGuard {
            session_junction: std::env::temp_dir()
                .join("dod_test_workspace_guard_session_junction_nonexistent"),
            exit_trigger: exit_trigger.clone(),
            pool_junctions: Vec::new(),
            route_junctions: Vec::new(),
            auto_clear_logs: false,
            auto_clear_temp_demos: true,
            auto_clear_previews: false,
            save_local_patched_copy: false,
        };
    }

    assert!(
        !chain_demo.exists(),
        "dodstudio_chain_01.dem should have been removed by WorkspaceGuard::drop with auto_clear_temp_demos on"
    );

    let _ = std::fs::remove_dir_all(&dod_dir);
}

fn streak_with_kills(start_tick: i32, end_tick: i32, kill_frames: &[i32]) -> CaptureStreak {
    CaptureStreak {
        start_tick,
        end_tick,
        source_demo: "demo1.dem".to_string(),
        target_player: None,
        kill_count: kill_frames.len(),
        timeline_string: String::new(),
        duration_string: String::new(),
        player_index: 0,
        kills: kill_frames
            .iter()
            .map(|&f| (f, f as f32 / 100.0, "k98".to_string()))
            .collect(),
        start_index: 0,
        end_index: kill_frames.len().saturating_sub(1),
        total_demo_frames: 30000,
        demo_fps: 100.0,
        viewdemo_times: Vec::new(),
        frame_times: std::sync::Arc::new(Vec::new()),
        match_start_tick: None,
        source_key: None,
        status: Default::default(),
    }
}

/// A game folder of this test's own.
///
/// One folder per call, because `build_batch_queue` writes its helper and
/// chain `.cfg` files into the game folder and deletes the previous run's
/// on the way in. A shared path meant tests running in parallel deleted
/// each other's files mid-write, which surfaces as a sharing violation on
/// Windows and an `Err` out of a call every test `unwrap`s.
///
/// `game_path` names the executable, not the folder — the engine's content
/// sits beside `hl.exe` and every caller reads `game_path.parent()`. Naming
/// the folder here put `dod_dir` one level too high, at the temp root, so
/// the tests were writing to a single shared `%TEMP%/dod` regardless.
/// Named after the running test, which the harness puts on the thread. That
/// is unique per test and stable across runs, so folders are isolated
/// without a counter that would leave a fresh one behind every run.
fn mock_config() -> PatcherConfig {
    let tag = std::thread::current()
        .name()
        .map(|n| n.replace(|c: char| !c.is_ascii_alphanumeric(), "_"))
        .unwrap_or_else(|| "mock".to_string());
    mock_config_in(&tag)
}

fn mock_config_in(tag: &str) -> PatcherConfig {
    let root = std::env::temp_dir().join(format!("dod_test_{}", tag));
    std::fs::create_dir_all(root.join("dod")).expect("Failed to create dummy dod dir");
    let mut config = PatcherConfig::default();
    config.game_path = root.join("hl.exe").to_string_lossy().to_string();
    config.primary_media_dir = Some(root);
    config
}

#[test]
fn obs_mode_pins_fps_override_and_fps_max_to_obs_capture_fps() {
    let mut config = PatcherConfig::default();
    config.capture_mode = crate::patch::CaptureMode::Obs;
    config.capture_fps = 300; // must not leak into fps_max — see below
    config.obs_capture_fps = 120;
    let commands = final_init_commands(&config);
    assert!(
        commands.iter().any(|c| c == "fps_override 1"),
        "expected fps_override 1, got: {commands:?}"
    );
    assert!(
        commands.iter().any(|c| c == "fps_max 120"),
        "expected fps_max 120 (from obs_capture_fps, not capture_fps), got: {commands:?}"
    );
}

#[test]
fn frame_sequence_and_direct_to_video_never_touch_fps_max() {
    for mode in [
        crate::patch::CaptureMode::FrameSequence,
        crate::patch::CaptureMode::DirectToVideo,
    ] {
        let mut config = PatcherConfig::default();
        config.capture_mode = mode;
        config.capture_fps = 120;
        config.obs_capture_fps = 120;
        let commands = final_init_commands(&config);
        assert!(
            !commands
                .iter()
                .any(|c| c.starts_with("fps_override") || c.starts_with("fps_max")),
            "{mode:?} should not touch fps_override/fps_max, got: {commands:?}"
        );
    }
}

#[test]
fn the_pre_roll_floor_names_whichever_requirement_is_binding() {
    // With nothing else configured the audio resync sets the bar, since it
    // is the longest of the fixed terms.
    let mut config = mock_config();
    config.custom_commands.clear();
    let f = roll_floors(&config);
    assert_eq!(f.pre_roll, AUDIO_RESYNC_SECONDS);
    assert!(
        f.pre_roll_binding.contains("audio"),
        "{}",
        f.pre_roll_binding
    );
    assert_eq!(f.post_roll, 0.0, "nothing needs post-roll on its own");

    // A Scheduled Command further out than that takes over, because
    // anything beyond the real-time window fires during fast-forward.
    config.custom_commands = vec![crate::patch::CustomCommand {
        command: "mirv_movie_fps 500".to_string(),
        offset: 8.0,
        relation: CommandRelation::Before,
    }];
    let f = roll_floors(&config);
    assert_eq!(f.pre_roll, 8.0);
    assert!(
        f.pre_roll_binding.contains("Scheduled"),
        "{}",
        f.pre_roll_binding
    );

    // The start lead covers part of that distance, because the offset
    // anchors to the kill and recording starts a lead before it. The real
    // configuration this got wrong: a 10s command with a 5s start lead
    // needs only 5s of pre-roll, and warning at 10 was a false alarm.
    config.record_start_lead = 5.0;
    config.custom_commands[0].offset = 10.0;
    let f = roll_floors(&config);
    assert_eq!(
        f.scheduled_before, 5.0,
        "10s out, 5s of it covered by the lead"
    );
    assert_eq!(f.pre_roll, AUDIO_RESYNC_SECONDS.max(5.0));

    // And a lead longer than the offset leaves nothing for the pre-roll.
    config.record_start_lead = 12.0;
    let f = roll_floors(&config);
    assert_eq!(f.scheduled_before, 0.0, "never negative");

    // And an "After" command is the only thing that asks for post-roll.
    config.custom_commands = vec![crate::patch::CustomCommand {
        command: "echo done".to_string(),
        offset: 3.0,
        relation: CommandRelation::After,
    }];
    let f = roll_floors(&config);
    assert_eq!(f.post_roll, 3.0);
    assert_eq!(
        f.pre_roll, AUDIO_RESYNC_SECONDS,
        "an After command asks nothing of pre-roll"
    );
}

#[test]
fn turning_the_flush_off_drops_its_term_from_the_floor() {
    let mut config = mock_config();
    config.custom_commands.clear();

    config.decal_flush = true;
    assert_eq!(
        roll_floors(&config).flush_lead,
        crate::patch::DEFAULT_LEAD_SECONDS
    );

    config.decal_flush = false;
    assert_eq!(roll_floors(&config).flush_lead, 0.0);
}

#[test]
fn a_command_outside_the_real_time_window_is_flagged() {
    // Real time runs from the speed drop (pre-roll) to the end of the
    // post-roll. Either side of that the engine is at host_framerate 0.05.
    let (speed_drop, post_roll_end) = (1000, 2000);

    assert!(
        runs_during_fast_forward(999, speed_drop, post_roll_end),
        "before the speed drop"
    );
    assert!(
        runs_during_fast_forward(2001, speed_drop, post_roll_end),
        "after the post-roll"
    );
    assert!(
        !runs_during_fast_forward(1000, speed_drop, post_roll_end),
        "the drop itself"
    );
    assert!(
        !runs_during_fast_forward(1500, speed_drop, post_roll_end),
        "mid-clip"
    );
    assert!(
        !runs_during_fast_forward(2000, speed_drop, post_roll_end),
        "the last post-roll tick"
    );
}

#[test]
fn test_merged_block_records_through_the_absorbed_highlights_last_kill() {
    // Two highlights close enough to merge into one continuous recording.
    // The merge keeps the first streak's fields and only extends end_tick —
    // but the record-stop mark is derived from kills[end_index], so unless
    // the absorbed streak's final kill is carried over, recording stops at
    // the FIRST highlight's last kill and the merged-in one never lands in
    // the take at all (while still being reported as one captured block).
    let config = mock_config();
    let raw_streaks = vec![
        streak_with_kills(1000, 1200, &[1000, 1200]),
        streak_with_kills(1300, 1500, &[1300, 1500]),
    ];

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    let job = &jobs[1];

    assert_eq!(
        job.streaks.len(),
        1,
        "the two highlights should merge into one block"
    );
    assert_eq!(job.blocks[0].source_streak_indices, vec![0, 1]);

    let record_stop = job
        .scheduled_commands
        .iter()
        .find(|(_, cmd)| cmd == "sys_record_stop")
        .map(|(tick, _)| *tick)
        .expect("merged block must schedule a record stop");

    assert!(
        record_stop >= 1500,
        "recording stopped at tick {} — before the merged-in highlight's last kill at 1500, \
         so that highlight was never actually captured",
        record_stop
    );
}

#[test]
fn test_demo_start_marker_carries_job_idx_total_and_clip_count() {
    // Two demos, one block each -> two real jobs (plus the primer at
    // jobs[0], which carries no DEMO_START of its own — see issue #98).
    let config = mock_config();
    let mut raw_streaks = vec![streak_with_kills(1000, 1200, &[1000, 1200])];
    let mut second_demo = streak_with_kills(1000, 1200, &[1000, 1200]);
    second_demo.source_demo = "demo2.dem".to_string();
    raw_streaks.push(second_demo);

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    assert_eq!(jobs.len(), 3, "primer + one job per demo");
    assert!(
        jobs[0]
            .scheduled_commands
            .iter()
            .all(|(_, c)| !c.contains("DEMO_START")),
        "the primer plays no real demo and must not announce one"
    );

    for (job, expected_idx) in [(&jobs[1], 1), (&jobs[2], 2)] {
        let echo = job
            .scheduled_commands
            .iter()
            .find(|(_, c)| c.contains("DEMO_START"))
            .map(|(_, c)| c.clone())
            .unwrap_or_else(|| panic!("job {:?} has no DEMO_START echo", job.output_demo));
        assert!(
            echo.contains(&format!("DEMO_START {} 2 1", expected_idx)),
            "expected job idx {} of 2, 1 clip in {:?}",
            expected_idx,
            echo
        );
    }
}

#[test]
fn test_next_clip_marker_announces_clip_1_and_each_later_non_chained_clip() {
    // Two demos, two non-overlapping (non-chained) clips each -> NEXT_CLIP
    // for clip 1 alongside DEMO_START, NEXT_CLIP for clip 2 alongside the
    // Stage-4 FAST_FORWARD, and nothing after the last clip in the demo.
    let config = mock_config();
    let mut raw_streaks = vec![
        streak_with_kills(1000, 1200, &[1000, 1200]),
        streak_with_kills(5000, 5200, &[5000, 5200]),
    ];
    let mut second_demo = streak_with_kills(1000, 1200, &[1000, 1200]);
    second_demo.source_demo = "demo2.dem".to_string();
    let mut second_demo_b = streak_with_kills(5000, 5200, &[5000, 5200]);
    second_demo_b.source_demo = "demo2.dem".to_string();
    raw_streaks.push(second_demo);
    raw_streaks.push(second_demo_b);

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    assert_eq!(jobs.len(), 3, "primer + one job per demo");

    for (job, expected_job_idx) in [(&jobs[1], 1), (&jobs[2], 2)] {
        let next_clip_echoes: Vec<&String> = job
            .scheduled_commands
            .iter()
            .filter(|(_, c)| c.contains("NEXT_CLIP"))
            .map(|(_, c)| c)
            .collect();
        assert_eq!(
            next_clip_echoes.len(),
            2,
            "expected one NEXT_CLIP for clip 1 and one for clip 2, got {:?}",
            next_clip_echoes
        );
        assert!(
            next_clip_echoes
                .iter()
                .any(|c| c.contains(&format!("NEXT_CLIP {} 2 1 2", expected_job_idx))),
            "missing clip-1-of-2 NEXT_CLIP for job {} in {:?}",
            expected_job_idx,
            next_clip_echoes
        );
        assert!(
            next_clip_echoes
                .iter()
                .any(|c| c.contains(&format!("NEXT_CLIP {} 2 2 2", expected_job_idx))),
            "missing clip-2-of-2 NEXT_CLIP for job {} in {:?}",
            expected_job_idx,
            next_clip_echoes
        );
    }
}

/// Ticks at which a given command is scheduled, in order.
fn ticks_for(job: &PatchJob, cmd: &str) -> Vec<i32> {
    job.scheduled_commands
        .iter()
        .filter(|(_, c)| c == cmd)
        .map(|(t, _)| *t)
        .collect()
}

#[test]
fn test_non_overlapping_recordings_stay_separate_takes() {
    // 40s apart at 100fps — far outside every window. Two independent
    // takes, each with its own fast-forward round trip.
    let config = mock_config();
    let raw_streaks = vec![
        streak_with_kills(1000, 1200, &[1000, 1200]),
        streak_with_kills(5000, 5200, &[5000, 5200]),
    ];

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    let job = &jobs[1];

    assert_eq!(job.streaks.len(), 2, "should not merge");
    assert_eq!(job.blocks.len(), 2);
    assert_eq!(ticks_for(job, "sys_record_start").len(), 2);
    assert_eq!(ticks_for(job, "sys_record_stop").len(), 2);
    // Each block drops out of fast-forward for itself, and the first block
    // resumes it afterwards.
    assert_eq!(
        ticks_for(job, "sys_normal_speed").len(),
        6,
        "3-frame redundancy per block"
    );
    assert_eq!(ticks_for(job, "sys_sound").len(), 2);
}

#[test]
fn test_blocks_carry_the_record_bounds_the_decal_flush_keys_off() {
    // A block's start_tick/end_tick are the highlight's own bounds — not
    // the frames HLAE records between, which are computed separately in the
    // scheduling loop. The decal flush strips every decal outside a block's
    // recorded frames, so bounds that came back wrong (or as 0) would scrub
    // the very clip the pass exists to protect.
    let config = mock_config();
    let raw_streaks = vec![
        streak_with_kills(1000, 1200, &[1000, 1200]),
        streak_with_kills(5000, 5200, &[5000, 5200]),
    ];

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    let job = &jobs[1];

    assert_eq!(job.blocks.len(), 2);

    // scheduled_commands is tick-sorted and the blocks don't overlap, so
    // the Nth start pairs with the Nth stop.
    let mut expected: Vec<(i32, i32)> = ticks_for(job, "sys_record_start")
        .into_iter()
        .zip(ticks_for(job, "sys_record_stop"))
        .collect();
    expected.sort_unstable();

    let mut reported: Vec<(i32, i32)> = job
        .blocks
        .iter()
        .map(|b| (b.record_start_tick, b.record_stop_tick))
        .collect();
    reported.sort_unstable();

    assert_eq!(
        reported, expected,
        "blocks must report the same frames the capture actually records between"
    );
    for b in &job.blocks {
        assert!(
            b.record_start_tick > 0 && b.record_stop_tick >= b.record_start_tick,
            "block {} has an unusable record window {}..{}",
            b.block_index,
            b.record_start_tick,
            b.record_stop_tick
        );
    }
}

#[test]
fn test_decal_flush_pins_the_ring_once_at_demo_load_and_never_again() {
    // r_decals bounds how far the rotating decal index may travel before it
    // wraps; it evicts nothing. Setting it a second time, lower, strands
    // every decal above the new limit permanently — so exactly one command
    // may own it, and it has to land at demo load.
    let mut config = mock_config();
    config.decal_ring_limit = 128;

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();
    let job = &jobs[1];

    assert_eq!(
        job.init_commands
            .iter()
            .filter(|c| c.starts_with("r_decals"))
            .count(),
        1,
        "exactly one command may own the ring: {:?}",
        job.init_commands
    );
    assert_eq!(
        job.init_commands.last().map(String::as_str),
        Some("r_decals 128"),
        "and it has to land at demo load: {:?}",
        job.init_commands
    );
    assert!(
        !job.scheduled_commands
            .iter()
            .any(|(_, c)| c.starts_with("r_decals")),
        "r_decals must never be touched mid-demo — that is what strands decals"
    );
}

#[test]
fn test_an_init_command_owns_the_ring_and_is_not_pinned_over() {
    // The sweep is sized to the ring, so the two are one number and
    // r_decals is where the engine reads it. Appending a second value could
    // only overrule what was asked for, with nothing on screen to show it:
    // the capture would run a ring the user did not choose, swept correctly
    // for a size they did not ask for.
    let mut config = mock_config();
    config.decal_ring_limit = 128;
    config.init_commands = vec!["r_decals 512".to_string()];

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    let pins: Vec<&String> = jobs[1]
        .init_commands
        .iter()
        .filter(|c| c.starts_with("r_decals"))
        .collect();
    assert_eq!(
        pins,
        vec![&"r_decals 512".to_string()],
        "the user's own line is the pin: {:?}",
        jobs[1].init_commands
    );
}

#[test]
fn test_decals_switched_off_entirely_is_left_alone() {
    // r_decals 0 means no decals at all. There is no ring to turn and
    // nothing to clear, so the flush has no work and no business pinning
    // the cvar back up to a value that would start collecting them.
    let mut config = mock_config();
    config.init_commands = vec!["r_decals 0".to_string()];

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert_eq!(
        jobs[1]
            .init_commands
            .iter()
            .filter(|c| c.starts_with("r_decals"))
            .count(),
        1,
        "the user's r_decals 0 must survive untouched: {:?}",
        jobs[1].init_commands
    );
}

#[test]
fn test_decal_flush_disabled_leaves_r_decals_untouched() {
    let mut config = mock_config();
    config.decal_flush = false;

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert!(
        !jobs[1]
            .init_commands
            .iter()
            .any(|c| c.starts_with("r_decals")),
        "with the flush off the pipeline must not touch the cvar at all"
    );
}

#[test]
fn test_a_maximum_ring_sweep_stops_pinning_the_cvar() {
    // r_decals is clamped to MAX_RENDER_DECALS, so a sweep that size turns
    // a full revolution whatever the cvar is. Pinning then buys nothing and
    // costs the precondition the rest of the design works around — that
    // nothing else may set r_decals.
    let mut config = mock_config();
    config.decal_ring_limit = crate::patch::MAX_RENDER_DECALS;

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert!(
        !jobs[1]
            .init_commands
            .iter()
            .any(|c| c.starts_with("r_decals")),
        "a maximum sweep must leave the cvar alone: {:?}",
        jobs[1].init_commands
    );
}

/// As `mock_config`, with a `config.cfg` in the game folder so the config
/// scan sees exactly what the test put there and nothing else.
///
/// The tag is fixed rather than counted: scans are cached per folder, so a
/// case has to name its own folder to be sure of what it is reading.
fn mock_config_with_game_cfg(tag: &str, cfg_body: &str) -> PatcherConfig {
    let config = mock_config_in(&format!("ring_{}", tag));
    let cfg = std::path::Path::new(&config.game_path)
        .parent()
        .expect("game folder")
        .join("dod")
        .join("config.cfg");
    if cfg_body.is_empty() {
        let _ = std::fs::remove_file(&cfg);
    } else {
        std::fs::write(&cfg, cfg_body).expect("config.cfg");
    }
    config
}

#[test]
fn test_a_game_config_setting_the_cvar_to_zero_is_now_left_alone() {
    // Regression: this used to assert the opposite — that a config's
    // `r_decals 0` got silently overridden with a pin (to MAX_RENDER_DECALS
    // in this exact case), on the reasoning that adopting it would leave a
    // ring the engine keeps nothing in. That override is gone: r_decals now
    // follows the same config precedence mirv_fov always has, and a
    // resulting 0 is caught and reported as its own fact
    // (`decal_flush_is_noop` / `prepare_flushed_source`'s own zero-check)
    // rather than silently patched away.
    let mut config = mock_config_with_game_cfg("zero", "r_decals 0\nfps_max 999\n");
    config.decal_ring_limit = crate::patch::MAX_RENDER_DECALS;

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert!(
        !jobs[1]
            .init_commands
            .iter()
            .any(|c| c.starts_with("r_decals")),
        "the config's own 0 already stands, nothing left to pin: {:?}",
        jobs[1].init_commands
    );
    assert_eq!(
        crate::patch::ring_limit(&config),
        0,
        "and ring_limit agrees"
    );
}

#[test]
fn test_a_nonzero_game_config_value_is_adopted_without_a_pin() {
    // The config's own line already achieves what a pin would — appending
    // a second, identical-in-effect command would be pure noise.
    let config = mock_config_with_game_cfg("nonzero", "r_decals 512\n");
    // decal_ring_limit stays at its default (256): if the config's value
    // were being ignored in favor of it, the pin would read 256, not 512.

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert!(
        !jobs[1]
            .init_commands
            .iter()
            .any(|c| c.starts_with("r_decals")),
        "the config's own line is already the pin, nothing to append: {:?}",
        jobs[1].init_commands
    );
    assert_eq!(crate::patch::ring_limit(&config), 512);
}

#[test]
fn test_a_maximum_ring_leaves_the_cvar_alone_when_no_config_touches_it() {
    // The other half of the rule, against a game folder proven empty rather
    // than one that merely happens to be.
    let mut config = mock_config_with_game_cfg("clean", "");
    config.decal_ring_limit = crate::patch::MAX_RENDER_DECALS;

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert!(
        !jobs[1]
            .init_commands
            .iter()
            .any(|c| c.starts_with("r_decals")),
        "nothing else sets the cvar, so the pin buys nothing: {:?}",
        jobs[1].init_commands
    );
}

#[test]
fn test_a_config_never_overrides_what_init_commands_state() {
    // The config only decides whether the pin is worth spending. It is
    // never adopted as a value — `init_commands` stays the authority, and a
    // stated maximum is still the user's own line and still the pin.
    let mut config = mock_config_with_game_cfg("stated", "r_decals 0\n");
    config.decal_ring_limit = 256;
    config.init_commands = vec!["r_decals 4096".to_string()];

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    let pins: Vec<&String> = jobs[1]
        .init_commands
        .iter()
        .filter(|c| c.starts_with("r_decals"))
        .collect();
    assert_eq!(
        pins,
        vec![&"r_decals 4096".to_string()],
        "exactly the stated line, appended once"
    );
}

#[test]
fn test_anything_below_the_maximum_still_pins() {
    // Below the ceiling the sweep only covers the ring it was sized for, so
    // the cvar has to be held there or the sweep under-clears.
    let mut config = mock_config();
    config.decal_ring_limit = crate::patch::MAX_RENDER_DECALS - 1;

    let (jobs, _) = build_batch_queue(
        vec![streak_with_kills(1000, 1200, &[1000, 1200])],
        &config,
        &std::collections::HashMap::new(),
    )
    .unwrap();

    assert_eq!(
        jobs[1].init_commands.last().map(String::as_str),
        Some("r_decals 4095")
    );
}

#[test]
fn test_colliding_rolls_keep_separate_takes_without_fast_forwarding_between() {
    // Recordings are 2s apart (200 ticks @ 100fps): clear of the 1s
    // separation guard so they stay two takes, but inside the 2.6s the
    // pre-roll and post-roll need, so the fast-forward round trip between
    // them can't fit and is dropped instead of collapsing the two
    // highlights into one clip full of dead air.
    let mut config = mock_config();
    config.pre_roll_seconds = 2.0;
    config.post_roll_seconds = 0.6;
    let raw_streaks = vec![
        streak_with_kills(1000, 1200, &[1000, 1200]),
        streak_with_kills(1400, 1600, &[1400, 1600]),
    ];

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    let job = &jobs[1];

    assert_eq!(
        job.streaks.len(),
        2,
        "recordings don't overlap, so don't merge"
    );
    assert_eq!(job.blocks.len(), 2);
    assert_eq!(job.blocks[0].source_streak_indices, vec![0]);
    assert_eq!(job.blocks[1].source_streak_indices, vec![1]);

    // Both clips still get their own recording.
    assert_eq!(ticks_for(job, "sys_record_start").len(), 2);
    assert_eq!(ticks_for(job, "sys_record_stop").len(), 2);

    // But only the first block exits fast-forward, and nothing re-enters it
    // between the two — playback simply stays at normal speed across the gap.
    assert_eq!(
        ticks_for(job, "sys_normal_speed").len(),
        3,
        "only the first block should drop out of fast-forward"
    );
    assert_eq!(
        ticks_for(job, "sys_sound").len(),
        1,
        "no fast-forward before the second clip means no audio to flush"
    );
    let fast_forwards = ticks_for(job, "sys_fast_forward");
    let record_stops = ticks_for(job, "sys_record_stop");
    assert!(
        !fast_forwards
            .iter()
            .any(|&t| t > record_stops[0] && t < record_stops[1]),
        "fast-forward must not be scheduled between the two takes, got {:?}",
        fast_forwards
    );
}

#[test]
fn test_takes_too_close_together_are_merged_rather_than_restarted() {
    // Only 0.5s (50 ticks @ 100fps) between one recording stopping and the
    // next starting — under MIN_TAKE_SEPARATION_SECONDS, so they merge
    // instead of risking a stop/start cycle that tight.
    let config = mock_config();
    let raw_streaks = vec![
        streak_with_kills(1000, 1200, &[1000, 1200]),
        streak_with_kills(1250, 1400, &[1250, 1400]),
    ];

    let (jobs, _) =
        build_batch_queue(raw_streaks, &config, &std::collections::HashMap::new()).unwrap();
    let job = &jobs[1];

    assert_eq!(job.streaks.len(), 1, "too close to be separate takes");
    assert_eq!(job.blocks[0].source_streak_indices, vec![0, 1]);
    assert_eq!(ticks_for(job, "sys_record_start").len(), 1);
    assert_eq!(ticks_for(job, "sys_record_stop").len(), 1);
}

#[test]
fn test_blocks_merge_only_when_padding_closes_the_gap() {
    // 100-tick gap between blocks. No padding: stays separate.
    assert!(!blocks_merge(1000, 1100, 0, 0));
    // Pre-roll alone reaches back far enough.
    assert!(blocks_merge(1000, 1100, 100, 0));
    // Post-roll alone reaches forward far enough.
    assert!(blocks_merge(1000, 1100, 0, 100));
    // Split across both, summing to exactly the gap — boundary is inclusive.
    assert!(blocks_merge(1000, 1100, 60, 40));
    // One tick short.
    assert!(!blocks_merge(1000, 1100, 60, 39));
}

#[test]
fn test_blocks_merge_clamps_negative_adjusted_start() {
    // A highlight near tick 0 with a large pre-roll must clamp at 0 rather
    // than going negative and merging with something it doesn't touch.
    assert!(blocks_merge(0, 50, 500, 0));
    assert!(!blocks_merge(-100, 50, 500, 0));
}

#[test]
fn test_ffd_allocation_succeeds_where_arrival_order_would_strand_a_block() {
    // Two drives: only drive 0 (capacity 7) is big enough for the size-7
    // block; drive 1 tops out at 6. In arrival order (7 is block 1, not
    // first), naive first-fit would let the size-3 block (block 0) land on
    // drive 0 first, leaving only 4 free there — too little for the size-7
    // block, which drive 1 could never fit either. FFD places the size-7
    // block first (while drive 0 is still empty), so both other blocks can
    // still find room afterward.
    let block_estimates = vec![3u64, 7u64, 2u64];
    let mut drive_free = vec![7u64, 6u64];
    let mut active_drive_idx = 0usize;

    let assignments = allocate_blocks_first_fit_decreasing(
        &block_estimates,
        &mut drive_free,
        &mut active_drive_idx,
        0,
    )
    .expect("FFD should find a placement that naive arrival-order first-fit would miss");

    assert_eq!(assignments.len(), 3);
    let drive_of = |block: usize| assignments.iter().find(|(b, _)| *b == block).unwrap().1;

    // The size-7 block must land on drive 0 — it's the only drive with room for it.
    assert_eq!(drive_of(1), 0);
    assert_eq!(drive_free[0], 0); // 7 - 7 exactly consumed
    assert_eq!(drive_free[1], 6 - 3 - 2); // blocks 0 and 2 backfilled drive 1
}

#[test]
fn test_ffd_allocation_fails_when_no_drive_has_room() {
    let block_estimates = vec![10u64];
    let mut drive_free = vec![5u64, 5u64];
    let mut active_drive_idx = 0usize;

    let result = allocate_blocks_first_fit_decreasing(
        &block_estimates,
        &mut drive_free,
        &mut active_drive_idx,
        0,
    );

    assert_eq!(result, Err(0));
}

/// A regression guard for #198's fix -- `workspace_guard_drop_actually_
/// removes_chain_demos_when_auto_clear_is_on` above already covers the
/// enabled case; this is the one combination it doesn't, and the reason
/// `remove_file_retrying` is called through `auto_clear_temp_demos &&
/// !save_local_patched_copy` rather than unconditionally.
#[test]
fn dropping_the_workspace_guard_keeps_demos_when_a_local_copy_was_requested() {
    let root = Scratch::new("workspace_guard_keep");
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(dod.join("dodstudio_primer.dem"), b"x").unwrap();
    std::fs::write(dod.join("dodstudio_chain_01.dem"), b"x").unwrap();

    {
        let _guard = WorkspaceGuard {
            session_junction: root.join("session_junction"),
            exit_trigger: root.join("exit_trigger"),
            pool_junctions: vec![],
            route_junctions: vec![],
            auto_clear_logs: false,
            auto_clear_temp_demos: true,
            auto_clear_previews: false,
            save_local_patched_copy: true,
        };
    }

    assert!(dod.join("dodstudio_primer.dem").exists());
    assert!(dod.join("dodstudio_chain_01.dem").exists());

    let _ = std::fs::remove_dir_all(&root);
}
