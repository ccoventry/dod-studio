//! Flag captures, flag ownership and capture attempts (#192).
//!
//! Rules 5 to 8 of the "What the demo knows" stats spec
//! (`docs/demo_stats_feasibility.md` points at it):
//!
//! - **Objective points** accumulate `ObjScore` *increments*. The value is a
//!   cumulative counter that resets, so neither the last nor the sum of the
//!   values is a total.
//! - **Cap credits.** `CapMsg` names exactly one capper. Everyone else who
//!   helped shows up only as an `ObjScore` increment in the *same frame*, so a
//!   capture's cappers are the named one plus every player whose objective
//!   score rose in that frame. No tolerance window: the spec measured the
//!   named capper's own increment in the same frame 99.5% of the time, and a
//!   wider window pulls in neighbouring captures.
//! - **Ownership.** `InitObj` gives the layout, `SetObj` every change of owner.
//!   `SetObj` names a flag by area index and `CapMsg` by display name; the two
//!   arrive in the same frame, which is how a flag's name gets learned.
//! - **Capture attempts.** `StartProg` opens one on an area. A `CancelProg`
//!   before the flag changes hands is a *block*, credited to the defending
//!   team; the demo never says which player blocked it.
//!
//! Messages are buffered per frame and resolved when the next frame starts,
//! because a capture's messages arrive in one frame but not in a guaranteed
//! order (in practice: `ObjScore`s, then `SetObj`, then `CapMsg`).

use crate::{AnalyzerEvent, AnalyzerState, player::PlayerGlobalId, time::GameTime};
use dod::{RoundState, Team, UserMessage};
use std::collections::HashMap;

/// Everything the demo says about the map's flags.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Objectives {
    /// One per capture area, from `InitObj` (or the first `SetObj` naming an
    /// area when the demo started after `InitObj` was sent).
    pub flags: Vec<Flag>,

    /// Completed flag captures since the match went live.
    pub captures: Vec<FlagCapture>,

    /// Timed capture attempts (`StartProg`) since the match went live. Flags
    /// with no capture time are taken instantly and never appear here.
    pub attempts: Vec<CaptureAttempt>,

    /// Last `ObjScore` value per player, for turning values into increments.
    #[serde(skip)]
    obj_last: HashMap<PlayerGlobalId, i32>,

    /// This frame's objective messages, resolved when the next frame starts.
    #[serde(skip)]
    pending: Vec<PendingEvent>,

    #[serde(skip)]
    pending_time: Option<GameTime>,
}

/// A capture area.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Flag {
    /// The area index `SetObj`, `StartProg` and `CancelProg` use.
    pub area_index: u8,
    pub entity_index: Option<u16>,

    /// Overview-map position, from `InitObj`.
    pub origin: Option<(i16, i16)>,

    /// Display name as `CapMsg` prints it. Learned from the first capture of
    /// this flag, so `None` for a flag nobody captured.
    pub name: Option<String>,

    /// Current owner. [Team::Unassigned] is neutral.
    pub owner: Team,
}

/// One completed flag capture.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FlagCapture {
    pub time: GameTime,

    /// Display name from `CapMsg`: a literal name or a map token such as
    /// `POINT_ANZIO_HILL`.
    pub flag_name: String,

    /// `None` when no `SetObj` arrived with the capture and the name had not
    /// been seen before.
    pub area_index: Option<u8>,

    /// The capturing team.
    pub team: Team,

    /// The capper `CapMsg` names. `None` if their slot did not resolve.
    pub capper: Option<PlayerGlobalId>,

    /// Everyone else whose objective score rose in the capture's frame.
    pub co_cappers: Vec<PlayerGlobalId>,

    /// Owner of this flag just before the capture. [Team::Unassigned] is
    /// neutral; `None` when the area, or who held it, is unknown (the demo
    /// started after `InitObj` and this is the flag's first change of hands).
    pub previous_owner: Option<Team>,

    /// Owner of every flag just before the capture, by area index. Enough to
    /// compute any "break" definition, such as a capture made while the
    /// capturing team was down to its last flag.
    pub owners_before: Vec<(u8, Team)>,
}

impl FlagCapture {
    /// The named capper followed by the co-cappers.
    pub fn cappers(&self) -> impl Iterator<Item = &PlayerGlobalId> {
        self.capper.iter().chain(self.co_cappers.iter())
    }

