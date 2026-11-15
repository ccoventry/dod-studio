//! Who stood in a capture area, and what that says about each kill (#1132).
//!
//! Two messages carry it, with the meanings `dod.so` and `client.so` give
//! them (R&D PR #1170, findings on #1132):
//!
//! - `ClientAreas` goes to one player (`MSG_ONE`). While a player touches an
//!   area their team can capture the server sends status 1 on every touch,
//!   and 0 when they leave. A POV demo carries it for its recorder alone; an
//!   HLTV demo carries none of it. That gives [`CaptureAreas::recorder_visits`].
//! - `PlayersIn` goes to everyone: how many of the team that does not own a
//!   point stand in its area, uncontested. Only for areas that need two or
//!   more players, and a count, never names. That gives
//!   [`CaptureAreas::attackers`], per team, in both demo types.
//!
//! The kill tags ([`KillAreaTags`]) take only what the demo shows:
//!
//! - **capping**, for the killer and for the victim: known for the recorder
//!   of a POV demo, from `ClientAreas`; [`Capping::Unknown`] for everyone
//!   else, and in HLTV demos for everyone. A `PlayersIn` count dropping after
//!   a death is not used to guess it: checked against the recorder's own
//!   `ClientAreas`, a drop followed 8 of 15 deaths in an area and also 4
//!   deaths outside one (9 POV demos, 9 players).
//! - **cleared the flag**: the kill that broke a timed capture
//!   ([`objective::breaking_kill`](crate::objective), the league's cap break),
//!   in both demo types.

use crate::objective::{AttemptOutcome, breaking_kill, kills_known_at};
use crate::{AnalyzerEvent, AnalyzerState, is_hltv_message, time::GameTime};
use dod::{ClientAreaStatus, Team, UserMessage};
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

/// How long after its last status-1 `ClientAreas` an area still counts as
/// stood in. The server repeats it every touch: on 9 POV demos (9 players)
/// the gaps were under 0.08 s 99% of the time and never over 0.33 s, so a
/// visit that went quiet for longer ended without its leave message.
const STALE_AFTER: Duration = Duration::from_millis(500);

/// Whether a player stood in a capture area their team can capture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Capping {
    Yes,
    No,
    /// The demo does not say: anyone but a POV demo's recorder.
    #[default]
    Unknown,
}

/// What the capture areas say about one kill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KillAreaTags {
    /// The killer stood in an area their team can capture. `Unknown` for a
    /// kill by the world or the victim themself.
    pub killer_capping: Capping,
    /// The victim did: "killed while capping", and for the killer a flag
    /// defence.
    pub victim_capping: Capping,
    /// This kill broke a timed capture.
    pub cleared_flag: bool,
}

/// A tag a highlight shows on one of its kills, from the killer's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagTag {
    /// The killer was capping.
    WhileCapping,
    /// The victim was capping.
    FlagDefence,
    /// The kill broke a timed capture.
    ClearedFlag,
}

impl KillAreaTags {
    /// The tags a highlight shows for this kill, in a fixed order.
    pub fn flag_tags(&self) -> Vec<FlagTag> {
        [
            (self.killer_capping == Capping::Yes, FlagTag::WhileCapping),
            (self.victim_capping == Capping::Yes, FlagTag::FlagDefence),
            (self.cleared_flag, FlagTag::ClearedFlag),
        ]
        .into_iter()
        .filter_map(|(on, tag)| on.then_some(tag))
        .collect()
    }
}

/// One stay of a POV demo's recorder in a capture area.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AreaVisit {
    /// `ClientAreas`' area index.
    pub area: u8,
    pub entered: GameTime,
    /// `None` while still in it when the demo ended.
    pub left: Option<GameTime>,
}

/// A change in how many attackers stand in a point's area (`PlayersIn`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttackerCount {
    pub time: GameTime,
    /// `PlayersIn`' point index.
    pub point: u8,
    /// [Team::Unassigned] with a count of 0: nobody, both teams, or only the
    /// owners stand in it.
    pub team: Team,
    pub count: u8,
    /// Players the area needs to be captured.
    pub required: u8,
}

