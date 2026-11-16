use super::*;
use crate::patch::types::CaptureBlock;
use crate::test_support::Scratch;

/// `source_demo` deliberately points at nothing: every case below must
/// decide to skip before it ever opens the file, so a read attempt would
/// surface as a failure rather than a silent fallback.
fn job_with_blocks(blocks: Vec<CaptureBlock>) -> PatchJob {
    PatchJob {
        source_demo: "no_such_demo_should_ever_be_read.dem".to_string(),
        output_demo: std::path::PathBuf::from("unused_output.dem"),
        streaks: Vec::new(),
        target_player: None,
        init_commands: Vec::new(),
        scheduled_commands: Vec::new(),
        director_events: Vec::new(),
        block_routes: Vec::new(),
        blocks,
    }
}

fn block(block_index: usize, record_start_tick: i32, record_stop_tick: i32) -> CaptureBlock {
    CaptureBlock {
        demo_name: "dodstudio_chain_01".to_string(),
        block_index,
        drive_index: 0,
        take_folder: std::path::PathBuf::from("take"),
        take_key: String::new(),
        source_streak_indices: vec![block_index],
        start_tick: record_start_tick,
        end_tick: record_stop_tick,
        plan: None,
        record_start_tick,
        record_stop_tick,
        ramped_gaps: 0,
    }
}

/// `job_with_blocks`'s `source_demo` points at nothing, so this passing at
/// all proves the cancelled job returned before reading the demo -- a read
/// attempt would surface as a skip, not an `Err`.
#[test]
fn a_cancelled_batch_abandons_the_flush_before_reading_the_demo() {
    let job = job_with_blocks(vec![block(0, 100, 200)]);
    let mut config = PatcherConfig::default();
    config.decal_flush = true;

    let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let result = prepare_flushed_source(&job, &config, crate::patch::Cancel::new(&token));

    assert!(
        matches!(result, Err(crate::patch::Cancelled)),
        "a cancelled batch must not fall back to the unflushed demo and carry on"
    );
}

/// The mirror of the above: an uncancelled token must change nothing, or
/// every flush would start returning Cancelled.
#[test]
fn an_unset_token_leaves_the_flush_decision_alone() {
    let job = job_with_blocks(Vec::new());
    let mut config = PatcherConfig::default();
    config.decal_flush = true;

    let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let result = prepare_flushed_source(&job, &config, crate::patch::Cancel::new(&token));

    // No blocks -> nothing to keep clean -> a plain skip, not an error.
    assert!(matches!(result, Ok(None)));
}

#[test]
fn the_pipeline_never_inserts_the_r_decals_frame() {
    // Inserting that ConsoleCommand frame shifts every later frame ordinal
    // by +1, which desyncs the scheduled capture commands from the
    // patcher's frame counter — invisibly, since the demo still parses and
    // plays. The pipeline pins the cvar from init_commands instead.
    let config = PatcherConfig {
        decal_ring_limit: 128,
        ..PatcherConfig::default()
    };
    let opts = flush_options(&config);

    assert!(
        !opts.inject_r_decals_command,
        "the pipeline must not insert a console-command frame — init_commands owns r_decals"
    );
    assert_eq!(
        opts.ring_limit, 128,
        "the configured ring size must reach the sweep"
    );
}

