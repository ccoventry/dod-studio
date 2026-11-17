use super::*;
use std::fs;

/// #351: DoD Studio's own injections into a POV demo's preview --
/// bookmarks (DRC_CMD_MESSAGE) and console commands (DRC_CMD_STUFFTEXT) --
/// don't make it HLTV; svc_hltv and a camera-moving director command do.
#[test]
fn only_a_camera_director_command_or_svc_hltv_means_hltv() {
    let director = |command: u8| {
        EngineMessage::SvcDirector(dem::types::SvcDirector {
            length: 2,
            command,
            message: vec![0],
        })
    };
    // Injected by the patcher (builder.rs's build_director_message and
    // build_director_stufftext).
    assert!(!is_hltv_message(&director(0x06)));
    assert!(!is_hltv_message(&director(0x0A)));
    // DRC_CMD_START, _EVENT, _MODE, _CAMERA: a real auto-director stream.
    for command in [0x01, 0x02, 0x03, 0x04] {
        assert!(is_hltv_message(&director(command)), "{command:#x}");
    }
    assert!(is_hltv_message(&EngineMessage::SvcHltv(
        dem::types::SvcHltv { mode: 0 }
    )));
    assert!(!is_hltv_message(&EngineMessage::SvcNop));
}

#[test]
fn is_relevant_message_admits_objective_messages() {
    for name in [
        &b"CapMsg"[..],
        b"InitObj",
        b"SetObj",
        b"StartProg",
        b"StartProgF",
        b"CancelProg",
    ] {
        assert!(is_relevant_message(name), "{:?} should be relevant", name);
    }
}

#[test]
fn is_relevant_message_rejects_unlisted_names() {
    assert!(!is_relevant_message(b"NotARealMessage"));
}

/// Demos for [`test_optimized_vs_unoptimized`]: every `.dem` in
/// `DOD_ANALYSIS_DEMOS` (a folder, or one file), else an even spread of up
/// to six from the PRE install's `dod` folder. Empty when neither exists.
pub(crate) fn comparison_demos() -> Vec<std::path::PathBuf> {
    let from_env = std::env::var_os("DOD_ANALYSIS_DEMOS").map(std::path::PathBuf::from);
    let source = from_env.clone().unwrap_or_else(|| {
        std::path::PathBuf::from(
            r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
        )
    });
    if source.is_file() {
        return vec![source];
    }
    let Ok(entries) = fs::read_dir(&source) else {
        return Vec::new();
    };
    let mut demos: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
        .collect();
    demos.sort();
    if from_env.is_some() || demos.len() <= 6 {
        return demos;
    }
    let step = demos.len() / 6;
    demos.into_iter().step_by(step).take(6).collect()
}

/// Analyses each demo twice -- the way [`Analysis::try_from_bytes`] does,
/// skipping user messages `is_relevant_message` rejects, and again feeding
/// every message through -- and asserts the two states match. Catches a
/// speed-up that skips something an analyzer needed. Ignored because CI has
/// no demos: `cargo test -p analysis -- --ignored test_optimized_vs_unoptimized`.
#[test]
#[ignore]
fn test_optimized_vs_unoptimized() {
    let paths = comparison_demos();
    assert!(
        !paths.is_empty(),
        "no demos found: set DOD_ANALYSIS_DEMOS to a folder or a .dem file"
    );

    for path in &paths {
        println!(
            "Testing optimized vs unoptimized parsing on: {}",
            path.display()
        );
        let file_bytes = fs::read(path).expect("failed to read demo");
        assert_optimized_matches_unoptimized(&file_bytes);
    }

    println!("All existing demos match perfectly!");
}

