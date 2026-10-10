use super::*;
use std::collections::HashMap;

#[test]
fn test_build_batch_queue_grouping() {
    let mut config = PatcherConfig::default();
    let temp_game_path = std::env::temp_dir().join("dod_test_mock");
    std::fs::create_dir_all(temp_game_path.join("dod")).expect("Failed to create dummy dod dir");
    config.game_path = temp_game_path.to_string_lossy().to_string();
    config.primary_media_dir = Some(temp_game_path.clone());

    let streak1 = CaptureStreak {
        start_tick: 100,
        end_tick: 200,
        source_demo: "demo_A.dem".to_string(),
        target_player: Some("Player1".to_string()),
        kill_count: 2,
        timeline_string: String::new(),
        duration_string: String::new(),
        player_index: 1,
        kills: vec![],
        start_index: 0,
        end_index: 0,
        total_demo_frames: 1000,
        demo_fps: 100.0,
        viewdemo_times: vec![],
        frame_times: Arc::new(vec![]),
        status: crate::patch::types::HighlightStatus::None,
        match_start_tick: None,
        source_key: None,
    };

    let streak2 = CaptureStreak {
        start_tick: 300,
        end_tick: 400,
        source_demo: "demo_A.dem".to_string(),
        target_player: Some("Player1".to_string()),
        kill_count: 2,
        timeline_string: String::new(),
        duration_string: String::new(),
        player_index: 1,
        kills: vec![],
        start_index: 0,
        end_index: 0,
        total_demo_frames: 1000,
        demo_fps: 100.0,
        viewdemo_times: vec![],
        frame_times: Arc::new(vec![]),
        status: crate::patch::types::HighlightStatus::None,
        match_start_tick: None,
        source_key: None,
    };

    let streak3 = CaptureStreak {
        start_tick: 100,
        end_tick: 200,
        source_demo: "demo_B.dem".to_string(),
        target_player: Some("Player2".to_string()),
        kill_count: 2,
        timeline_string: String::new(),
        duration_string: String::new(),
        player_index: 2,
        kills: vec![],
        start_index: 0,
        end_index: 0,
        total_demo_frames: 1000,
        demo_fps: 100.0,
        viewdemo_times: vec![],
        frame_times: Arc::new(vec![]),
        status: crate::patch::types::HighlightStatus::None,
        match_start_tick: None,
        source_key: None,
    };

    let raw_streaks = vec![streak1, streak2, streak3];
    let global_arrays = HashMap::new();

    let (jobs, _) = build_batch_queue(raw_streaks, &config, &global_arrays).unwrap();

    assert_eq!(
        jobs.len(),
        3,
        "Expected exactly 3 patch jobs (1 primer + 2 grouped chains) after grouping by source demo and player"
    );
}
