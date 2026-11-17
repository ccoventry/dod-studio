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
//!   team.
//! - **Cap breaks** (the league scoreboard's "breaks"): a block credited to
//!   the defender who caused it. Neither `StartProg` nor `CancelProg` names a
//!   player, so the break is found by timing, in [cancel_cause]: an
//!   attacker who died within [BREAK_WINDOW] before the cancel was a capper,
//!   and the defender who killed them broke the capture. A capper killed by a
//!   teammate, the world or themselves leaves the block unattributed; a
//!   cancel with no attacker death is the cappers walking off, abandoned.
//!
//! Messages are buffered per frame and resolved when the next frame starts,
//! because a capture's messages arrive in one frame but not in a guaranteed
//! order (in practice: `ObjScore`s, then `SetObj`, then `CapMsg`).

use crate::{
    AnalyzerEvent, AnalyzerState, player::PlayerGlobalId, position::KillPosition, time::GameTime,
};
use dod::{RoundState, Team, UserMessage};
use std::collections::HashMap;
use std::time::Duration;

/// How long before a `CancelProg` an attacker's death still counts as the
/// one that cancelled it (#192). The cancel does not come with the death but
/// 0.5 to 1.1 s after it, on nearly every cancel `cap_break_probe` measured
/// (2,558 cancels, 458 demos). Past 1.1 s, attacker deaths are no more common
/// than at any other moment, so a wider window would only add coincidences.
pub const BREAK_WINDOW: Duration = Duration::from_millis(1100);

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

    /// Cancelled attempts whose capper died just before the cancel, but not
    /// at a defender's hand: blocks with no breaker (#192).
    #[serde(default)]
    pub blocks_unattributed: u32,

    /// Cancelled attempts with no attacker death just before the cancel: the
    /// cappers left the area alive, so nobody broke it (#192).
    #[serde(default)]
    pub attempts_abandoned: u32,

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
    /// The stats spec first called this a "break". The league's breaks are
    /// stopped captures instead ([Player::cap_breaks](crate::Player)), so this
    /// is named for what it checks (#192).
    pub fn taken_from_enemy(&self) -> bool {
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

    /// For a cancelled attempt, what cancelled it as far as the demo shows
    /// (#192). `None` for any other outcome.
    #[serde(default)]
    pub cancel_cause: Option<CancelCause>,
}

/// What cancelled a capture attempt, from the deaths just before the
/// `CancelProg` (#192).
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CancelCause {
    /// A defender killed one of the attacking team within [BREAK_WINDOW]
    /// before the cancel: a cap break, credited to that defender.
    Break { breaker: PlayerGlobalId },

    /// An attacker died within the window, but not by a defender's hand:
    /// killed by a teammate, the world or themselves, or by a player whose
    /// team is unknown. A block with no breaker.
    Unattributed,

    /// No attacker died within the window: the cappers walked off the point.
    /// A defender stepping onto the point would land here too; the demo has
    /// no capture-area bounds to tell the two apart.
    Abandoned,
}

/// Why `attempt`, cancelled at `cancelled`, was cancelled, from `kills` (in
/// arrival order).
///
/// A death counts when its victim was on the attacking team, it came after
/// the attempt started, and at most [BREAK_WINDOW] before the cancel. The
/// victim is then taken to be a capper: the demo never names an attempt's
/// cappers. An attacker dies somewhere within any given 1.1 s about one time
/// in ten (`cap_break_probe`'s baseline), so roughly 5% of breaks credited
/// this way are a death elsewhere that happened to land in the window. Of the
/// deaths that count, the last one by a defender wins, as it is the one that
/// left too few cappers; with none by a defender, the block is unattributed.
///
/// A defender standing on the point is not looked for: capture areas are
/// brush triggers the server never sends, and `InitObj`'s flag origin is not
/// a world position (the victims above died a median 7,148 units from it),
/// so the demo cannot say who stood in an area.
pub fn cancel_cause(
    attempt: &CaptureAttempt,
    cancelled: &GameTime,
    kills: &[KillPosition],
) -> CancelCause {
    let mut deaths = cancelling_deaths(attempt, cancelled, kills).peekable();
    if deaths.peek().is_none() {
        return CancelCause::Abandoned;
    }
    deaths
        .find_map(|(_, k)| breaker_of(attempt, k))
        .map_or(CancelCause::Unattributed, |breaker| CancelCause::Break {
            breaker,
        })
}

/// Where in `kills` the kill that broke `attempt` is: the one
/// [cancel_cause] credits a [CancelCause::Break] to, `None` for any other
/// cause. The capture-area tags mark it "cleared the flag" (#1132).
pub(crate) fn breaking_kill(
    attempt: &CaptureAttempt,
    cancelled: &GameTime,
    kills: &[KillPosition],
) -> Option<usize> {
    cancelling_deaths(attempt, cancelled, kills)
        .find(|(_, k)| breaker_of(attempt, k).is_some())
        .map(|(index, _)| index)
}

