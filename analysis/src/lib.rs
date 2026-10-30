mod british;
/// The on-disk analysis cache Studio and the hook DLL share (#565).
#[cfg(not(target_arch = "wasm32"))]
pub mod cache;
/// Whether a demo carries a camera to play back (#920).
pub mod camera_data;
mod chat;
mod clan_match;
/// The clock offset between two recordings of one half (#708).
mod clock_offset;
mod demo_info;
pub mod entity_replay;
mod finalization;
/// The 64-bit FNV-1a every crate hashes demo keys and file names with.
pub mod fnv1a;
mod headshot;
/// What a highlight is, and whose, for every list of them (#573).
pub mod highlights;
/// Whether every delta decodes against a frame the file has (#41).
pub mod integrity;
mod kill;
mod localization;
/// Which match and which half a demo records, from its contents (#685).
mod match_group;
mod mortality;
mod objective;
mod player;
mod position;
mod pov_stats;
mod round;
mod scoreboard;
mod segment;
mod team_tag;
#[cfg(test)]
mod tests_fixture;
mod time;
#[cfg(not(target_arch = "wasm32"))]
mod utf16;
mod weapon_names;
mod wire_index;

use crate::wire_index::wire_slot;
use crate::{
    british::check_and_promote_british,
    chat::use_chat_updates,
    clan_match::{ClanMatchDetection, use_clan_match_detection_updates},
    finalization::use_general_finalization,
    headshot::use_headshot_updates,
    kill::{
        use_kill_streak_updates, use_teamkill_and_suicide_updates, use_weapon_breakdown_updates,
    },
    mortality::with_mortality_detection,
    objective::use_objective_updates,
    player::use_player_updates,
    position::use_position_updates,
    pov_stats::use_pov_stats_updates,
    round::use_rounds_updates,
    scoreboard::{TeamScores, use_scoreboard_updates, use_team_score_updates},
    segment::{use_segment_boundary, use_signon_maps},
    time::{GameTime, use_timing_updates},
};
use dem::types::{Demo, EngineMessage, Frame, FrameData, MessageData, NetMessage};
use dod::UserMessage;
use std::time::Duration;

pub(crate) use crate::demo_info::{is_hltv_demo, is_hltv_message};
#[cfg(not(target_arch = "wasm32"))]
pub use crate::localization::add_localization_search_path;
pub use crate::weapon_names::{all_weapon_display_names, weapon_display_name};
pub use crate::{
    chat::{ChatMessage, ChatType, translate_system_message},
    clock_offset::{ClockOffset, MIN_PAIRS, is_same_half, kill_clock_offset},
    headshot::{Headshot, headshot_of},
    localization::{get_active_language, set_active_language, translate_key},
    match_group::{
        Half, HalfCall, HalfReason, MatchGroup, MatchHalf, MatchTeam, Side, find_demo,
        group_matches, half_of, trim_for_grouping,
    },
    mortality::{Mortality, MortalityChange, MortalityState},
    objective::{
        AttemptOutcome, BREAK_WINDOW, CancelCause, CaptureAttempt, Flag, FlagCapture, Objectives,
        cancel_cause,
    },
    player::{Connection, Player, PlayerGlobalId, SteamId, steam_id_of},
    position::{KillPosition, PlayerPose},
    round::Round,
    team_tag::{TeamTag, detect_team_tag, team_tags},
};
pub use crate::{
    demo_info::{DemoDamage, DemoInfo},
    finalization::use_time_left_updates,
    pov_stats::{PovStats, WeaponPovStats},
};
pub use dod::{Team, Weapon};