/// The comparison itself, also run on the checked-in fixture in CI
/// (`tests_fixture.rs`).
pub(crate) fn assert_optimized_matches_unoptimized(file_bytes: &[u8]) {
    // Unoptimized parse
    let demo = dem::open_demo_from_bytes(file_bytes).unwrap();
    let mut state_unopt = initial_state(&demo);
    let mut last_live_frame = None;
    let mut processed_frames = 0;
    let process_event = run_analyzers;
    process_event(&mut state_unopt, &AnalyzerEvent::Initialization);
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let old_live = matches!(
                state_unopt.clan_match_detection,
                ClanMatchDetection::MatchIsLive
            );
            process_event(&mut state_unopt, &AnalyzerEvent::Frame(frame));
            if let FrameData::NetworkMessage(box_type) = &frame.frame_data
                && let MessageData::Parsed(msgs) = &box_type.1.messages
            {
                for net_msg in msgs {
                    match net_msg {
                        NetMessage::EngineMessage(engine_msg) => {
                            process_event(
                                &mut state_unopt,
                                &AnalyzerEvent::EngineMessage(engine_msg),
                            );
                        }
                        NetMessage::UserMessage(user_msg) => {
                            if let Ok(msg) = UserMessage::new(&user_msg.name, &user_msg.data) {
                                process_event(&mut state_unopt, &AnalyzerEvent::UserMessage(msg));
                            }
                        }
                    }
                }
            }
            let new_live = matches!(
                state_unopt.clan_match_detection,
                ClanMatchDetection::MatchIsLive
            );
            if !old_live && new_live {
                last_live_frame = Some(processed_frames);
            }
            processed_frames += 1;
        }
    }
    process_event(&mut state_unopt, &AnalyzerEvent::Finalization);

    // Optimized parse
    let opt_analysis = Analysis::try_from_bytes(file_bytes).unwrap();
    let state_opt = opt_analysis.state;

    // Compare debug print representation
    let total_frames: usize = demo
        .directory
        .entries
        .iter()
        .map(|entry| entry.frames.len())
        .sum();
    println!(
        "  -> Total frames: {}, Live frame index: {:?} (Warmup frames skipped: {:?})",
        total_frames,
        last_live_frame,
        last_live_frame.unwrap_or(0)
    );

    assert_states_eq(&state_unopt, &state_opt);
}

fn assert_states_eq(left: &AnalyzerState, right: &AnalyzerState) {
    // Compare clan_match_detection
    assert_eq!(
        format!("{:?}", left.clan_match_detection),
        format!("{:?}", right.clan_match_detection)
    );

    // Compare current_time
    assert_eq!(
        left.current_time.real_offset,
        right.current_time.real_offset
    );
    assert_eq!(
        left.current_time.viewdemo_offset,
        right.current_time.viewdemo_offset
    );

    assert_eq!(left.match_start_witnessed, right.match_start_witnessed);
    assert_eq!(left.match_start_tick, right.match_start_tick);
    assert_eq!(left.started_late, right.started_late);
    assert_eq!(left.ended_early, right.ended_early);
    assert_eq!(left.first_time_left, right.first_time_left);
    assert_eq!(left.last_time_left, right.last_time_left);
    assert_eq!(left.map_changed, right.map_changed);
    assert_eq!(left.pause_replays, right.pause_replays);
    assert_eq!(left.initial_map_name, right.initial_map_name);

    // Compare team_scores
    assert_eq!(
        format!("{:?}", left.team_scores),
        format!("{:?}", right.team_scores)
    );

    // Compare POV stats
    assert_eq!(left.pov_player_index, right.pov_player_index);
    assert_eq!(left.pov_stats, right.pov_stats);

    // Compare rounds
    assert_eq!(left.rounds.len(), right.rounds.len());
    for (i, (r_l, r_r)) in left.rounds.iter().zip(right.rounds.iter()).enumerate() {
        assert_eq!(
            format!("{:?}", r_l),
            format!("{:?}", r_r),
            "Round {} mismatched",
            i
        );
    }

    // Compare players
    assert_eq!(left.players.len(), right.players.len());
    for (i, (p_l, p_r)) in left.players.iter().zip(right.players.iter()).enumerate() {
        assert_eq!(p_l.id, p_r.id, "Player {} ID mismatched", i);
        assert_eq!(
            format!("{:?}", p_l.connection),
            format!("{:?}", p_r.connection),
            "Player {} connection mismatched",
            i
        );
        assert_eq!(p_l.name, p_r.name, "Player {} name mismatched", i);
        assert_eq!(p_l.team, p_r.team, "Player {} team mismatched", i);
        assert_eq!(
            format!("{:?}", p_l.class),
            format!("{:?}", p_r.class),
            "Player {} class mismatched",
            i
        );
        assert_eq!(p_l.stats, p_r.stats, "Player {} stats mismatched", i);
        assert_eq!(
            p_l.stats_seeded, p_r.stats_seeded,
            "Player {} stats_seeded mismatched",
            i
        );
        assert_eq!(
            p_l.has_pre_demo_activity, p_r.has_pre_demo_activity,
            "Player {} has_pre_demo_activity mismatched",
            i
        );
        assert_eq!(
            p_l.has_reconnected, p_r.has_reconnected,
            "Player {} has_reconnected mismatched",
            i
        );

        // kill_streaks
        assert_eq!(
            p_l.kill_streaks.len(),
            p_r.kill_streaks.len(),
            "Player {} kill_streaks len mismatched",
            i
        );
        for (j, (k_l, k_r)) in p_l
            .kill_streaks
            .iter()
            .zip(p_r.kill_streaks.iter())
            .enumerate()
        {
            assert_eq!(
                format!("{:?}", k_l.kills),
                format!("{:?}", k_r.kills),
                "Player {} kill_streak {} mismatched",
                i,
                j
            );
            assert_eq!(
                k_l.slot, k_r.slot,
                "Player {} kill_streak {} slot mismatched",
                i, j
            );
        }

        // weapon_breakdown (HashMap == is order-independent)
        assert_eq!(
            p_l.weapon_breakdown, p_r.weapon_breakdown,
            "Player {} weapon_breakdown mismatched",
            i
        );

        // mortality
        assert_eq!(
            p_l.mortality.len(),
            p_r.mortality.len(),
            "Player {} mortality len mismatched",
            i
        );
        for (j, (m_l, m_r)) in p_l.mortality.iter().zip(p_r.mortality.iter()).enumerate() {
            assert_eq!(
                m_l.time().real_offset,
                m_r.time().real_offset,
                "Player {} mortality {} real_offset mismatched",
                i,
                j
            );
            assert_eq!(
                m_l.time().viewdemo_offset,
                m_r.time().viewdemo_offset,
                "Player {} mortality {} viewdemo_offset mismatched",
                i,
                j
            );
            assert_eq!(
                m_l.mortality(),
                m_r.mortality(),
                "Player {} mortality {} status mismatched",
                i,
                j
            );
        }
    }

    // Compare chat messages
    assert_eq!(
        left.chat_messages.len(),
        right.chat_messages.len(),
        "Chat messages length mismatched"
    );
    for (i, (c_l, c_r)) in left
        .chat_messages
        .iter()
        .zip(right.chat_messages.iter())
        .enumerate()
    {
        assert_eq!(
            c_l.chat_type, c_r.chat_type,
            "Chat message {} type mismatched",
            i
        );
        assert_eq!(
            c_l.sender_name, c_r.sender_name,
            "Chat message {} sender mismatched",
            i
        );
        assert_eq!(
            c_l.sender_team, c_r.sender_team,
            "Chat message {} team mismatched",
            i
        );
        assert_eq!(
            c_l.sender_dead, c_r.sender_dead,
            "Chat message {} dead flag mismatched",
            i
        );
        assert_eq!(c_l.text, c_r.text, "Chat message {} text mismatched", i);
        assert_eq!(
            c_l.system_token, c_r.system_token,
            "Chat message {} system_token mismatched",
            i
        );
        assert_eq!(
            c_l.system_args, c_r.system_args,
            "Chat message {} system_args mismatched",
            i
        );
    }
}

