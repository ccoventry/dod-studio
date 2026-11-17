// Panicking `unwrap()` / `expect()` stay out of non-test code; tests are allowed in clippy.toml (#1179).
#![warn(clippy::unwrap_used, clippy::expect_used)]
mod british;
/// The on-disk analysis cache Studio and the hook DLL share (#565).
#[cfg(not(target_arch = "wasm32"))]
pub mod cache;
/// Whether a demo carries a camera to play back (#920).
pub mod camera_data;
/// Who stood in a capture area, and the kill tags it gives (#1132).
mod capture_area;
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
mod lock;
/// Which match and which half a demo records, from its contents (#685).
mod match_group;
mod mortality;
mod objective;
/// Stretches an HLTV demo replays after a pause, counted once.
mod pause_replay;
mod player;
mod position;
mod pov_stats;
pub mod repair;
mod round;
mod scoreboard;
mod segment;
mod team_tag;
#[cfg(test)]
mod tests_fixture;
#[cfg(test)]
mod tests_old_server_build;
#[cfg(test)]
mod tests_sounds;
mod time;
#[cfg(not(target_arch = "wasm32"))]
mod utf16;
mod weapon_names;
mod wire_index;

use crate::wire_index::wire_slot;
use crate::{
    british::check_and_promote_british,
    capture_area::use_capture_area_updates,
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
use dem::types::{
    Demo, EngineMessage, Frame, FrameData, MessageData, MessageDataParseMode, NetMessage,
};
use dod::UserMessage;
use std::time::Duration;

pub(crate) use crate::demo_info::{is_hltv_demo, is_hltv_message};
#[cfg(not(target_arch = "wasm32"))]
pub use crate::localization::add_localization_search_path;
pub use crate::weapon_names::{all_weapon_display_names, weapon_display_name};
pub use crate::{
    capture_area::{
        AreaVisit, AttackerCount, Capping, CaptureAreas, FlagTag, KillAreaTags, flag_tags_of,
    },
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
    pause_replay::PauseReplay,
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
    /// Who stood in the capture areas over the demo (#1132).
    #[serde(default)]
    pub capture_areas: CaptureAreas,
    /// The stretches an HLTV demo replays after a match pause, oldest
    /// first. Their events are counted once, at their first showing.
    #[serde(default)]
    pub pause_replays: Vec<PauseReplay>,
    /// Whether the frame being read is inside one of `pause_replays`.
    #[serde(skip)]
    replaying: bool,
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

/// Whether an analysis decodes the demo's `svc_sound` messages. No analyzer
/// reads them, so both give the same [`Analysis`] (a test holds them equal),
/// and the analyzer cache serves either. Skipping them is the default parse
/// (#37); `Decode` stays for a probe that reads them. An analyzer that starts
/// reading sounds must switch the default to `Decode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sounds {
    Decode,
    Skip,
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
            | b"ClientAreas"
            | b"PlayersIn"
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
    if let AnalyzerEvent::Frame(_) = event {
        state.replaying = pause_replay::is_replayed(&state.pause_replays, state.frame_index + 1);
    }
    if state.replaying && !matches!(event, AnalyzerEvent::Finalization) {
        run_replayed_analyzers(state, event);
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
    // After positions (it tags the kill they pushed) and objectives (it
    // reads the finished cap breaks).
    use_capture_area_updates(state, event);
    use_chat_updates(state, event);
    use_clan_match_detection_updates(Duration::from_secs(30), state, event);
    use_pov_stats_updates(state, event);
    use_general_finalization(state, event);
    check_and_promote_british(state);
    // Last: pairing reads the victims' final teams.
    use_headshot_updates(state, event);
}

/// What a replayed stretch still feeds: the clock, which keeps the demo's
/// own times, and the entity snapshots, which later packets are deltas
/// against. Every user message is a repeat, and dropping them all leaves
/// the state as it was at the pause, which is where the match goes on.
/// Objectives see the frame only to settle what the pause's own frame left
/// pending.
fn run_replayed_analyzers(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    match event {
        AnalyzerEvent::Frame(_) => {
            use_timing_updates(state, event);
            use_position_updates(state, event);
            use_objective_updates(state, event);
        }
        AnalyzerEvent::EngineMessage(_) => {
            use_timing_updates(state, event);
            use_position_updates(state, event);
        }
        AnalyzerEvent::Initialization
        | AnalyzerEvent::Finalization
        | AnalyzerEvent::UserMessage(_) => {}
    }
}

/// The state an analysis of `demo` starts from: empty, but for the pause
/// replays found in it, which the frame loop needs before it reads them.
fn initial_state(demo: &Demo) -> AnalyzerState {
    AnalyzerState {
        pause_replays: pause_replay::find_replays(demo),
        ..Default::default()
    }
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
    /// is `(PROGRESS_SCALE, PROGRESS_SCALE)`. Skips the demo's sounds
    /// ([`Sounds::Skip`]).
    pub fn try_from_bytes_with_progress<F>(value: &[u8], progress_cb: F) -> Result<Self, String>
    where
        F: FnMut(usize, usize),
    {
        Self::try_from_bytes_decoding(value, Sounds::Skip, progress_cb)
    }

    /// [`Analysis::try_from_bytes_with_progress`], decoding the demo's
    /// sounds or not as `sounds` says (the same analysis either way).
    pub fn try_from_bytes_decoding<F>(
        value: &[u8],
        sounds: Sounds,
        mut progress_cb: F,
    ) -> Result<Self, String>
    where
        F: FnMut(usize, usize),
    {
        let mode = match sounds {
            Sounds::Decode => MessageDataParseMode::Parse,
            Sounds::Skip => MessageDataParseMode::ParseWithoutSounds,
        };
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
            dem::open_demo_from_bytes_tolerant_in_mode(value, mode, &mut |read, total| {
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

        let mut state = initial_state(&demo);
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
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lib_progress_tests.rs"]
mod progress_tests;