#[derive(Debug)]
pub enum AnalyzerEvent<'a> {
    Initialization,
    Finalization,

    Frame(&'a Frame),
    EngineMessage(&'a EngineMessage),
    UserMessage(UserMessage),
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AnalyzerState {
    clan_match_detection: ClanMatchDetection,
    /// Set to `true` as soon as any definitive clan-match signal is observed
    /// (ClanTimer message, WaveTime > 0, or the Reset→Start scoreboard zeroing
    /// sequence). Unlike `clan_match_detection`, this flag is never cleared, so
    /// demos recorded *after* the match went live still report correctly.
    pub clan_match_detected: bool,
    pub match_start_witnessed: bool,
    pub match_start_tick: Option<i32>,
    pub started_late: bool,
    pub ended_early: bool,
    pub first_time_left: Option<std::time::Duration>,
    pub last_time_left: Option<std::time::Duration>,
    /// The demo kept recording through a level change, to another map or
    /// the same one; everything from there on is ignored, as the game's
    /// `viewdemo` stops there too (`use_segment_boundary`).
    pub map_changed: bool,
    /// The map of every signon in the demo, in order, including the ones the
    /// analysis ignores: two or more is a demo that can be split into one per
    /// map (#624). Defaulted so an older cache entry still loads; empty there
    /// means "not recorded", not "no maps".
    #[serde(default)]
    pub signon_maps: Vec<String>,
    pub initial_map_name: Option<String>,
    pub current_time: GameTime,

    pub frame_index: usize,
    pub players: Vec<Player>,
    pub rounds: Vec<Round>,
    pub team_scores: TeamScores,
    pub chat_messages: Vec<ChatMessage>,
    pub pov_player_index: Option<u8>,
    pub pov_stats: PovStats,
    pub hltv_name: Option<String>,
    pub allies_are_british: bool,
    pub server_name: Option<String>,
    pub server_address: Option<String>,
    /// Every kill, with where both players stood (#448). Defaulted so a
    /// cache entry written before it existed still loads.
    #[serde(default)]
    pub kill_positions: Vec<KillPosition>,
    /// Working state for `kill_positions`; never serialized.
    #[serde(skip)]
    positions: position::PositionTracker,
    /// Working state for `KillPosition::headshot`; never serialized.
    #[serde(skip)]
    headshots: headshot::HeadshotTracker,
    /// Flag layout, ownership, captures and capture attempts (#192).
    #[serde(default)]
    pub objectives: Objectives,
}

/// What [`Analysis::try_from_bytes_with_progress`] reports progress out of.
pub const PROGRESS_SCALE: usize = 1000;
/// The part of [`PROGRESS_SCALE`] reading the demo's structure takes.
pub const PARSE_SHARE: usize = 850;

/// `done` of `total`, placed between `from` and `to` on [`PROGRESS_SCALE`].
/// In u128: a byte count times 850 passes 2^32 in the 32-bit game DLL.
fn scaled(done: usize, total: usize, from: usize, to: usize) -> usize {
    if total == 0 {
        return to;
    }
    let done = (done as u128).min(total as u128);
    from + (done * (to - from) as u128 / total as u128) as usize
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Analysis {
    pub demo_info: DemoInfo,
    pub state: AnalyzerState,
}

fn is_relevant_message(name_bytes: &[u8]) -> bool {
    let mut len = name_bytes.len();
    while len > 0 && name_bytes[len - 1] == 0 {
        len -= 1;
    }
    let trimmed = &name_bytes[..len];

    matches!(
        trimmed,
        b"RoundState"
            | b"ClanTimer"
            | b"TimeLeft"
            | b"WaveTime"
            | b"TeamScore"
            | b"ScoreShort"
            | b"ObjScore"
            | b"Frags"
            | b"PClass"
            | b"PTeam"
            | b"ScoreInfo"
            | b"ScoreInfoLong"
            | b"SayText"
            | b"TextMsg"
            | b"DeathMsg"
            | b"PStatus"
            | b"Scope"
            | b"CurWeapon"
            | b"ReloadDone"
            | b"ResetHUD"
            | b"Health"
            | b"CapMsg"
            | b"InitObj"
            | b"SetObj"
            | b"StartProg"
            | b"StartProgF"
            | b"CancelProg"
    )
}

/// Feeds one event through every analyzer, in order. Shared with the
/// optimised-vs-unoptimised test so its reference pass can't drift from this.
fn run_analyzers(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    use_signon_maps(state, event);
    if !state.map_changed {
        use_segment_boundary(state, event);
    }
    if state.map_changed && !matches!(event, AnalyzerEvent::Finalization) {
        return;
    }
    use_timing_updates(state, event);
    use_player_updates(state, event);
    use_position_updates(state, event);
    with_mortality_detection(state, event);
    use_scoreboard_updates(state, event);
    use_kill_streak_updates(state, event);
    use_weapon_breakdown_updates(state, event);
    use_teamkill_and_suicide_updates(state, event);
    use_team_score_updates(state, event);
    use_rounds_updates(state, event);
    use_objective_updates(state, event);
    use_chat_updates(state, event);
    use_clan_match_detection_updates(Duration::from_secs(30), state, event);
    use_pov_stats_updates(state, event);
    use_general_finalization(state, event);
    check_and_promote_british(state);
    // Last: pairing reads the victims' final teams.
    use_headshot_updates(state, event);
}

impl Analysis {
    fn new(demo_info: DemoInfo, state: AnalyzerState) -> Self {
        Self { demo_info, state }
    }

    pub fn try_from_bytes(value: &[u8]) -> Result<Self, String> {
        Self::try_from_bytes_with_progress(value, |_, _| {})
    }

    /// Analyses a demo, calling `progress_cb(done, total)` as it goes. Only
    /// the ratio means anything: reading the demo's structure is the first
    /// [`PARSE_SHARE`] of [`PROGRESS_SCALE`] (it is ~85% of the time, measured
    /// on a 72 MB demo), going through its frames the rest, and the last call
    /// is `(PROGRESS_SCALE, PROGRESS_SCALE)`.
    pub fn try_from_bytes_with_progress<F>(value: &[u8], mut progress_cb: F) -> Result<Self, String>
    where
        F: FnMut(usize, usize),
    {
        // This only catches anything in a build that unwinds -- tests and
        // `cargo run` without `--release`. The release profile sets
        // `panic = "abort"`, so a panic inside the parser aborts the process
        // and never reaches here. It is kept because it costs nothing and does
        // work under test, but the actual defence is that `dem-patch` does not
        // panic on malformed input: bounds live at the read sites, and
        // `BitReader::is_bad_read` turns an overrun into a parse error. See
        // #225, and `dem-patch/examples/mangle_probe.rs` for the harness that
        // proves it.
        //
        // A demo damaged partway (#811) is read up to the frame that breaks,
        // and the analysis says where that is (`DemoInfo::damage`).
        let demo_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dem::open_demo_from_bytes_tolerant_with_progress(value, &mut |read, total| {
                progress_cb(scaled(read, total, 0, PARSE_SHARE), PROGRESS_SCALE)
            })
        }));
        let (demo, damage) = match demo_res {
            Ok(Ok(d)) => d,
            Ok(Err(e)) => {
                return Err(format!(
                    "Parse error: {}",
                    integrity::parse_error_reason(&e.to_string())
                ));
            }
            Err(_) => return Err("Parser panicked during demo structural decoding".to_string()),
        };

        let mut state = AnalyzerState::default();
        let process_event = run_analyzers;

        process_event(&mut state, &AnalyzerEvent::Initialization);

        let total_frames: usize = demo
            .directory
            .entries
            .iter()
            .map(|entry| entry.frames.len())
            .sum();
        let mut processed_frames = 0;

        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                process_event(&mut state, &AnalyzerEvent::Frame(frame));
                if let FrameData::NetworkMessage(box_type) = &frame.frame_data
                    && let MessageData::Parsed(msgs) = &box_type.1.messages
                {
                    for net_msg in msgs {
                        match net_msg {
                            NetMessage::EngineMessage(engine_msg) => {
                                process_event(
                                    &mut state,
                                    &AnalyzerEvent::EngineMessage(engine_msg),
                                );
                            }
                            NetMessage::UserMessage(user_msg) => {
                                if is_relevant_message(user_msg.name.as_ref())
                                    && let Ok(msg) =
                                        UserMessage::new(&user_msg.name, &user_msg.data)
                                {
                                    process_event(&mut state, &AnalyzerEvent::UserMessage(msg));
                                }
                            }
                        }
                    }
                }

                processed_frames += 1;
                if processed_frames % 500 == 0 || processed_frames == total_frames {
                    progress_cb(
                        scaled(processed_frames, total_frames, PARSE_SHARE, PROGRESS_SCALE),
                        PROGRESS_SCALE,
                    );
                }
            }
        }

        process_event(&mut state, &AnalyzerEvent::Finalization);

        let mut info = DemoInfo::from(&demo);
        info.damage = damage.map(|d| DemoDamage::new(&demo, d));
        release_frames(demo);
        Ok(Analysis::new(info, state))
    }
}

