use super::*;
use crate::patch::builder::spawn_patch_batch;
use crate::patch::types::PatchEvent;
use crate::patch::types::{CaptureStreak, PatchJob, PatcherConfig};

#[test]
fn test_stream_patcher_end_to_end() {
    let scratch_dir = std::path::Path::new("scratch");
    if !scratch_dir.exists() {
        let _ = std::fs::create_dir_all(scratch_dir);
    }

    let input_path = scratch_dir.join("test_input.dem");
    let output_path = scratch_dir.join("test_output.dem");

    // Synthesize a valid minimal GoldSrc demo file
    let mut header = [0u8; 544];
    header[0..8].copy_from_slice(b"HLDEMO\0\0");
    // directory offset is at 540..544.
    // File structure:
    // Header: 544 bytes
    // Frame 1 (DemoStart, type 2): 9 bytes (offset 544..553)
    // Frame 2 (NextSection, type 5): 9 bytes (offset 553..562)
    // Directory count (i32): 4 bytes (offset 562..566)
    // Directory entry: 92 bytes (offset 566..658)
    // Total size = 658 bytes. Directory starts at offset 562.
    let directory_offset: i32 = 562;
    header[540..544].copy_from_slice(&directory_offset.to_le_bytes());

    let mut file_data = Vec::new();
    file_data.extend_from_slice(&header);

    // Frame 1: DemoStart (type 2), time = 0.0, tick = 0
    file_data.push(2);
    file_data.extend_from_slice(&0.0_f32.to_le_bytes());
    file_data.extend_from_slice(&0_i32.to_le_bytes());

    // Frame 2: NextSection (type 5), time = 0.0, tick = 0
    file_data.push(5);
    file_data.extend_from_slice(&0.0_f32.to_le_bytes());
    file_data.extend_from_slice(&0_i32.to_le_bytes());

    // Directory Block
    // Entry count: 1
    file_data.extend_from_slice(&1_i32.to_le_bytes());
    // Entry type: 1 (Playback)
    file_data.extend_from_slice(&1_i32.to_le_bytes());
    // Description: 64 bytes
    let mut desc = [0u8; 64];
    desc[..8].copy_from_slice(b"Playback");
    file_data.extend_from_slice(&desc);
    // flags (4), cd_track (4), track_time (4)
    file_data.extend_from_slice(&0_i32.to_le_bytes());
    file_data.extend_from_slice(&0_i32.to_le_bytes());
    file_data.extend_from_slice(&0.0_f32.to_le_bytes());
    // frame_count (4)
    file_data.extend_from_slice(&2_i32.to_le_bytes());
    // frame_offset (4) = 544
    file_data.extend_from_slice(&544_i32.to_le_bytes());
    // file_length (4) = 18
    file_data.extend_from_slice(&18_i32.to_le_bytes());

    std::fs::write(&input_path, &file_data).unwrap();

    let job = PatchJob {
        source_demo: input_path.to_string_lossy().to_string(),
        output_demo: output_path.clone(),
        streaks: vec![CaptureStreak {
            start_tick: 50,
            end_tick: 100,
            source_demo: input_path.to_string_lossy().to_string(),
            target_player: None,
            kill_count: 0,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: Vec::new(),
            start_index: 0,
            end_index: 0,
            total_demo_frames: 3000,
            demo_fps: 100.0,
            viewdemo_times: Vec::new(),
            frame_times: std::sync::Arc::new(Vec::new()),
            match_start_tick: None,
            source_key: None,
            spectated: false,
            steam_id: None,
            recorded_this_demo: false,
            status: Default::default(),
        }],
        target_player: None,
        init_commands: vec!["host_framerate 0".to_string()],
        scheduled_commands: vec![(10, "some_command".to_string())],
        director_events: Vec::new(),
        block_routes: Vec::new(),
        blocks: Vec::new(),
    };

    let config = PatcherConfig {
        pre_roll_ticks: 10,
        post_roll_ticks: 10,
        capture_fps: 300,
        exit_on_finish: true,
        init_commands: vec!["host_framerate 0".to_string()],
        custom_commands: Vec::new(),
        pre_roll_seconds: 2.0,
        post_roll_seconds: 0.6,
        record_start_lead: 0.0,
        record_stop_trail: 0.0,
        initial_delay: 3.0,
        ..PatcherConfig::default()
    };

    let patcher = StreamPatcher::new(&input_path, &output_path);
    let cancel_token = Arc::new(AtomicBool::new(false));
    let mut stages = Vec::new();
    patcher
        .patch_with_stages(&job, &config, &cancel_token, &mut |s| stages.push(s))
        .unwrap();

    // No blocks, so no decal pass; the percentages climb to exactly 100.
    assert_eq!(stages.first(), Some(&PatchStage::Writing { percent: 0 }));
    assert_eq!(stages.last(), Some(&PatchStage::Writing { percent: 100 }));
    assert!(!stages.contains(&PatchStage::ClearingDecals));
    let percents: Vec<u8> = stages
        .iter()
        .map(|s| match s {
            PatchStage::Writing { percent } => *percent,
            PatchStage::ClearingDecals => unreachable!(),
        })
        .collect();
    assert!(percents.windows(2).all(|w| w[0] < w[1]), "{:?}", percents);

    assert!(output_path.exists());
    let output_data = std::fs::read(&output_path).unwrap();
    assert!(output_data.len() > file_data.len());

    let _ = std::fs::remove_file(input_path);
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn test_stream_patcher_cancellation_and_cleanup() {
    let scratch_dir = std::path::Path::new("scratch");
    if !scratch_dir.exists() {
        let _ = std::fs::create_dir_all(scratch_dir);
    }

    let input_path = scratch_dir.join("test_cancel_input.dem");
    let output_path = scratch_dir.join("test_cancel_output.dem");

    let mut header = [0u8; 544];
    header[0..8].copy_from_slice(b"HLDEMO\0\0");
    let directory_offset: i32 = 562;
    header[540..544].copy_from_slice(&directory_offset.to_le_bytes());

    let mut file_data = Vec::new();
    file_data.extend_from_slice(&header);

    file_data.push(2);
    file_data.extend_from_slice(&0.0_f32.to_le_bytes());
    file_data.extend_from_slice(&0_i32.to_le_bytes());

    file_data.push(5);
    file_data.extend_from_slice(&0.0_f32.to_le_bytes());
    file_data.extend_from_slice(&0_i32.to_le_bytes());

    file_data.extend_from_slice(&1_i32.to_le_bytes());
    file_data.extend_from_slice(&1_i32.to_le_bytes());
    let mut desc = [0u8; 64];
    desc[..8].copy_from_slice(b"Playback");
    file_data.extend_from_slice(&desc);
    file_data.extend_from_slice(&0_i32.to_le_bytes());
    file_data.extend_from_slice(&0_i32.to_le_bytes());
    file_data.extend_from_slice(&0.0_f32.to_le_bytes());
    file_data.extend_from_slice(&2_i32.to_le_bytes());
    file_data.extend_from_slice(&544_i32.to_le_bytes());
    file_data.extend_from_slice(&18_i32.to_le_bytes());

    std::fs::write(&input_path, &file_data).unwrap();

    let job = PatchJob {
        source_demo: input_path.to_string_lossy().to_string(),
        output_demo: output_path.clone(),
        streaks: vec![CaptureStreak {
            start_tick: 50,
            end_tick: 100,
            source_demo: input_path.to_string_lossy().to_string(),
            target_player: None,
            kill_count: 0,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: Vec::new(),
            start_index: 0,
            end_index: 0,
            total_demo_frames: 3000,
            demo_fps: 100.0,
            viewdemo_times: Vec::new(),
            frame_times: std::sync::Arc::new(Vec::new()),
            match_start_tick: None,
            source_key: None,
            spectated: false,
            steam_id: None,
            recorded_this_demo: false,
            status: Default::default(),
        }],
        target_player: None,
        init_commands: vec!["host_framerate 0".to_string()],
        scheduled_commands: vec![(10, "some_command".to_string())],
        director_events: Vec::new(),
        block_routes: Vec::new(),
        blocks: Vec::new(),
    };

    let config = PatcherConfig {
        pre_roll_ticks: 10,
        post_roll_ticks: 10,
        capture_fps: 300,
        exit_on_finish: true,
        init_commands: vec!["host_framerate 0".to_string()],
        custom_commands: Vec::new(),
        pre_roll_seconds: 2.0,
        post_roll_seconds: 0.6,
        record_start_lead: 0.0,
        record_stop_trail: 0.0,
        initial_delay: 3.0,
        ..PatcherConfig::default()
    };

    // Create a token that starts out as cancelled (true)
    let cancel_token = Arc::new(AtomicBool::new(true));

    let patcher = StreamPatcher::new(&input_path, &output_path);
    let res = patcher.patch(&job, &config, &cancel_token);

    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::Interrupted);

    // Check spawn_patch_batch clean-up
    let cancel_token = Arc::new(AtomicBool::new(true));
    let worker = spawn_patch_batch(vec![job], config, cancel_token);
    let rx = worker.receiver;

    let mut got_cancelled = false;
    while let Ok(event) = rx.recv() {
        if let PatchEvent::Cancelled = event {
            got_cancelled = true;
        }
    }

    assert!(got_cancelled);
    assert!(!output_path.exists());

    let _ = std::fs::remove_file(input_path);
}