/// Capture-area occupancy over the demo.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CaptureAreas {
    /// The recorder's visits, in order. Empty in an HLTV demo.
    pub recorder_visits: Vec<AreaVisit>,
    /// Every change of a point's count, in order.
    pub attackers: Vec<AttackerCount>,
    #[serde(skip)]
    tracker: Tracker,
}

/// Working state; never serialized.
#[derive(Debug, Default)]
struct Tracker {
    hltv: bool,
    /// A `ClientAreas` arrived: the server sends the recorder the areas'
    /// icons on joining, so without one the demo can't say "not capping".
    areas_known: bool,
    /// Per open visit (its index in `recorder_visits`): the last status 1.
    open: HashMap<u8, (usize, GameTime)>,
    /// Areas the recorder died in. The touch can go on for up to a second
    /// before the leave message (measured 0.3 to 1.0 s), and the dead stand
    /// nowhere: status 1 for these is ignored until the leave.
    died_in: BTreeSet<u8>,
    /// The last count per point, so repeats are not stored.
    counts: HashMap<u8, (Team, u8, u8)>,
}

impl CaptureAreas {
    fn on_client_areas(&mut self, area: u8, status: ClientAreaStatus, now: &GameTime) {
        self.tracker.areas_known = true;
        match status {
            ClientAreaStatus::Inside if !self.tracker.died_in.contains(&area) => {
                self.close_stale(now);
                match self.tracker.open.get_mut(&area) {
                    Some((_, seen)) => *seen = now.clone(),
                    None => {
                        self.tracker
                            .open
                            .insert(area, (self.recorder_visits.len(), now.clone()));
                        self.recorder_visits.push(AreaVisit {
                            area,
                            entered: now.clone(),
                            left: None,
                        });
                    }
                }
            }
            ClientAreaStatus::Left => {
                self.tracker.died_in.remove(&area);
                if let Some((index, _)) = self.tracker.open.remove(&area) {
                    self.end_visit(index, now.clone());
                }
            }
            ClientAreaStatus::ClearAll => {
                self.tracker.died_in.clear();
                self.close_all(now);
            }
            _ => {}
        }
    }

    /// Ends every open visit that went quiet for longer than [STALE_AFTER],
    /// at its last status 1.
    fn close_stale(&mut self, now: &GameTime) {
        let stale: Vec<u8> = self
            .tracker
            .open
            .iter()
            .filter(|(_, (_, seen))| {
                now.viewdemo_offset.saturating_sub(seen.viewdemo_offset) > STALE_AFTER
            })
            .map(|(area, _)| *area)
            .collect();
        for area in stale {
            if let Some((index, seen)) = self.tracker.open.remove(&area) {
                self.end_visit(index, seen);
            }
        }
    }

    fn close_all(&mut self, now: &GameTime) {
        for (_, (index, _)) in std::mem::take(&mut self.tracker.open) {
            self.end_visit(index, now.clone());
        }
    }

    fn end_visit(&mut self, index: usize, at: GameTime) {
        if let Some(visit) = self.recorder_visits.get_mut(index) {
            visit.left = Some(at);
        }
    }

    /// Whether the recorder is capping right now, as far as the demo says.
    fn recorder_capping(&mut self, now: &GameTime) -> Capping {
        if self.tracker.hltv || !self.tracker.areas_known {
            return Capping::Unknown;
        }
        self.close_stale(now);
        if self.tracker.open.is_empty() {
            Capping::No
        } else {
            Capping::Yes
        }
    }

    /// The recorder died: their visits end now.
    fn recorder_died(&mut self, now: &GameTime) {
        self.tracker
            .died_in
            .extend(self.tracker.open.keys().copied());
        self.close_all(now);
    }

    fn on_players_in(&mut self, message: &dod::PlayersIn, now: &GameTime) {
        let count = (
            message.team.clone(),
            message.players_inside_area,
            message.required_players_to_capture,
        );
        if self.tracker.counts.get(&message.area_index) == Some(&count) {
            return;
        }
        self.tracker.counts.insert(message.area_index, count);
        self.attackers.push(AttackerCount {
            time: now.clone(),
            point: message.area_index,
            team: message.team.clone(),
            count: message.players_inside_area,
            required: message.required_players_to_capture,
        });
    }
}