/// Frees the decoded frame tree. That's about a third of a cold parse (441 ms
/// of ~1.3 s, `docs/demo_analyzer_load_performance.md`) and nothing needs the
/// tree once the analysis is built, so where there are threads the caller
/// doesn't wait for it. The browser build (wasm) frees it in place.
fn release_frames(demo: Demo) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut demo = demo;
        let entries = std::mem::take(&mut demo.directory.entries);
        std::thread::spawn(move || drop(entries));
    }
    #[cfg(target_arch = "wasm32")]
    drop(demo);
}

impl AnalyzerState {
    pub fn is_clan_match(&self) -> bool {
        self.clan_match_detected
    }

    fn find_player_by_client_index(&self, client_index: u8) -> Option<&Player> {
        self.players.iter().find(|player| match player.connection {
            Connection::Connected { client_id } => client_id == client_index,
            _ => false,
        })
    }

    fn find_player_by_client_index_mut(&mut self, client_index: u8) -> Option<&mut Player> {
        self.players
            .iter_mut()
            .find(|player| match player.connection {
                Connection::Connected { client_id } => client_id == client_index,
                _ => false,
            })
    }

    /// Looks a player up by the 1-based client index a message carries.
    fn find_player_by_wire_index(&self, client_index: u8) -> Option<&Player> {
        self.find_player_by_client_index(wire_slot(client_index)?)
    }

