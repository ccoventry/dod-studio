use super::autosave::fixtures::{autosave_job, session, settings};
use super::*;

#[test]
fn renders_while_a_capture_runs_follow_the_setting_within_the_batchs_own_number() {
    assert_eq!(concurrency_cap(4, None), 4);
    assert_eq!(concurrency_cap(4, Some(1)), 1);
    assert_eq!(concurrency_cap(4, Some(3)), 3);
    // Never more than the Render tab's own number, never none.
    assert_eq!(concurrency_cap(2, Some(4)), 2);
    assert_eq!(concurrency_cap(4, Some(0)), 1);
    assert_eq!(concurrency_cap(0, None), 1);
    assert_eq!(concurrency_cap(0, Some(1)), 1);
}

/// #1438: while a capture holds for room on one drive, queued renders of
/// takes there start first; the rest keep their order behind them.
#[test]
fn renders_that_free_the_held_drive_start_first() {
    assert_eq!(
        start_order([(0, false), (2, true), (3, false), (5, true)]),
        vec![2, 5, 0, 3]
    );
    assert_eq!(start_order([(1, false), (4, false)]), vec![1, 4]);
    assert!(start_order([]).is_empty());
}

#[test]
fn the_scan_count_is_sent_at_most_every_33_ms_and_once_at_the_end() {
    let start = std::time::Instant::now();
    let at = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut throttle = ScanCountThrottle::default();
    assert_eq!(throttle.found_one(at(0)), Some(1));
    assert_eq!(throttle.found_one(at(10)), None);
    assert_eq!(throttle.found_one(at(20)), None);
    assert_eq!(throttle.found_one(at(40)), Some(4));
    assert_eq!(throttle.last(), None, "the last count went out already");
    assert_eq!(throttle.found_one(at(50)), None);
    assert_eq!(throttle.last(), Some(5));
}

fn attempt(stream: &str, outcome: RenderOutcome, path: &str) -> RenderAttempt {
    RenderAttempt {
        started_unix_ms: 1,
        stream: stream.to_string(),
        codec: "prores".to_string(),
        custom_codec_args: String::new(),
        fps: 300,
        outcome,
        output_path: path.to_string(),
        output_size_bytes: Some(7),
        error: None,
    }
}

fn entry(stream: &str, outcome: RenderOutcome, path: &str, exists: bool) -> HistoryEntry {
    HistoryEntry {
        attempt: attempt(stream, outcome, path),
        output_exists: exists,
    }
}

#[test]
fn a_stream_rendered_before_is_staged_from_its_newest_surviving_output() {
    let history = vec![
        entry("all", RenderOutcome::Finished, "E:/out/old.mov", true),
        entry("all", RenderOutcome::Failed, "", false),
        entry("hud:hud", RenderOutcome::Finished, "E:/out/hud.mov", true),
        entry("all", RenderOutcome::Finished, "E:/out/new.mov", true),
    ];
    assert_eq!(
        last_finished_output(&history, "all"),
        Some(("E:/out/new.mov".to_string(), Some(7)))
    );
    assert_eq!(
        last_finished_output(&history, "hud:hud"),
        Some(("E:/out/hud.mov".to_string(), Some(7)))
    );
    assert_eq!(last_finished_output(&history, "other"), None);
}

#[test]
fn a_deleted_or_failed_render_does_not_count() {
    let history = vec![
        entry("all", RenderOutcome::Finished, "E:/out/gone.mov", false),
        entry("all", RenderOutcome::Cancelled, "", false),
    ];
    assert_eq!(last_finished_output(&history, "all"), None);
    assert_eq!(last_finished_output(&[], "all"), None);
}

#[test]
fn a_recovered_batch_starts_with_a_config_rebuilt_from_autosave_and_settings() {
    let manager = RenderManager::new();
    let views = manager.install_recovered(
        session(vec![
            autosave_job("a", AutosaveJobStatus::Pending, true),
            autosave_job("b", AutosaveJobStatus::Completed, true),
        ]),
        &settings(),
    );
    assert_eq!(views.len(), 2);
    let (config, starting) = manager.start_config().expect("a recovered batch starts");
    assert_eq!(starting, 1);
    assert_eq!(config.source_folder, "Z:/no-such-takes");
    assert_eq!(config.fps, 300);
    assert_eq!(config.target_codec, RenderCodec::from_str_id("prores"));
    assert_eq!(config.export_directories, vec![PathBuf::from("E:/exports")]);
    assert_eq!(config.max_concurrent_renders, 3);
    assert!(!config.ffmpeg_path.is_empty());
    // Each job keeps its own settings, not the batch's.
    assert_eq!(views[0].codec_id, "h264");
    // A take with no recorded capture rate: the job's own Source FPS.
    assert!(
        views[0]
            .settings_summary
            .ends_with("@ 60fps (Render Settings)"),
        "{}",
        views[0].settings_summary
    );
}