#[test]
fn test_reconnect_stats_accumulation() {
    let mut player = Player::new_mock(1, "STEAM_0:0:99999");

    // Session 1: Player gets 5 kills, 2 deaths, 10 score
    player.update_session_stats(10, 5, 2);
    assert_eq!(player.stats, (10, 5, 2));

    // Player disconnects
    player.needs_reconnect_sync = true;

    // Player reconnects. Server resets stats, so first update is 0 kills, 0 deaths, 0 score.
    player.update_session_stats(0, 0, 0);

    // Assert old session accumulated, active session reset to 0, total displayed stats are still 5 kills, 2 deaths, 10 score
    assert_eq!(player.accumulated_stats, (10, 5, 2));
    assert_eq!(player.session_stats, (0, 0, 0));
    assert_eq!(player.stats, (10, 5, 2));

    // Player gets a kill and a death in Session 2
    player.update_session_stats(12, 1, 1);
    assert_eq!(player.accumulated_stats, (10, 5, 2));
    assert_eq!(player.session_stats, (12, 1, 1));
    assert_eq!(player.stats, (22, 6, 3)); // 10+12 score, 5+1 kills, 2+1 deaths

    // Player disconnects and reconnects again. This time, server preserves/restores stats (e.g. they reconnect and have 1 kill, 1 death, 12 score).
    player.needs_reconnect_sync = true;
    player.update_session_stats(12, 1, 1);

    // Assert no double counting (accumulated stats should remain unchanged)
    assert_eq!(player.accumulated_stats, (10, 5, 2));
    assert_eq!(player.session_stats, (12, 1, 1));
    assert_eq!(player.stats, (22, 6, 3));
}