/// The deaths [cancel_cause] weighs, latest first, with their index in
/// `kills`.
fn cancelling_deaths<'a>(
    attempt: &'a CaptureAttempt,
    cancelled: &GameTime,
    kills: &'a [KillPosition],
) -> impl Iterator<Item = (usize, &'a KillPosition)> {
    let earliest = cancelled
        .viewdemo_offset
        .saturating_sub(BREAK_WINDOW)
        .max(attempt.started.viewdemo_offset);
    let latest = cancelled.viewdemo_offset;
    kills.iter().enumerate().rev().filter(move |(_, k)| {
        k.victim_team
            .as_ref()
            .is_some_and(|team| same_side(team, &attempt.team))
            && k.time.viewdemo_offset >= earliest
            && k.time.viewdemo_offset <= latest
    })
}

/// The defender who made `kill`, when a defender of `attempt`'s flag did.
fn breaker_of(attempt: &CaptureAttempt, kill: &KillPosition) -> Option<PlayerGlobalId> {
    let defender = !kill.teamkill
        && kill
            .killer_team
            .as_ref()
            .is_some_and(|team| opposes(team, &attempt.team));
    defender.then(|| kill.killer.clone()).flatten()
}

/// The kills of `kills` (a finished analysis' `kill_positions`, in arrival
/// order) that had arrived when a cancel at `at` was resolved: everything up
/// to the end of the cancel's frame, as the cancel is resolved when the next
/// frame starts. What `kill_positions` held at that moment, so
/// [cancel_cause] gives the same answer on a cached analysis as it did live.
pub(crate) fn kills_known_at<'a>(kills: &'a [KillPosition], at: &GameTime) -> &'a [KillPosition] {
    &kills[..kills.partition_point(|k| k.time.frame_index <= at.frame_index)]
}

/// Every cancel's cause, and the totals `resolve_pending` adds up from them,
/// worked out again from a finished analysis' attempts and kills.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, PartialEq)]
pub(crate) struct CancelTally {
    /// Per attempt, in order: its `cancel_cause`.
    pub causes: Vec<Option<CancelCause>>,
    /// Breaks per breaker.
    pub breaks: HashMap<PlayerGlobalId, u32>,
    pub blocks_unattributed: u32,
    pub attempts_abandoned: u32,
}

/// [CancelTally] for `attempts` and `kills`, through [cancel_cause] like the
/// live analysis. The analyzer cache's v9 → v10 upgrade uses it on entries
/// saved before causes were kept (#743). `Err` names what is missing: a
/// cancelled attempt with no end time has no moment to look back from.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn recount_cancels(
    attempts: &[CaptureAttempt],
    kills: &[KillPosition],
) -> Result<CancelTally, &'static str> {
    let mut tally = CancelTally::default();
    for attempt in attempts {
        if attempt.outcome != AttemptOutcome::Cancelled {
            tally.causes.push(None);
            continue;
        }
        let Some(cancelled) = &attempt.ended else {
            return Err("a cancelled capture attempt has no end time");
        };
        let cause = cancel_cause(attempt, cancelled, kills_known_at(kills, cancelled));
        match &cause {
            CancelCause::Break { breaker } => {
                *tally.breaks.entry(breaker.clone()).or_default() += 1
            }
            CancelCause::Unattributed => tally.blocks_unattributed += 1,
            CancelCause::Abandoned => tally.attempts_abandoned += 1,
        }
        tally.causes.push(Some(cause));
    }
    Ok(tally)
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
        self.blocks_unattributed = 0;
        self.attempts_abandoned = 0;
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
            let Some(slot) = crate::wire_index::wire_slot(obj_score.client_index) else {
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
                    cancel_cause: None,
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
                let Some(attempt) = state.objectives.open_attempt_mut(*area) else {
                    continue;
                };
                if captured_later(&attempt.team) {
                    continue;
                }
                // Resolved at the start of the next frame, so a death in the
                // cancel's own packet is already in `kill_positions`.
                let cause = cancel_cause(attempt, &time, &state.kill_positions);
                attempt.outcome = AttemptOutcome::Cancelled;
                attempt.ended = Some(time.clone());
                attempt.cancel_cause = Some(cause.clone());
                match cause {
                    CancelCause::Break { breaker } => {
                        if let Some(player) = state.find_player_by_id_mut(&breaker) {
                            player.cap_breaks += 1;
                        }
                    }
                    CancelCause::Unattributed => state.objectives.blocks_unattributed += 1,
                    CancelCause::Abandoned => state.objectives.attempts_abandoned += 1,
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
#[path = "objective_tests.rs"]
mod tests;