/// "Resumable renders" off (#1451): the cleanup before a batch makes room
/// for the clips alone, with no fallback, so no take goes for segments.
#[test]
fn with_resumable_renders_off_the_first_wave_needs_the_clips_alone() {
    let clip = |frames: usize| ClipData {
        take_folder: "Z:/no-such-take".to_string(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: Some("sound.wav".to_string()),
        base_name: "t".to_string(),
        frame_count: frames,
        width: 64,
        height: 48,
        date: String::new(),
        video_file: None,
        alpha_folder: None,
        clip_name: None,
    };
    let queued: Vec<QueuedTake> = [100, 300]
        .into_iter()
        .map(|frames| QueuedTake {
            clip: clip(frames),
            codec: RenderCodec::ProRes422,
            custom_args: String::new(),
            fps: 300,
        })
        .collect();
    let (pieces, fallback) = first_wave_pieces(&queued, 2, PieceRoom::ClipAlone);
    assert_eq!(pieces, [64 * 48 * 3 * 300, 64 * 48 * 3 * 100]);
    assert_eq!(fallback, None);
    let (_, fallback) = first_wave_pieces(&queued, 2, PieceRoom::SegmentsFirst);
    assert_eq!(fallback, Some(pieces));
}

/// Segment room is only planned for when the setting is on and an
/// export folder's path is short enough for a segment folder (#1451
/// review): otherwise no take goes for room the render won't use.
#[test]
fn segment_room_is_planned_only_with_the_setting_on_and_a_short_enough_folder() {
    let short = PathBuf::from("D:/clips");
    let long = PathBuf::from(format!("D:/{}", "a".repeat(200)));
    let both = [long.clone(), short.clone()];
    assert_eq!(PieceRoom::for_batch(true, &both), PieceRoom::SegmentsFirst);
    assert_eq!(PieceRoom::for_batch(false, &both), PieceRoom::ClipAlone);
    assert_eq!(PieceRoom::for_batch(true, &[long]), PieceRoom::ClipAlone);
}

/// A job started after "Resumable renders" was changed mid-batch renders
/// with the new value, like the export folders (#1430).
#[test]
fn a_job_started_after_the_setting_changes_gets_the_new_value() {
    let manager = RenderManager::new();
    let handles = manager.handles();
    let jobs = recovered_jobs(&session(vec![autosave_job(
        "a",
        AutosaveJobStatus::Pending,
        true,
    )]));
    let per_job = |handles: &SchedulerHandles| PerJob {
        export_directories: Vec::new(),
        thread_share: 1,
        clip_tag: None,
        resumable: handles.resumable.is_on(),
        live_thread_share: Default::default(),
    };
    let config = RenderConfig::default();
    assert!(job_config(&config, &jobs[0], per_job(&handles)).resumable_renders);
    manager.resumable().saved(false);
    assert!(!job_config(&config, &jobs[0], per_job(&handles)).resumable_renders);
    assert!(
        config.resumable_renders,
        "the batch's own config is not what decides"
    );
}

/// A running job reads the share the scheduler sets after it started, so
/// its next segment recounts its threads (#1512).
#[test]
fn a_started_job_follows_the_schedulers_thread_share() {
    let jobs = recovered_jobs(&session(vec![autosave_job(
        "a",
        AutosaveJobStatus::Pending,
        true,
    )]));
    let live = native::hlcr::renderer::thread_share::LiveThreadShare::new(3);
    let config = job_config(
        &RenderConfig::default(),
        &jobs[0],
        PerJob {
            export_directories: Vec::new(),
            thread_share: 3,
            clip_tag: None,
            resumable: true,
            live_thread_share: live.clone(),
        },
    );
    live.set(1);
    assert_eq!(
        config.max_concurrent_renders, 3,
        "the share it started with"
    );
    assert_eq!(config.live_thread_share.current(), Some(1));
}

#[test]
fn reset_to_queued_on_a_recovered_finished_row_can_start() {
    let manager = RenderManager::new();
    manager.install_recovered(
        session(vec![autosave_job("b", AutosaveJobStatus::Completed, true)]),
        &settings(),
    );
    assert_eq!(
        manager.start_config().unwrap_err(),
        crate::messages::NOTHING_QUEUED_TO_RENDER
    );
    for job in manager.jobs.lock().unwrap().iter_mut() {
        requeue(job);
    }
    assert!(manager.start_config().is_ok());
}

#[test]
fn a_queued_job_saved_without_its_take_asks_for_a_scan() {
    let manager = RenderManager::new();
    manager.install_recovered(
        session(vec![autosave_job("old", AutosaveJobStatus::Pending, false)]),
        &settings(),
    );
    let err = manager.start_config().unwrap_err();
    assert!(
        err.starts_with("recovered batch: scan again to set "),
        "{}",
        err
    );
    assert!(err.contains("old"), "{}", err);
}

#[test]
fn the_autosave_prompt_stays_away_while_the_process_holds_a_batch() {
    let manager = RenderManager::new();
    assert!(!manager.holds_batch());
    assert!(manager.views().is_empty());
    manager.install_recovered(
        session(vec![autosave_job("a", AutosaveJobStatus::Pending, true)]),
        &settings(),
    );
    assert!(manager.holds_batch());
    assert_eq!(manager.views().len(), 1);
}