    /// A capture of a flag the other team held, as opposed to a neutral one.
    ///
    /// This is the spec's "obvious" break rule, not necessarily what a
    /// league's `cap_break` column means; [FlagCapture::owners_before] is kept
    /// so a narrower rule can be computed instead.
    pub fn is_break(&self) -> bool {
        self.previous_owner
            .as_ref()
            .is_some_and(|owner| opposes(owner, &self.team))
    }
}

/// One timed capture attempt, from `StartProg` to whatever ended it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CaptureAttempt {
    pub area_index: u8,

    /// The attacking team.
    pub team: Team,
    pub started: GameTime,
    pub ended: Option<GameTime>,
    pub outcome: AttemptOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AttemptOutcome {
    /// Still running when the demo ended.
    InProgress,

    /// The flag changed hands to the attacking team.
    Captured,

    /// `CancelProg`: too few attackers left in the area before the timer ran
    /// out, whether they died or walked out. This is a cap block.
    Cancelled,

    /// A second `StartProg` on the same area replaced this one with no
    /// `CancelProg` between them.
    Restarted,

    /// The round was reset while this attempt was running.
    RoundEnded,
}

#[derive(Debug)]
enum PendingEvent {
    Bump {
        player: PlayerGlobalId,
        team: Option<Team>,
    },
    SetObj {
        area: u8,
        team: Team,
    },
    Cap {
        capper: Option<PlayerGlobalId>,
        name: String,
        team: Team,
    },
    Start {
        area: u8,
        team: Team,
    },
    Cancel {
        area: u8,
    },
    RoundReset,
}

impl Objectives {
    /// Attempts by `attacking_team` that were cancelled: cap blocks, credited
    /// to the other team.
    pub fn blocked_attempts(&self, attacking_team: &Team) -> usize {
        self.attempts
            .iter()
            .filter(|a| {
                a.outcome == AttemptOutcome::Cancelled && same_side(&a.team, attacking_team)
            })
            .count()
    }

    pub fn flag(&self, area_index: u8) -> Option<&Flag> {
        self.flags.iter().find(|f| f.area_index == area_index)
    }

    /// Forgets everything counted so far, keeping the layout, ownership and
    /// learned flag names. Called when the match goes live.
    ///
    /// The last `ObjScore` values are kept on purpose: whether or not the
    /// server re-sends a zero at the wipe, the next value then still yields
    /// the right increment.
    pub(crate) fn clear_match_totals(&mut self) {
        self.captures.clear();
        self.attempts.clear();
    }

    pub(crate) fn convert_allies_to_british(&mut self) {
        let convert = |team: &mut Team| {
            if *team == Team::Allies {
                *team = Team::British;
            }
        };
        for flag in &mut self.flags {
            convert(&mut flag.owner);
        }
        for capture in &mut self.captures {
            convert(&mut capture.team);
            if let Some(owner) = &mut capture.previous_owner {
                convert(owner);
            }
            for (_, owner) in &mut capture.owners_before {
                convert(owner);
            }
        }
        for attempt in &mut self.attempts {
            convert(&mut attempt.team);
        }
    }

    fn flag_mut(&mut self, area_index: u8) -> &mut Flag {
        let index = match self.flags.iter().position(|f| f.area_index == area_index) {
            Some(index) => index,
            None => {
                self.flags.push(Flag {
                    area_index,
                    entity_index: None,
                    origin: None,
                    name: None,
                    owner: Team::Unassigned,
                });
                self.flags.len() - 1
            }
        };
        &mut self.flags[index]
    }

    fn open_attempt_mut(&mut self, area_index: u8) -> Option<&mut CaptureAttempt> {
        self.attempts
            .iter_mut()
            .rev()
            .find(|a| a.area_index == area_index && a.outcome == AttemptOutcome::InProgress)
    }
}

/// Allies and British are the same side: the wire reports British as Allies.
fn same_side(left: &Team, right: &Team) -> bool {
    let norm = |team: &Team| match team {
        Team::British => Team::Allies,
        other => other.clone(),
    };
    norm(left) == norm(right)
}

fn is_playing(team: &Team) -> bool {
    matches!(team, Team::Allies | Team::British | Team::Axis)
}

/// Both teams are playing teams, on different sides.
fn opposes(left: &Team, right: &Team) -> bool {
    is_playing(left) && is_playing(right) && !same_side(left, right)
}

fn wire_team(state: &AnalyzerState, team: &Team) -> Team {
    if *team == Team::Allies && state.allies_are_british {
        Team::British
    } else {
        team.clone()
    }
}