#[test]
fn test_partial_demo_and_pre_activity_detection() {
    let mut state = AnalyzerState::default();

    // Simulate start of demo
    use_general_finalization(&mut state, &AnalyzerEvent::Initialization);

    // Send TimeLeft message
    use_general_finalization(
        &mut state,
        &AnalyzerEvent::UserMessage(dod::UserMessage::TimeLeft(dod::TimeLeft(
            Duration::from_secs(900), // 15 mins
        ))),
    );

    // Seed a player with pre-demo activity (e.g. they already had 2 score, 1 kill, 1 death before demo started)
    let mut player = Player::new_mock(1, "Player 1");
    player.update_session_stats(2, 1, 1);
    state.players.push(player);

    // Send another TimeLeft message near the end
    use_general_finalization(
        &mut state,
        &AnalyzerEvent::UserMessage(dod::UserMessage::TimeLeft(dod::TimeLeft(
            Duration::from_secs(300), // 5 mins
        ))),
    );

    // Add a completed round (winner_stats is Some)
    state.rounds.push(Round::Completed {
        start_time: crate::time::GameTime::default(),
        end_time: crate::time::GameTime::default(),
        winner_stats: Some((Team::Allies, 1)),
    });

    // Add an active/incomplete round at the end
    state.rounds.push(Round::Completed {
        start_time: crate::time::GameTime::default(),
        end_time: crate::time::GameTime::default(),
        winner_stats: None,
    });

    // Simulate Finalization
    use_general_finalization(&mut state, &AnalyzerEvent::Finalization);

    assert_eq!(state.first_time_left, Some(Duration::from_secs(900)));
    assert_eq!(state.last_time_left, Some(Duration::from_secs(300)));
    assert!(state.started_late);
    assert!(state.ended_early);
    assert!(state.players[0].has_pre_demo_activity);
}

fn server_info(map: &str) -> EngineMessage {
    EngineMessage::SvcServerInfo(dem::types::SvcServerInfo {
        protocol: 48,
        spawn_count: 1,
        map_checksum: 0,
        client_dll_hash: dem::types::ByteString(vec![0; 16]),
        max_players: 32,
        player_index: 3,
        is_deathmatch: 0,
        game_dir: b"dod\0".to_vec(),
        hostname: b"server\0".to_vec(),
        map_file_name: format!("maps/{}.bsp\0", map).into_bytes(),
        map_cycle: Vec::new(),
        unknown: 0,
        legacy_trailer: None,
    })
}

fn state_with_a_kill() -> AnalyzerState {
    let mut state = AnalyzerState::default();
    use_segment_boundary(
        &mut state,
        &AnalyzerEvent::EngineMessage(&server_info("dod_lennon2")),
    );
    let mut player = Player::new_mock(3, "Player");
    player.update_session_stats(2, 1, 0);
    state.players.push(player);
    state
}

#[test]
fn a_same_map_signon_after_gameplay_ends_the_demo() {
    // #217: both halves on dod_lennon2 in one recording.
    let mut state = state_with_a_kill();
    use_segment_boundary(
        &mut state,
        &AnalyzerEvent::EngineMessage(&server_info("dod_lennon2")),
    );
    assert!(state.map_changed);
    assert_eq!(state.players.len(), 1);
}

#[test]
fn a_different_map_after_gameplay_ends_the_demo() {
    let mut state = state_with_a_kill();
    use_segment_boundary(
        &mut state,
        &AnalyzerEvent::EngineMessage(&server_info("dod_anzio")),
    );
    assert!(state.map_changed);
    assert_eq!(state.initial_map_name.as_deref(), Some("dod_lennon2"));
}

#[test]
fn a_different_map_before_gameplay_still_ends_the_demo() {
    // viewdemo stops at the first level change, warm-up or not, so the
    // analysis keeps the first map rather than skipping to the next.
    let mut state = AnalyzerState::default();
    use_segment_boundary(
        &mut state,
        &AnalyzerEvent::EngineMessage(&server_info("dod_warmup")),
    );
    state.players.push(Player::new_mock(3, "Player"));
    use_segment_boundary(
        &mut state,
        &AnalyzerEvent::EngineMessage(&server_info("dod_anzio")),
    );
    assert!(state.map_changed);
    assert_eq!(state.players.len(), 1);
    assert_eq!(state.initial_map_name.as_deref(), Some("dod_warmup"));
}