    fn find_player_by_wire_index_mut(&mut self, client_index: u8) -> Option<&mut Player> {
        self.find_player_by_client_index_mut(wire_slot(client_index)?)
    }

    fn find_player_by_id(&self, id: &PlayerGlobalId) -> Option<&Player> {
        self.players.iter().find(|player| player.id == *id)
    }

    fn find_player_by_id_mut(&mut self, id: &PlayerGlobalId) -> Option<&mut Player> {
        self.players.iter_mut().find(|player| player.id == *id)
    }
}

#[cfg(test)]
mod tests {
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
        let mut state_unopt = AnalyzerState::default();
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
                                    process_event(
                                        &mut state_unopt,
                                        &AnalyzerEvent::UserMessage(msg),
                                    );
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
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn progress_is_placed_on_one_scale() {
        assert_eq!(scaled(0, 100, 0, PARSE_SHARE), 0);
        assert_eq!(scaled(100, 100, 0, PARSE_SHARE), PARSE_SHARE);
        assert_eq!(scaled(50, 100, PARSE_SHARE, PROGRESS_SCALE), 925);
        assert_eq!(scaled(5, 0, PARSE_SHARE, PROGRESS_SCALE), PROGRESS_SCALE);
        // A 4 GB file's worth of bytes still scales without overflow.
        assert_eq!(scaled(usize::MAX / 2, usize::MAX, 0, 1000), 499);
    }
}