pub fn use_objective_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    let pending = match event {
        AnalyzerEvent::Frame(_) | AnalyzerEvent::Finalization => {
            resolve_pending(state);
            return;
        }

        AnalyzerEvent::UserMessage(UserMessage::ObjScore(obj_score)) => {
            let Some(slot) = obj_score.client_index.checked_sub(1) else {
                return;
            };
            let Some(player) = state.find_player_by_client_index(slot) else {
                return;
            };
            let id = player.id.clone();
            let team = player.team.clone();

            let value = obj_score.score as i32;
            let previous = state
                .objectives
                .obj_last
                .insert(id.clone(), value)
                .unwrap_or(0);
            // A value below the last one means the counter was reset and then
            // scored again. An equal one is a resend, and gains nothing.
            let gained = if value > previous {
                value - previous
            } else if value > 0 && value < previous {
                value
            } else {
                0
            };
            if gained <= 0 {
                return;
            }
            if let Some(player) = state.find_player_by_id_mut(&id) {
                player.obj_points += gained as u32;
            }
            PendingEvent::Bump { player: id, team }
        }

        AnalyzerEvent::UserMessage(UserMessage::InitObj(init_obj)) => {
            for objective in &init_obj.objectives {
                let owner = objective
                    .team
                    .as_ref()
                    .map(|team| wire_team(state, team))
                    .unwrap_or(Team::Unassigned);
                let flag = state.objectives.flag_mut(objective.area_index);
                flag.entity_index = Some(objective.entity_index);
                flag.origin = Some(objective.origin);
                flag.owner = owner;
            }
            return;
        }

        AnalyzerEvent::UserMessage(UserMessage::SetObj(set_obj)) => PendingEvent::SetObj {
            area: set_obj.area_index,
            team: set_obj
                .team
                .as_ref()
                .map(|team| wire_team(state, team))
                .unwrap_or(Team::Unassigned),
        },

        AnalyzerEvent::UserMessage(UserMessage::CapMsg(cap_msg)) => PendingEvent::Cap {
            capper: cap_msg
                .client_index
                .checked_sub(1)
                .and_then(|slot| state.find_player_by_client_index(slot))
                .map(|player| player.id.clone()),
            name: cap_msg.point_name.clone(),
            team: wire_team(state, &cap_msg.team),
        },

        AnalyzerEvent::UserMessage(UserMessage::StartProg(start)) => PendingEvent::Start {
            area: start.area_index,
            team: wire_team(state, &start.team),
        },

        AnalyzerEvent::UserMessage(UserMessage::StartProgF(start)) => PendingEvent::Start {
            area: start.area_index,
            team: wire_team(state, &start.team),
        },

        AnalyzerEvent::UserMessage(UserMessage::CancelProg(cancel)) => PendingEvent::Cancel {
            area: cancel.area_index,
        },

        AnalyzerEvent::UserMessage(UserMessage::RoundState(RoundState::Reset)) => {
            PendingEvent::RoundReset
        }

        _ => return,
    };

    let objectives = &mut state.objectives;
    if objectives.pending_time.is_none() {
        objectives.pending_time = Some(state.current_time.clone());
    }
    objectives.pending.push(pending);
}