/// Runs after `use_position_updates` (it tags the kill that pushed) and
/// `use_objective_updates` (it reads the finished cap breaks).
pub fn use_capture_area_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    match event {
        AnalyzerEvent::EngineMessage(message) if is_hltv_message(message) => {
            state.capture_areas.tracker.hltv = true;
        }
        AnalyzerEvent::UserMessage(UserMessage::ClientAreas(message)) => {
            let now = state.current_time.clone();
            state
                .capture_areas
                .on_client_areas(message.icon_index, message.status, &now);
        }
        AnalyzerEvent::UserMessage(UserMessage::PlayersIn(message)) => {
            let now = state.current_time.clone();
            state.capture_areas.on_players_in(message, &now);
        }
        AnalyzerEvent::UserMessage(UserMessage::DeathMsg(death)) => {
            // `use_position_updates` pushed a kill for this message.
            if death.victim_client_index == 0 {
                return;
            }
            let now = state.current_time.clone();
            let recorder = state.pov_player_index.and_then(|slot| slot.checked_add(1));
            let by_other = death.killer_client_index != 0
                && death.killer_client_index != death.victim_client_index;
            let mut tags = KillAreaTags::default();
            if Some(death.killer_client_index) == recorder && by_other {
                tags.killer_capping = state.capture_areas.recorder_capping(&now);
            }
            if Some(death.victim_client_index) == recorder {
                tags.victim_capping = state.capture_areas.recorder_capping(&now);
                state.capture_areas.recorder_died(&now);
            }
            if let Some(kill) = state.kill_positions.last_mut() {
                kill.capture_area = tags;
            }
        }
        AnalyzerEvent::Finalization => mark_cleared_flags(state),
        _ => {}
    }
}

/// Marks the kill that broke each timed capture, found the way the cap
/// break was credited (`cancel_cause`, over the kills known by then).
fn mark_cleared_flags(state: &mut AnalyzerState) {
    let breaking: Vec<usize> = state
        .objectives
        .attempts
        .iter()
        .filter(|a| a.outcome == AttemptOutcome::Cancelled)
        .filter_map(|attempt| {
            let ended = attempt.ended.as_ref()?;
            breaking_kill(attempt, ended, kills_known_at(&state.kill_positions, ended))
        })
        .collect();
    for index in breaking {
        if let Some(kill) = state.kill_positions.get_mut(index) {
            kill.capture_area.cleared_flag = true;
        }
    }
}