#[test]
fn a_same_map_signon_before_gameplay_ends_the_demo_too() {
    let mut state = AnalyzerState::default();
    use_segment_boundary(
        &mut state,
        &AnalyzerEvent::EngineMessage(&server_info("dod_anzio")),
    );
    state.players.push(Player::new_mock(3, "Player"));
    use_segment_boundary(
        &mut state,
        &AnalyzerEvent::EngineMessage(&server_info("dod_anzio")),
    );
    assert!(state.map_changed);
    assert_eq!(state.players.len(), 1);
}

#[test]
fn test_inspect_lenn_demo() {
    let mut path = "local/demos/ktps8w1-m00cat_soul_lenn_h2.dem";
    if !std::path::Path::new(path).exists() {
        path = "../local/demos/ktps8w1-m00cat_soul_lenn_h2.dem";
    }
    if std::path::Path::new(path).exists() {
        let file_bytes = fs::read(path).unwrap();
        let analysis = Analysis::try_from_bytes(&file_bytes).unwrap();
        println!("CLAN MATCH: map_name: {}", analysis.demo_info.map_name);
        println!(
            "CLAN MATCH: clan_match_detected: {}",
            analysis.state.clan_match_detected
        );
        println!(
            "CLAN MATCH: match_start_witnessed: {}",
            analysis.state.match_start_witnessed
        );
        println!("CLAN MATCH: started_late: {}", analysis.state.started_late);
        println!("CLAN MATCH: ended_early: {}", analysis.state.ended_early);
        println!(
            "CLAN MATCH: first_time_left: {:?}",
            analysis.state.first_time_left
        );
        println!(
            "CLAN MATCH: last_time_left: {:?}",
            analysis.state.last_time_left
        );
        println!("CLAN MATCH: map_changed: {}", analysis.state.map_changed);
        println!(
            "CLAN MATCH: initial_map_name: {:?}",
            analysis.state.initial_map_name
        );
        for p in &analysis.state.players {
            if p.has_pre_demo_activity {
                println!(
                    "CLAN MATCH: Player {} has pre-demo activity! Stats: {:?}",
                    p.name, p.stats
                );
            }
        }
    } else {
        // A developer inspection aid, not a regression test. The fixture is
        // not committed, so skip rather than fail the suite on a fresh clone.
        eprintln!("skipping test_inspect_lenn_demo: {path} not present");
    }
}

#[test]
fn test_pub_demo_natural_end_detection() {
    let mut state = AnalyzerState::default();
    state.clan_match_detected = false;

    // 1. Without rounds and map change/timeleft, duration is 0, which is NOT close to match end.
    // It has no rounds, so ended_early defaults to false.
    use_general_finalization(&mut state, &AnalyzerEvent::Finalization);
    assert!(!state.ended_early);

    // 2. Add an incomplete round, making it not natural end.
    let mut state = AnalyzerState::default();
    state.clan_match_detected = false;
    state.rounds.push(Round::Completed {
        start_time: crate::time::GameTime {
            real_offset: Duration::ZERO,
            ..Default::default()
        },
        end_time: crate::time::GameTime {
            real_offset: Duration::ZERO,
            ..Default::default()
        },
        winner_stats: None,
    });
    use_general_finalization(&mut state, &AnalyzerEvent::Finalization);
    assert!(state.ended_early); // Ended early because not close to match end, no map change, and last round is incomplete

    // 3. Make duration close to standard map end (e.g., 20 mins = 1200s, let's use 1195s)
    let mut state = AnalyzerState::default();
    state.clan_match_detected = false;
    state.rounds.push(Round::Completed {
        start_time: crate::time::GameTime {
            real_offset: Duration::ZERO,
            ..Default::default()
        },
        end_time: crate::time::GameTime {
            real_offset: Duration::ZERO,
            ..Default::default()
        },
        winner_stats: None,
    });
    state.current_time.real_offset = Duration::from_secs(1195);
    use_general_finalization(&mut state, &AnalyzerEvent::Finalization);
    assert!(!state.ended_early); // Not ended early because duration is close to 1200s (natural end)
}

#[test]
fn test_stealth_partial_demo() {
    let mut path = "local/demos/ktps8w8-stealth_ih_saints_h1_p2.dem";
    if !std::path::Path::new(path).exists() {
        path = "../local/demos/ktps8w8-stealth_ih_saints_h1_p2.dem";
    }
    if std::path::Path::new(path).exists() {
        let file_bytes = fs::read(path).unwrap();
        let analysis = Analysis::try_from_bytes(&file_bytes).unwrap();
        assert!(analysis.state.clan_match_detected);
        assert!(!analysis.state.match_start_witnessed);
        assert!(analysis.state.started_late);
        assert!(analysis.state.ended_early);
    }
}