fn resolve_pending(state: &mut AnalyzerState) {
    let events = std::mem::take(&mut state.objectives.pending);
    let Some(time) = state.objectives.pending_time.take() else {
        return;
    };

    let owners_before: Vec<(u8, Team)> = state
        .objectives
        .flags
        .iter()
        .map(|f| (f.area_index, f.owner.clone()))
        .collect();

    // Captures first, against the ownership as it stood before this frame.
    let mut bumps: Vec<(PlayerGlobalId, Option<Team>)> = events
        .iter()
        .filter_map(|e| match e {
            PendingEvent::Bump { player, team } => Some((player.clone(), team.clone())),
            _ => None,
        })
        .collect();
    let named: Vec<PlayerGlobalId> = events
        .iter()
        .filter_map(|e| match e {
            PendingEvent::Cap { capper, .. } => capper.clone(),
            _ => None,
        })
        .collect();
    // A capper named by another capture in this frame is not a co-capper here.
    bumps.retain(|(player, _)| !named.contains(player));

    let mut set_objs_used = vec![false; events.len()];
    for event in &events {
        let PendingEvent::Cap { capper, name, team } = event else {
            continue;
        };

        // The SetObj handing a flag to this team in this frame is this flag.
        let matched = events.iter().enumerate().find_map(|(i, e)| match e {
            PendingEvent::SetObj { area, team: owner }
                if !set_objs_used[i] && same_side(owner, team) =>
            {
                Some((i, *area))
            }
            _ => None,
        });
        let area_index = match matched {
            Some((i, area)) => {
                set_objs_used[i] = true;
                let flag = state.objectives.flag_mut(area);
                if flag.name.is_none() {
                    flag.name = Some(name.clone());
                }
                Some(area)
            }
            None => state
                .objectives
                .flags
                .iter()
                .find(|f| f.name.as_deref() == Some(name.as_str()))
                .map(|f| f.area_index),
        };
        let previous_owner = area_index.and_then(|area| {
            owners_before
                .iter()
                .find(|(a, _)| *a == area)
                .map(|(_, owner)| owner.clone())
        });

        // Everyone else who scored in this frame, unless they are known to be
        // on the other team.
        let mut co_cappers = vec![];
        bumps.retain(|(player, player_team)| {
            let enemy = player_team
                .as_ref()
                .is_some_and(|player_team| opposes(player_team, team));
            if !enemy {
                co_cappers.push(player.clone());
            }
            enemy
        });

        let capture = FlagCapture {
            time: time.clone(),
            flag_name: name.clone(),
            area_index,
            team: team.clone(),
            capper: capper.clone(),
            co_cappers,
            previous_owner,
            owners_before: owners_before.clone(),
        };
        let credited: Vec<PlayerGlobalId> = capture.cappers().cloned().collect();
        for id in &credited {
            if let Some(player) = state.find_player_by_id_mut(id) {
                player.cap_credits += 1;
            }
        }
        state.objectives.captures.push(capture);
    }

    // Then ownership and attempts, in arrival order.
    for (i, event) in events.iter().enumerate() {
        match event {
            PendingEvent::SetObj { area, team } => {
                state.objectives.flag_mut(*area).owner = team.clone();
                if let Some(attempt) = state.objectives.open_attempt_mut(*area)
                    && same_side(&attempt.team, team)
                {
                    attempt.outcome = AttemptOutcome::Captured;
                    attempt.ended = Some(time.clone());
                }
            }

            PendingEvent::Start { area, team } => {
                if let Some(attempt) = state.objectives.open_attempt_mut(*area) {
                    attempt.outcome = AttemptOutcome::Restarted;
                    attempt.ended = Some(time.clone());
                }
                state.objectives.attempts.push(CaptureAttempt {
                    area_index: *area,
                    team: team.clone(),
                    started: time.clone(),
                    ended: None,
                    outcome: AttemptOutcome::InProgress,
                });
            }

            PendingEvent::Cancel { area } => {
                // Cancelled in the same frame the flag fell is still a capture;
                // the SetObj after this closes it.
                let captured_later = |team: &Team| {
                    events[i + 1..].iter().any(|e| {
                        matches!(e, PendingEvent::SetObj { area: a, team: t }
                            if a == area && same_side(t, team))
                    })
                };
                if let Some(attempt) = state.objectives.open_attempt_mut(*area)
                    && !captured_later(&attempt.team)
                {
                    attempt.outcome = AttemptOutcome::Cancelled;
                    attempt.ended = Some(time.clone());
                }
            }

            PendingEvent::RoundReset => {
                for attempt in &mut state.objectives.attempts {
                    if attempt.outcome == AttemptOutcome::InProgress {
                        attempt.outcome = AttemptOutcome::RoundEnded;
                        attempt.ended = Some(time.clone());
                    }
                }
            }

            PendingEvent::Bump { .. } | PendingEvent::Cap { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Player, time::use_timing_updates};
    use dem::types::{Frame, FrameData};

    // Wire team bytes.
    const ALLIES: u8 = 1;
    const AXIS: u8 = 2;

    fn run(state: &mut AnalyzerState, event: &AnalyzerEvent) {
        use_timing_updates(state, event);
        use_objective_updates(state, event);
    }

    fn next_frame(state: &mut AnalyzerState) {
        let frame = Frame {
            time: 0.0,
            frame: 0,
            frame_data: FrameData::DemoStart,
        };
        run(state, &AnalyzerEvent::Frame(&frame));
    }

    /// Sends one user message, as its wire bytes.
    fn send(state: &mut AnalyzerState, name: &str, data: &[u8]) {
        let Ok(msg) = UserMessage::new(name.as_bytes(), data) else {
            panic!("{name} {data:?} did not parse");
        };
        run(state, &AnalyzerEvent::UserMessage(msg));
    }

    fn obj_score(state: &mut AnalyzerState, client_index: u8, score: i16) {
        let [lo, hi] = score.to_le_bytes();
        send(state, "ObjScore", &[client_index, lo, hi]);
    }

    fn cap_msg(state: &mut AnalyzerState, client_index: u8, name: &str, team: u8) {
        let mut data = vec![client_index];
        data.extend_from_slice(name.as_bytes());
        data.extend_from_slice(&[0, team]);
        send(state, "CapMsg", &data);
    }

    fn player(slot: u8, name: &str, team: Team) -> Player {
        let mut player = Player::new_mock(slot, name);
        player.team = Some(team);
        player
    }

    fn id(state: &AnalyzerState, name: &str) -> PlayerGlobalId {
        get(state, name).id.clone()
    }

    fn get<'a>(state: &'a AnalyzerState, name: &str) -> &'a Player {
        state.players.iter().find(|p| p.name == name).unwrap()
    }

    fn outcomes(state: &AnalyzerState) -> Vec<AttemptOutcome> {
        state
            .objectives
            .attempts
            .iter()
            .map(|a| a.outcome.clone())
            .collect()
    }

    /// Slots 0-2 Axis, slot 3 Allies. Client indices on the wire are 1-based.
    fn two_teams() -> AnalyzerState {
        let mut state = AnalyzerState::default();
        state.players.push(player(0, "a1", Team::Axis));
        state.players.push(player(1, "a2", Team::Axis));
        state.players.push(player(2, "a3", Team::Axis));
        state.players.push(player(3, "b1", Team::Allies));
        next_frame(&mut state);
        state
    }

    #[test]
    fn co_cappers_are_the_same_frame_obj_score_rises() {
        let mut state = two_teams();
        // As the wire orders a capture: the ObjScores, then SetObj, then CapMsg.
        obj_score(&mut state, 1, 1);
        obj_score(&mut state, 2, 1);
        obj_score(&mut state, 4, 1); // an Allied player, not a co-capper
        send(&mut state, "SetObj", &[1, AXIS, 0]);
        cap_msg(&mut state, 2, "Grassy Knoll", AXIS);
        obj_score(&mut state, 3, 0); // no rise, no credit
        next_frame(&mut state);

        let captures = &state.objectives.captures;
        assert_eq!(captures.len(), 1);
        let capture = &captures[0];
        assert_eq!(capture.capper, Some(id(&state, "a2")));
        assert_eq!(capture.co_cappers, vec![id(&state, "a1")]);
        assert_eq!(capture.area_index, Some(1));
        assert_eq!(capture.team, Team::Axis);
        // No InitObj and no earlier SetObj: who held it before is unknown.
        assert_eq!(capture.previous_owner, None);
        assert!(!capture.is_break());

        assert_eq!(get(&state, "a1").cap_credits, 1);
        assert_eq!(get(&state, "a2").cap_credits, 1);
        assert_eq!(get(&state, "a3").cap_credits, 0);
        assert_eq!(get(&state, "b1").cap_credits, 0);
        assert_eq!(get(&state, "b1").obj_points, 1);

        let flag = state.objectives.flag(1).unwrap();
        assert_eq!(flag.name.as_deref(), Some("Grassy Knoll"));
        assert_eq!(flag.owner, Team::Axis);
    }

    #[test]
    fn a_rise_in_a_neighbouring_frame_is_not_a_co_capper() {
        let mut state = two_teams();
        obj_score(&mut state, 1, 1);
        next_frame(&mut state);
        send(&mut state, "SetObj", &[1, AXIS, 0]);
        cap_msg(&mut state, 2, "Grassy Knoll", AXIS);
        next_frame(&mut state);

        let capture = &state.objectives.captures[0];
        assert!(capture.co_cappers.is_empty());
        assert_eq!(get(&state, "a1").cap_credits, 0);
        assert_eq!(get(&state, "a2").cap_credits, 1);
    }

    #[test]
    fn taking_an_enemy_flag_is_a_break_and_the_learned_name_finds_its_area() {
        let mut state = two_teams();
        send(&mut state, "SetObj", &[0, ALLIES, 0]);
        cap_msg(&mut state, 4, "Church", ALLIES);
        next_frame(&mut state);

        send(&mut state, "SetObj", &[0, AXIS, 0]);
        cap_msg(&mut state, 1, "Church", AXIS);
        next_frame(&mut state);

        // No SetObj this time: the name learned above still places it.
        cap_msg(&mut state, 4, "Church", ALLIES);
        next_frame(&mut state);

        let captures = &state.objectives.captures;
        assert!(!captures[0].is_break());
        assert!(captures[1].is_break());
        assert_eq!(captures[1].previous_owner, Some(Team::Allies));
        assert_eq!(captures[1].owners_before, vec![(0, Team::Allies)]);
        assert_eq!(captures[2].area_index, Some(0));
    }

    #[test]
    fn objective_points_add_up_rises_across_a_counter_reset() {
        let mut state = two_teams();
        for value in [1, 3, 3, 0, 1, 1] {
            obj_score(&mut state, 1, value);
            next_frame(&mut state);
        }
        // 1, then 2 more, a resend, a reset to 0, 1 more, and a resend.
        assert_eq!(get(&state, "a1").obj_points, 4);
    }

    #[test]
    fn a_cancelled_attempt_is_a_block_for_the_defenders() {
        let mut state = two_teams();
        send(&mut state, "StartProg", &[1, AXIS, 2, 0]);
        next_frame(&mut state);
        send(&mut state, "CancelProg", &[1, 0]);
        next_frame(&mut state);

        send(&mut state, "StartProg", &[1, AXIS, 2, 0]);
        next_frame(&mut state);
        send(&mut state, "SetObj", &[1, AXIS, 0]);
        cap_msg(&mut state, 1, "Grassy Knoll", AXIS);
        next_frame(&mut state);

        assert_eq!(
            outcomes(&state),
            [AttemptOutcome::Cancelled, AttemptOutcome::Captured]
        );
        assert_eq!(state.objectives.blocked_attempts(&Team::Axis), 1);
        assert_eq!(state.objectives.blocked_attempts(&Team::Allies), 0);
    }

    #[test]
    fn a_cancel_in_the_frame_the_flag_falls_is_not_a_block() {
        let mut state = two_teams();
        send(&mut state, "StartProg", &[1, AXIS, 2, 0]);
        next_frame(&mut state);
        send(&mut state, "CancelProg", &[1, 0]);
        send(&mut state, "SetObj", &[1, AXIS, 0]);
        cap_msg(&mut state, 1, "Grassy Knoll", AXIS);
        next_frame(&mut state);

        assert_eq!(outcomes(&state), [AttemptOutcome::Captured]);
        assert_eq!(state.objectives.blocked_attempts(&Team::Axis), 0);
    }

    #[test]
    fn a_restart_or_a_round_reset_ends_an_attempt_without_a_block() {
        let mut state = two_teams();
        send(&mut state, "StartProg", &[2, ALLIES, 5, 0]);
        next_frame(&mut state);
        send(&mut state, "StartProg", &[2, ALLIES, 5, 0]);
        next_frame(&mut state);
        send(&mut state, "RoundState", &[0]);
        // The round reset's own SetObjs hand flags back; none is a capture.
        send(&mut state, "SetObj", &[2, 0, 0]);
        next_frame(&mut state);

        assert_eq!(
            outcomes(&state),
            [AttemptOutcome::Restarted, AttemptOutcome::RoundEnded]
        );
        assert_eq!(state.objectives.blocked_attempts(&Team::Allies), 0);
        assert!(state.objectives.captures.is_empty());
        assert_eq!(state.objectives.flag(2).unwrap().owner, Team::Unassigned);
    }

    #[test]
    fn init_obj_lays_out_the_flags() {
        let mut state = two_teams();
        // Per objective: u16 entity, u8 area, u8 team, four icon bytes, i16 x, i16 y.
        let mut data = vec![2];
        data.extend_from_slice(&[213, 0, 2, 0, 1, 0, 0, 0]);
        data.extend_from_slice(&6368i16.to_le_bytes());
        data.extend_from_slice(&(-1985i16).to_le_bytes());
        data.extend_from_slice(&[217, 0, 0, ALLIES, 1, 0, 0, 0]);
        data.extend_from_slice(&1664i16.to_le_bytes());
        data.extend_from_slice(&(-15264i16).to_le_bytes());
        send(&mut state, "InitObj", &data);

        let flag = state.objectives.flag(2).unwrap();
        assert_eq!(flag.entity_index, Some(213));
        assert_eq!(flag.origin, Some((6368, -1985)));
        assert_eq!(flag.owner, Team::Unassigned);
        assert_eq!(state.objectives.flag(0).unwrap().owner, Team::Allies);
    }
}