#[test]
fn an_init_command_states_the_ring_and_the_sweep_is_sized_to_it() {
    // One number, read where the engine reads it. A separate setting could
    // only agree with the cvar or silently disagree, and a sweep sized to
    // the wrong one under-clears with nothing in the output to show it.
    let config = PatcherConfig {
        decal_ring_limit: 128,
        init_commands: vec!["r_decals 512".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(ring_limit(&config), 512);
    assert_eq!(flush_options(&config).ring_limit, 512);
}

/// A game folder laid out as the engine expects: `hl.exe` with `dod/`
/// beside it. Returns the path to the exe, and the guard the caller must
/// hold -- dropping it here would delete the folder before it was read.
fn fake_game(
    tag: &str,
    config_cfg: &str,
    movie_cfg: Option<&str>,
) -> (Scratch, std::path::PathBuf) {
    let root = Scratch::new(format_args!("fov_cfg_{tag}"));
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(dod.join("config.cfg"), config_cfg).unwrap();
    if let Some(movie) = movie_cfg {
        std::fs::write(dod.join("movie.cfg"), movie).unwrap();
    }
    let exe = root.join("hl.exe");
    std::fs::write(&exe, b"").unwrap();
    (root, exe)
}

#[test]
fn a_fov_the_game_config_sets_is_found_rather_than_assumed() {
    // The real shape, and the reason this exists: config.cfg ends with
    // `exec movie.cfg`, movie.cfg carries `mirv_fov 105`, and the app was
    // never told. Sizing the cone for the default 90 makes it ~7 degrees
    // too narrow and calls in-shot positions hidden.
    let (_root, exe) = fake_game("found", "exec movie.cfg\n", Some("mirv_fov \"105\"\n"));
    let config = PatcherConfig {
        game_path: exe.to_string_lossy().to_string(),
        ..PatcherConfig::default()
    };

    assert_eq!(
        capture_fov(&config),
        config.capture_fov,
        "init commands say nothing"
    );
    assert_eq!(capture_fov_resolved(&config), 105.0, "the config does");
}

#[test]
fn an_init_command_stating_the_default_value_still_outranks_the_config() {
    // The trap: `mirv_fov 90` is what the app seeds into a new install's
    // init commands, and 90 is also the configured default. Deciding
    // "was one stated?" by comparing against the default would read that as
    // silence and hand every defaulted install to its movie.cfg — here,
    // rendering the cone for 105 when the user asked for 90.
    let (_root, exe) = fake_game(
        "states_default",
        "exec movie.cfg\n",
        Some("mirv_fov \"105\"\n"),
    );
    let config = PatcherConfig {
        game_path: exe.to_string_lossy().to_string(),
        init_commands: vec!["mirv_fov 90".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(
        config.capture_fov, 90.0,
        "the default this could be confused with"
    );
    assert_eq!(capture_fov_from_init(&config.init_commands), Some(90.0));
    assert_eq!(capture_fov_resolved(&config), 90.0, "the user asked for 90");
}

#[test]
fn an_init_command_outranks_the_game_config() {
    let (_root, exe) = fake_game("outrank", "exec movie.cfg\n", Some("mirv_fov \"105\"\n"));
    let config = PatcherConfig {
        game_path: exe.to_string_lossy().to_string(),
        init_commands: vec!["mirv_fov 120".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(capture_fov_resolved(&config), 120.0);
}

#[test]
fn a_config_that_switches_decals_off_is_now_adopted_like_fov_is() {
    // Regression: this used to assert the opposite — that a config's
    // `r_decals 0` was ignored in favor of the app's own default, on the
    // reasoning that adopting it would silently stand the flush down.
    // Now that stand-down is its own loud, reported fact
    // (decal_flush_is_noop) rather than something to hide by disagreeing
    // with the config, so r_decals follows mirv_fov's precedence exactly:
    // init commands, then an executed config, then the app's default.
    let (_root, exe) = fake_game("decals_off", "exec movie.cfg\n", Some("r_decals \"0\"\n"));
    let config = PatcherConfig {
        game_path: exe.to_string_lossy().to_string(),
        ..PatcherConfig::default()
    };

    assert_eq!(
        ring_limit(&config),
        0,
        "the config's own value now wins, same as mirv_fov's would"
    );
}

#[test]
fn a_nonzero_r_decals_the_game_config_sets_is_adopted() {
    let (_root, exe) = fake_game(
        "decals_from_config",
        "exec movie.cfg\n",
        Some("r_decals \"512\"\n"),
    );
    let config = PatcherConfig {
        game_path: exe.to_string_lossy().to_string(),
        decal_ring_limit: 128,
        ..PatcherConfig::default()
    };

    assert_eq!(
        ring_limit(&config),
        512,
        "the config's value, not the app's own default"
    );
}

#[test]
fn an_init_command_still_outranks_a_game_config_for_r_decals() {
    let (_root, exe) = fake_game(
        "decals_init_outranks",
        "exec movie.cfg\n",
        Some("r_decals \"0\"\n"),
    );
    let config = PatcherConfig {
        game_path: exe.to_string_lossy().to_string(),
        init_commands: vec!["r_decals 512".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(
        ring_limit(&config),
        512,
        "stated in Initial Commands, so it wins outright"
    );
}

#[test]
fn the_last_r_decals_wins_like_the_console_does() {
    let config = PatcherConfig {
        init_commands: vec!["r_decals 512".to_string(), "r_decals 64".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(ring_limit(&config), 64);
}

#[test]
fn a_quoted_r_decals_in_init_commands_is_still_recognised() {
    // Real .cfg syntax quotes every value, and "Load from .cfg file…"
    // carries that straight into Initial Commands — a raw, unquoted
    // parse would silently read `r_decals "512"` as unstated and fall
    // through to the app's own default instead of the user's value.
    let config = PatcherConfig {
        decal_ring_limit: 128,
        init_commands: vec!["r_decals \"512\"".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(ring_limit_from_init(&config.init_commands), Some(512));
    assert_eq!(ring_limit(&config), 512);
}

#[test]
fn a_quoted_mirv_fov_in_init_commands_is_still_recognised() {
    let config = PatcherConfig {
        capture_fov: 90.0,
        init_commands: vec!["mirv_fov \"105\"".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(capture_fov_from_init(&config.init_commands), Some(105.0));
    assert_eq!(capture_fov(&config), 105.0);
}

#[test]
fn the_configured_default_fills_in_when_nothing_states_it() {
    let config = PatcherConfig {
        decal_ring_limit: 128,
        init_commands: vec!["mirv_fov 105".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(ring_limit_from_init(&config.init_commands), None);
    assert_eq!(ring_limit(&config), 128);
}

#[test]
fn a_ring_past_the_engine_ceiling_is_clamped_to_it() {
    // The engine clamps r_decals to MAX_RENDER_DECALS, so positions beyond
    // that would be spent turning a ring that had already come round.
    let config = PatcherConfig {
        init_commands: vec!["r_decals 99999".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(ring_limit(&config), crate::patch::MAX_RENDER_DECALS);
}

#[test]
fn a_longer_command_that_merely_starts_the_same_is_not_the_ring() {
    let config = PatcherConfig {
        decal_ring_limit: 256,
        init_commands: vec!["r_decals_enabled 1".to_string()],
        ..PatcherConfig::default()
    };

    assert_eq!(ring_limit_from_init(&config.init_commands), None);
    assert_eq!(ring_limit(&config), 256);
}

#[test]
fn decals_switched_off_entirely_leaves_the_demo_alone() {
    // r_decals 0 is no decals at all: no ring to turn, no bullet holes to
    // clear, and a sweep sized zero would be a burst with nowhere to put
    // anything.
    let config = PatcherConfig {
        init_commands: vec!["r_decals 0".to_string()],
        ..PatcherConfig::default()
    };
    let job = job_with_blocks(vec![block(0, 1000, 2000)]);

    assert_eq!(ring_limit(&config), 0);
    assert!(
        prepare_flushed_source(&job, &config, crate::patch::Cancel::never())
            .unwrap()
            .is_none()
    );
}

#[test]
fn flush_disabled_leaves_the_demo_alone() {
    let config = PatcherConfig {
        decal_flush: false,
        ..PatcherConfig::default()
    };
    let job = job_with_blocks(vec![block(0, 1000, 2000)]);

    assert!(
        prepare_flushed_source(&job, &config, crate::patch::Cancel::never())
            .unwrap()
            .is_none()
    );
}

#[test]
fn jobs_with_no_blocks_are_skipped() {
    // The primer job and preview jobs record nothing, so there is no clip
    // to keep clean and nothing to strip against.
    let job = job_with_blocks(Vec::new());

    assert!(
        prepare_flushed_source(
            &job,
            &PatcherConfig::default(),
            crate::patch::Cancel::never()
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn one_block_missing_its_record_bounds_skips_the_whole_job() {
    // A partial window set would strip the decals landing inside the clip
    // whose bounds went missing — worse than the dirty walls being fixed.
    let job = job_with_blocks(vec![block(0, 1000, 2000), block(1, 0, 0)]);

    assert!(keep_windows_for(&job).is_none());
    assert!(
        prepare_flushed_source(
            &job,
            &PatcherConfig::default(),
            crate::patch::Cancel::never()
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn an_inverted_record_window_skips_the_whole_job() {
    // A record stop clamped back to the end of the demo can land ahead of
    // its own start. That window keeps nothing, so the clip would be
    // stripped rather than protected.
    let job = job_with_blocks(vec![block(0, 1000, 2000), block(1, 9000, 8000)]);

    assert!(keep_windows_for(&job).is_none());
}

#[test]
fn a_single_frame_record_window_is_usable() {
    // With no record lead or trail, a one-kill highlight really does start
    // and stop on the same frame. Rejecting that would silently disable the
    // flush for the whole job.
    let job = job_with_blocks(vec![block(0, 1000, 2000), block(1, 5000, 5000)]);

    assert_eq!(
        keep_windows_for(&job),
        Some(vec![(1000, 2000), (5000, 5000)]),
        "equal record bounds are a real one-frame window, not a missing one"
    );
}

#[test]
fn the_cone_covers_the_corner_of_the_frame_not_its_edge() {
    // A decal at the corner of the frame is further off the view axis than
    // one at the edge, so the horizontal half-angle is not enough on its
    // own. Testing against it is what let a spot at 47 degrees pass as
    // "hidden" during a 90-degree capture.
    for fov in [75.0f32, 90.0, 105.0, 120.0] {
        let half = on_screen_half_angle(fov, 1920, 1080);
        assert!(
            half > fov / 2.0,
            "at {} degrees the cone was {:.1}, which does not even reach the frame edge at {:.1}",
            fov,
            half,
            fov / 2.0
        );
    }
}

#[test]
fn a_wider_capture_needs_a_wider_cone() {
    let at90 = on_screen_half_angle(90.0, 1920, 1080);
    let at105 = on_screen_half_angle(105.0, 1920, 1080);
    assert!(
        at105 > at90 + 5.0,
        "105 gave {:.1} against 90's {:.1} — a movie shot wide would keep spots that are in shot",
        at105,
        at90
    );
}

#[test]
fn the_old_fixed_cone_was_too_narrow_at_every_common_fov() {
    // Regression guard for the bug this replaced: 40 degrees, justified as
    // "generous" against a 90-degree FOV, was under even the 45-degree
    // horizontal half-angle.
    const OLD_FIXED_CONE: f32 = 40.0;
    for (fov, w, h) in [
        (90.0f32, 1920, 1080),
        (90.0, 1280, 960),
        (105.0, 1920, 1080),
    ] {
        assert!(
            on_screen_half_angle(fov, w, h) > OLD_FIXED_CONE,
            "fov {} at {}x{} still fits inside the old fixed cone",
            fov,
            w,
            h
        );
    }
}

#[test]
fn a_taller_frame_pushes_the_corner_further_out() {
    // 4:3 puts more of the frame vertically, so its corner sits further off
    // the axis than a 16:9 corner at the same horizontal FOV.
    let wide = on_screen_half_angle(90.0, 1920, 1080);
    let tall = on_screen_half_angle(90.0, 1280, 960);
    assert!(
        tall > wide,
        "4:3 {:.1} should exceed 16:9 {:.1}",
        tall,
        wide
    );
}

#[test]
fn a_nonsense_resolution_falls_back_rather_than_dividing_by_zero() {
    let half = on_screen_half_angle(90.0, 0, 0);
    assert!(half.is_finite() && half > 45.0);
}

#[test]
fn mirv_fov_in_the_init_commands_wins() {
    // The capture runs at whatever the console was last told, so reading it
    // from there is the only way the two cannot disagree.
    let mut config = PatcherConfig {
        capture_fov: 90.0,
        ..PatcherConfig::default()
    };
    assert_eq!(capture_fov(&config), 90.0);

    config.init_commands = vec!["exec autoexec".to_string(), "mirv_fov 105".to_string()];
    assert_eq!(capture_fov(&config), 105.0);

    // Last one wins, as at the console.
    config.init_commands.push("mirv_fov 100".to_string());
    assert_eq!(capture_fov(&config), 100.0);
}

#[test]
fn a_command_that_merely_starts_the_same_is_not_mistaken_for_it() {
    let config = PatcherConfig {
        capture_fov: 90.0,
        init_commands: vec!["mirv_fov_something_else 12".to_string()],
        ..PatcherConfig::default()
    };
    assert_eq!(capture_fov(&config), 90.0);
}

#[test]
fn the_pipeline_derives_its_cone_from_the_capture_fov() {
    // The whole point: a movie shot at mirv_fov 105 must not be planned
    // against a 90-degree frame.
    let config = PatcherConfig {
        resolution_width: 1920,
        resolution_height: 1080,
        init_commands: vec!["mirv_fov 105".to_string()],
        ..PatcherConfig::default()
    };
    let opts = flush_options(&config);
    assert!((opts.visibility_cone_degrees - on_screen_half_angle(105.0, 1920, 1080)).abs() < 0.01);
    assert!(opts.visibility_cone_degrees > 55.0);
}

#[test]
fn the_sweep_takes_only_its_own_stale_leavings() {
    // This deletes files, so what it will not touch matters more than what
    // it will. A concurrent capture's scratch is young; everything else in
    // the temp directory is not ours at any age.
    use std::time::{Duration, SystemTime};

    let dir = Scratch::new("sweep_test");

    let stale = dir.join(format!("{}old_1_0.dem", SCRATCH_PREFIX));
    let fresh = dir.join(format!("{}live_2_0.dem", SCRATCH_PREFIX));
    let other = dir.join("someone_elses_file.dem");
    for p in [&stale, &fresh, &other] {
        std::fs::write(p, b"x").unwrap();
    }

    // Backdate the stale one well past the threshold.
    let long_ago = SystemTime::now() - SCRATCH_STALE_AFTER - Duration::from_secs(60);
    filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(long_ago)).unwrap();

    sweep_stale_scratch(&dir);

    assert!(
        !stale.exists(),
        "an orphaned scratch demo should be removed"
    );
    assert!(
        fresh.exists(),
        "a scratch demo young enough to belong to a running capture must survive"
    );
    assert!(
        other.exists(),
        "files that are not ours must never be touched"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