/// The tags recorded for the kill of `victim` at `time`, none when the demo
/// has no such kill. `kills` is `kill_positions`, in time order.
pub fn flag_tags_of(
    kills: &[crate::KillPosition],
    time: &GameTime,
    victim: &crate::PlayerGlobalId,
) -> Vec<FlagTag> {
    let first = kills.partition_point(|k| k.time.frame_index < time.frame_index);
    kills[first..]
        .iter()
        .take_while(|k| k.time.frame_index == time.frame_index)
        .find(|k| k.victim.as_ref() == Some(victim))
        .map_or_else(Vec::new, |k| k.capture_area.flag_tags())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Player;
    use crate::objective::{CancelCause, CaptureAttempt};
    use crate::position::KillPosition;
    use dod::{ClientAreas, DeathMsg, PlayersIn, Weapon};

    fn at(secs: f32) -> GameTime {
        GameTime {
            real_offset: Duration::from_secs_f32(secs),
            viewdemo_offset: Duration::from_secs_f32(secs),
            frame_index: (secs * 100.0) as usize,
        }
    }

    /// A POV demo recorded by slot 0 (entity 1), against slot 1 (entity 2).
    fn pov() -> AnalyzerState {
        let mut state = AnalyzerState::default();
        state.pov_player_index = Some(0);
        for (slot, name, team) in [(0, "rec", Team::Allies), (1, "foe", Team::Axis)] {
            let mut p = Player::new_mock(slot, name);
            p.team = Some(team);
            state.players.push(p);
        }
        state
    }

    fn area(state: &mut AnalyzerState, secs: f32, area: u8, status: u8) {
        state.current_time = at(secs);
        let message = ClientAreas {
            icon_index: area,
            status: ClientAreaStatus::from(status),
            hud_icon: None,
        };
        use_capture_area_updates(
            state,
            &AnalyzerEvent::UserMessage(UserMessage::ClientAreas(message)),
        );
    }

    /// A kill as `use_position_updates` would push it, then tagged.
    fn kill(state: &mut AnalyzerState, secs: f32, killer: u8, victim: u8) -> KillAreaTags {
        state.current_time = at(secs);
        state.kill_positions.push(KillPosition {
            time: at(secs),
            weapon: Weapon::Garand,
            killer: None,
            victim: None,
            killer_team: None,
            victim_team: None,
            teamkill: false,
            killer_origin: None,
            victim_origin: None,
            distance: None,
            headshot: Default::default(),
            capture_area: Default::default(),
        });
        let death = DeathMsg {
            killer_client_index: killer,
            victim_client_index: victim,
            weapon: Weapon::Garand,
        };
        use_capture_area_updates(
            state,
            &AnalyzerEvent::UserMessage(UserMessage::DeathMsg(death)),
        );
        state.kill_positions.last().unwrap().capture_area
    }

    #[test]
    fn the_recorder_capping_tags_their_kill_and_their_death() {
        let mut state = pov();
        area(&mut state, 1.0, 0, 255);
        assert_eq!(kill(&mut state, 2.0, 1, 2).killer_capping, Capping::No);
        area(&mut state, 3.0, 0, 1);
        area(&mut state, 3.03, 0, 1);
        let tags = kill(&mut state, 3.05, 1, 2);
        assert_eq!(tags.killer_capping, Capping::Yes);
        assert_eq!(tags.victim_capping, Capping::Unknown);
        assert_eq!(tags.flag_tags(), [FlagTag::WhileCapping]);
        let tags = kill(&mut state, 3.06, 2, 1);
        assert_eq!(tags.victim_capping, Capping::Yes);
        assert_eq!(tags.killer_capping, Capping::Unknown);
        assert_eq!(tags.flag_tags(), [FlagTag::FlagDefence]);
        assert_eq!(state.capture_areas.recorder_visits.len(), 1);
        assert_eq!(state.capture_areas.recorder_visits[0].left, Some(at(3.06)));
    }

    #[test]
    fn a_dead_recorder_is_not_back_in_the_area_until_they_leave_it() {
        let mut state = pov();
        area(&mut state, 1.0, 0, 1);
        kill(&mut state, 1.01, 2, 1);
        // The touch goes on for a moment after the death.
        area(&mut state, 1.2, 0, 1);
        assert_eq!(kill(&mut state, 1.3, 1, 2).killer_capping, Capping::No);
        area(&mut state, 1.6, 0, 0);
        area(&mut state, 9.0, 0, 1);
        assert_eq!(kill(&mut state, 9.01, 1, 2).killer_capping, Capping::Yes);
        assert_eq!(state.capture_areas.recorder_visits.len(), 2);
    }

    #[test]
    fn leaving_a_quiet_area_or_a_round_reset_ends_the_visit() {
        let mut state = pov();
        area(&mut state, 1.0, 0, 1);
        area(&mut state, 1.1, 0, 0);
        assert_eq!(kill(&mut state, 1.2, 1, 2).killer_capping, Capping::No);
        // No leave message, but no touch for longer than STALE_AFTER.
        area(&mut state, 2.0, 1, 1);
        assert_eq!(kill(&mut state, 3.0, 1, 2).killer_capping, Capping::No);
        assert_eq!(state.capture_areas.recorder_visits[1].left, Some(at(2.0)));
        area(&mut state, 4.0, 1, 1);
        area(&mut state, 4.1, 0, 2);
        assert_eq!(kill(&mut state, 4.2, 1, 2).killer_capping, Capping::No);
        assert!(
            state
                .capture_areas
                .recorder_visits
                .iter()
                .all(|v| v.left.is_some())
        );
    }

    #[test]
    fn other_players_and_hltv_demos_stay_unknown() {
        let mut state = pov();
        area(&mut state, 1.0, 0, 1);
        // Neither player is the recorder.
        let tags = kill(&mut state, 1.01, 3, 2);
        assert_eq!(tags, KillAreaTags::default());
        // A demo with no ClientAreas at all can't say "not capping".
        let mut state = pov();
        assert_eq!(kill(&mut state, 1.0, 1, 2).killer_capping, Capping::Unknown);
        let mut state = pov();
        state.capture_areas.tracker.hltv = true;
        area(&mut state, 1.0, 0, 255);
        assert_eq!(kill(&mut state, 2.0, 1, 2).killer_capping, Capping::Unknown);
        // A suicide has no killer to tag.
        let mut state = pov();
        area(&mut state, 1.0, 0, 1);
        let tags = kill(&mut state, 1.01, 1, 1);
        assert_eq!(tags.killer_capping, Capping::Unknown);
        assert_eq!(tags.victim_capping, Capping::Yes);
    }

    #[test]
    fn players_in_keeps_each_change_once() {
        let mut state = pov();
        for (secs, team, count) in [
            (1.0, Team::Axis, 1),
            (1.1, Team::Axis, 1),
            (1.2, Team::Axis, 2),
            (1.5, Team::Unassigned, 0),
        ] {
            state.current_time = at(secs);
            let message = PlayersIn {
                area_index: 2,
                team,
                players_inside_area: count,
                required_players_to_capture: 2,
            };
            use_capture_area_updates(
                &mut state,
                &AnalyzerEvent::UserMessage(UserMessage::PlayersIn(message)),
            );
        }
        let counts: Vec<_> = state
            .capture_areas
            .attackers
            .iter()
            .map(|a| (a.team.clone(), a.count))
            .collect();
        assert_eq!(
            counts,
            [(Team::Axis, 1), (Team::Axis, 2), (Team::Unassigned, 0)]
        );
    }

    #[test]
    fn the_kill_that_broke_a_capture_cleared_the_flag() {
        let mut state = pov();
        let defender = Player::new_mock(5, "defender").id;
        for (secs, team, killer_team) in [
            (5.0, Team::Axis, Team::Allies),
            (5.5, Team::Axis, Team::Allies),
            (5.6, Team::Allies, Team::Axis),
        ] {
            kill(&mut state, secs, 0, 0);
            let k = state.kill_positions.last_mut().unwrap();
            *k = KillPosition {
                time: at(secs),
                killer: Some(defender.clone()),
                victim_team: Some(team),
                killer_team: Some(killer_team),
                ..k.clone()
            };
        }
        let attempt = CaptureAttempt {
            area_index: 1,
            team: Team::Axis,
            started: at(1.0),
            ended: Some(at(6.0)),
            outcome: AttemptOutcome::Cancelled,
            cancel_cause: Some(CancelCause::Break {
                breaker: defender.clone(),
            }),
        };
        state.objectives.attempts.push(attempt);
        use_capture_area_updates(&mut state, &AnalyzerEvent::Finalization);
        let cleared: Vec<bool> = state
            .kill_positions
            .iter()
            .map(|k| k.capture_area.cleared_flag)
            .collect();
        // The last attacker death by a defender, as cancel_cause credits.
        assert_eq!(cleared, [false, true, false]);
        assert_eq!(
            flag_tags_of(&state.kill_positions, &at(5.5), &defender),
            Vec::<FlagTag>::new(),
            "looked up by victim, not killer"
        );
    }

    #[test]
    fn flag_tags_are_looked_up_by_frame_and_victim() {
        let mut state = pov();
        area(&mut state, 1.0, 0, 1);
        kill(&mut state, 1.01, 1, 2);
        let victim = state.players[1].id.clone();
        state.kill_positions[0].victim = Some(victim.clone());
        assert_eq!(
            flag_tags_of(&state.kill_positions, &at(1.01), &victim),
            [FlagTag::WhileCapping]
        );
        assert!(flag_tags_of(&state.kill_positions, &at(7.0), &victim).is_empty());
    }

    /// How many kills get which tag, per demo and per demo type (#1132).
    /// Demos from `DOD_ANALYSIS_DEMOS` (a folder), else the PRE install's
    /// `dod` folder: an even spread of up to 80 of its match demos
    /// (`solifotw-*`, `ktps*`, `wsod25_*`, `k4-*`, `bandits-*`, `week*`, and
    /// every HLTV one). Fails when it finds none, when an HLTV demo tags
    /// anyone capping, or when the cleared flags don't match the cap breaks.
    /// `cargo test -p analysis --release -- --ignored --nocapture capture_area_tags`.
    #[test]
    #[ignore]
    fn capture_area_tags_on_real_demos() {
        let folder = std::env::var_os("DOD_ANALYSIS_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        let mut demos: Vec<_> = std::fs::read_dir(&folder)
            .unwrap_or_else(|e| panic!("{}: {e} (set DOD_ANALYSIS_DEMOS)", folder.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                name.ends_with(".dem")
                    && ["solifotw-", "ktps", "wsod25_", "k4-", "bandits-", "week"]
                        .iter()
                        .any(|prefix| name.starts_with(prefix))
                    || name.ends_with(".dem") && name.contains("hltv")
            })
            .collect();
        demos.sort();
        let step = demos.len().div_ceil(80).max(1);
        let hltv_named: Vec<_> = demos
            .iter()
            .filter(|p| p.to_string_lossy().contains("hltv"))
            .cloned()
            .collect();
        let mut picked: Vec<_> = demos.iter().step_by(step).cloned().collect();
        for path in hltv_named {
            if !picked.contains(&path) {
                picked.push(path);
            }
        }
        assert!(!picked.is_empty(), "no demos in {}", folder.display());

        // Per type: demos, kills, recorder kills, while capping, recorder
        // deaths, killed while capping, cleared flags, cap breaks, cleared
        // flags by the recorder.
        let mut totals: std::collections::BTreeMap<String, [usize; 9]> = Default::default();
        let mut recorders = std::collections::BTreeSet::new();
        for path in &picked {
            let bytes = std::fs::read(path).unwrap();
            let Ok(analysis) = crate::Analysis::try_from_bytes(&bytes) else {
                continue;
            };
            let kind = analysis.demo_info.demo_type.clone();
            let kills = &analysis.state.kill_positions;
            let count = |f: &dyn Fn(&KillAreaTags) -> bool| {
                kills.iter().filter(|k| f(&k.capture_area)).count()
            };
            let breaks = analysis
                .state
                .objectives
                .attempts
                .iter()
                .filter(|a| matches!(a.cancel_cause, Some(CancelCause::Break { .. })))
                .count();
            let row = [
                1,
                kills.len(),
                count(&|t| t.killer_capping != Capping::Unknown),
                count(&|t| t.killer_capping == Capping::Yes),
                count(&|t| t.victim_capping != Capping::Unknown),
                count(&|t| t.victim_capping == Capping::Yes),
                count(&|t| t.cleared_flag),
                breaks,
                count(&|t| t.cleared_flag && t.killer_capping != Capping::Unknown),
            ];
            let recorder = crate::highlights::highlights(&analysis).recorder;
            println!(
                "{}\t{kind}\t{}\t{}\tkills {} | recorder kills {} capping {} | recorder deaths {} capping {} | cleared {} of {} breaks, {} by the recorder",
                path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                analysis.demo_info.map_name,
                recorder.as_deref().unwrap_or("-"),
                row[1],
                row[2],
                row[3],
                row[4],
                row[5],
                row[6],
                row[7],
                row[8],
            );
            if kind == "HLTV" {
                assert_eq!(
                    row[2] + row[4],
                    0,
                    "{}: HLTV knows no one's area",
                    path.display()
                );
            } else if let Some(name) = recorder {
                recorders.insert(name);
            }
            // A kill can break two captures at once, never fewer than one.
            assert!(
                row[6] <= row[7] && (row[7] == 0) == (row[6] == 0),
                "{}",
                path.display()
            );
            let total = totals.entry(kind).or_default();
            for (t, r) in total.iter_mut().zip(row) {
                *t += r;
            }
        }
        for (kind, t) in &totals {
            println!(
                "{kind}: {} demos, {} kills; recorder kills {} of which capping {}; recorder deaths {} of which capping {}; cleared the flag {} (cap breaks {}), {} by the recorder",
                t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8]
            );
        }
        println!("POV recorders: {}", recorders.len());
        assert!(
            totals.get("POV").is_some_and(|t| t[3] > 0),
            "no POV kill while capping"
        );
    }
}
