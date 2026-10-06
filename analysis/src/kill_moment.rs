//! What was happening at the moment of each kill, for telling a highlight
//! apart from an ordinary frag (R&D, highlight detection).
//!
//! Every kill gets a [`KillMoment`]: who, with what, and where both players
//! were at a handful of instants up to a second before it (for line-of-sight
//! checks against the map, which need the BSP and so happen outside this
//! crate). A kill by the recording player of a POV demo also gets what only
//! that player's own frames carry: the view angles, so how far and how fast
//! the crosshair moved before the shot; the zoom; the keys held; on the
//! ground or not; how fast they were moving.
//!
//! ## The shot, not the death message
//!
//! A `DeathMsg` arrives after the shot that caused it, by a frame or more. For
//! a gun the shot is the last time the recording player's clip went down
//! (`CurWeapon`) in the [`SHOT_WINDOW`] before it; the moment is read there.
//! Without one (a knife, a grenade, a kill by someone else) it is read at the
//! death message itself.
//!
//! Pure: no I/O, so it builds for `wasm32` like the rest of the crate.

use std::collections::VecDeque;

use crate::entity_replay::{ClientDataReplay, EntityFields, field_f32};
use crate::{AnalyzerEvent, AnalyzerState, player::PlayerGlobalId, time::GameTime};
use dem::types::{Delta, EngineMessage, FrameData};
use dod::{UserMessage, Weapon};

/// How much history is kept, in seconds of demo time.
const HISTORY: f32 = 1.6;
/// How long before a death message its shot can be.
pub const SHOT_WINDOW: f32 = 0.6;
/// The instants before the shot at which both players' positions are kept.
pub const SAMPLE_OFFSETS: [f32; 7] = [0.0, 0.1, 0.2, 0.3, 0.5, 0.75, 1.0];

/// `usercmd.buttons` bits.
pub const IN_ATTACK: u16 = 1 << 0;
pub const IN_JUMP: u16 = 1 << 1;
pub const IN_DUCK: u16 = 1 << 2;
pub const IN_ATTACK2: u16 = 1 << 11;
/// `+speed`, which DoD binds to sprint.
pub const IN_RUN: u16 = 1 << 12;

/// `clientdata_t.flags`: `FL_DUCKING`.
const FL_DUCKING: u32 = 1 << 14;

/// One recording-player frame.
#[derive(Clone, Copy, Debug, Default)]
struct RecorderFrame {
    t: f32,
    /// Pitch, yaw: where the crosshair pointed.
    angles: [f32; 2],
    eye: [f32; 3],
    on_ground: bool,
    velocity: [f32; 3],
    buttons: u16,
    /// `usercmd` side and forward movement: strafing keys.
    side_move: f32,
    forward_move: f32,
    fov: f32,
    ducking: bool,
    prone: bool,
    health: i32,
}

/// The recording player's `clientdata_t` fields this reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct RecorderData {
    fov: f32,
    flags: u32,
    iuser3: u32,
}

impl EntityFields for RecorderData {
    fn apply(&mut self, delta: &Delta) {
        for (key, value) in delta {
            match key.as_bytes() {
                b"fov\0" => {
                    if let Some(v) = field_f32(value) {
                        self.fov = v;
                    }
                }
                b"flags\0" => self.flags = int(value),
                b"iuser3\0" => self.iuser3 = int(value),
                _ => {}
            }
        }
    }
}

fn int(value: &[u8]) -> u32 {
    let mut b = [0u8; 4];
    for (i, v) in value.iter().take(4).enumerate() {
        b[i] = *v;
    }
    u32::from_le_bytes(b)
}

/// Every player's position at one frame.
#[derive(Clone, Debug, Default)]
struct PositionFrame {
    t: f32,
    /// Entity index and origin, for the players the snapshot had.
    origins: Vec<(u16, [f32; 3])>,
}

/// Working state; never serialized.
#[derive(Debug, Default)]
pub(crate) struct MomentTracker {
    recorder: VecDeque<RecorderFrame>,
    positions: VecDeque<PositionFrame>,
    /// When the recording player's clip went down.
    shots: VecDeque<f32>,
    /// When the recording player pressed fire (the key going down).
    presses: VecDeque<f32>,
    attack_was_held: bool,
    data: ClientDataReplay<RecorderData>,
    clip: Option<(Weapon, u8)>,
    now: f32,
    /// The last `TimeLeft`: seconds left in the match, and when it came.
    time_left: Option<(f32, f32)>,
}

/// Where a player stood at one instant before the shot.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PoseSample {
    /// Seconds before the shot.
    pub before: f32,
    pub killer: Option<[f32; 3]>,
    /// The recording player's real eye; others' is their origin.
    pub killer_eye_is_real: bool,
    pub victim: Option<[f32; 3]>,
}

/// What the recording player's own frames say about a kill they made.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShooterView {
    /// Seconds from the shot to the death message.
    pub shot_lead: f32,
    /// What placed the shot: "press" (the fire key going down), "clip" (the
    /// clip going down) or "death" (neither found: the death message).
    pub shot_from: String,
    /// Whether a clip drop was found for the shot.
    pub shot_seen: bool,
    /// The field of view at the shot: 90 unzoomed.
    pub fov: f32,
    /// Largest angle between the view in the last 1.2 s and at the shot.
    pub turn_1200ms: f32,
    /// Strafing (`side_move`) and forward (`forward_move`) input at the shot.
    pub side_move: f32,
    pub forward_move: f32,
    /// Fastest the player moved along the ground in the 0.6 s before the
    /// shot: a strafe out from cover that stopped to shoot shows here and not
    /// in `horizontal_speed`.
    pub max_speed_600ms: f32,
    pub angles: [f32; 2],
    /// Largest angle between the view at any frame in the last 0.25 s and
    /// the view at the shot, in degrees: how far the crosshair travelled to
    /// get there.
    pub turn_250ms: f32,
    /// The same over the last 0.6 s.
    pub turn_600ms: f32,
    /// Fastest the view turned in the last 0.25 s, degrees per second.
    pub peak_turn_speed: f32,
    /// How long the view had been still (under 20 degrees per second) at the
    /// shot, in seconds, capped at 1.
    pub settled_for: f32,
    /// Zoomed in (`fov` under 90).
    pub scoped: bool,
    /// How long it had been zoomed, in seconds, when scoped.
    pub scoped_for: f32,
    pub on_ground: bool,
    /// Off the ground at any point in the 0.3 s before the shot.
    pub airborne_recently: bool,
    pub horizontal_speed: f32,
    pub jump_held: bool,
    pub duck_held: bool,
    pub sprint_held: bool,
    pub ducking: bool,
    pub prone: bool,
    pub attack2_held: bool,
    pub health: i32,
    /// Shots fired (clip drops) in the 3 s before this one, this one excluded.
    pub shots_before: u32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KillMoment {
    pub time: GameTime,
    pub weapon: Weapon,
    pub killer: Option<PlayerGlobalId>,
    pub victim: Option<PlayerGlobalId>,
    /// Entity indices (client slot plus one), for joining with other data.
    pub killer_index: u8,
    pub victim_index: u8,
    pub teamkill: bool,
    pub suicide: bool,
    pub by_recorder: bool,
    pub victim_is_recorder: bool,
    /// Both players at [`SAMPLE_OFFSETS`] before the shot.
    pub samples: Vec<PoseSample>,
    /// Only for a kill by the recording player of a POV demo.
    pub view: Option<ShooterView>,
    /// Seconds left on the match clock, from the last `TimeLeft`.
    #[serde(default)]
    pub match_time_left: Option<f32>,
}

fn angle_between(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dir = |p: [f32; 2]| {
        let (pitch, yaw) = (p[0].to_radians(), p[1].to_radians());
        [
            pitch.cos() * yaw.cos(),
            pitch.cos() * yaw.sin(),
            -pitch.sin(),
        ]
    };
    let (u, v) = (dir(a), dir(b));
    let dot = (u[0] * v[0] + u[1] * v[1] + u[2] * v[2]).clamp(-1.0, 1.0);
    dot.acos().to_degrees()
}

impl MomentTracker {
    fn record_frame(&mut self, frame_time: f32, frame: &RecorderFrame, positions: PositionFrame) {
        self.now = frame_time;
        self.recorder.push_back(*frame);
        self.positions.push_back(positions);
        while self
            .recorder
            .front()
            .is_some_and(|f| frame_time - f.t > HISTORY)
        {
            self.recorder.pop_front();
        }
        while self
            .positions
            .front()
            .is_some_and(|f| frame_time - f.t > HISTORY)
        {
            self.positions.pop_front();
        }
        while self.shots.front().is_some_and(|t| frame_time - *t > 4.0) {
            self.shots.pop_front();
        }
        let held = frame.buttons & IN_ATTACK != 0;
        if held && !self.attack_was_held {
            self.presses.push_back(frame_time);
        }
        self.attack_was_held = held;
        while self.presses.front().is_some_and(|t| frame_time - *t > 4.0) {
            self.presses.pop_front();
        }
    }

    fn position_at(&self, t: f32, index: u16) -> Option<[f32; 3]> {
        let frame = self
            .positions
            .iter()
            .rev()
            .find(|f| f.t <= t + 0.001)
            .or_else(|| self.positions.front())?;
        frame
            .origins
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, o)| *o)
    }

    fn recorder_at(&self, t: f32) -> Option<&RecorderFrame> {
        self.recorder
            .iter()
            .rev()
            .find(|f| f.t <= t + 0.001)
            .or_else(|| self.recorder.front())
    }

    fn view_at_shot(&self, shot: f32, shot_from: &str) -> Option<ShooterView> {
        let shot_seen = shot_from == "clip";
        let at = *self.recorder_at(shot)?;
        let window = |span: f32| {
            self.recorder
                .iter()
                .filter(move |f| f.t <= shot + 0.001 && shot - f.t <= span)
        };
        let turn = |span: f32| {
            window(span)
                .map(|f| angle_between(f.angles, at.angles))
                .fold(0.0f32, f32::max)
        };
        let mut peak = 0.0f32;
        let mut prev: Option<&RecorderFrame> = None;
        for f in window(0.25) {
            if let Some(p) = prev {
                let dt = f.t - p.t;
                if dt > 0.0005 {
                    peak = peak.max(angle_between(p.angles, f.angles) / dt);
                }
            }
            prev = Some(f);
        }
        // Walk back from the shot until the view was moving.
        let mut settled_for = 0.0f32;
        let mut later = at;
        for f in self.recorder.iter().rev().filter(|f| f.t <= shot + 0.001) {
            if f.t == at.t {
                continue;
            }
            let dt = later.t - f.t;
            if dt > 0.0005 && angle_between(f.angles, later.angles) / dt > 20.0 {
                break;
            }
            settled_for = (at.t - f.t).min(1.0);
            later = *f;
        }
        let scoped = at.fov > 0.0 && at.fov < 89.0;
        let scoped_for = if scoped {
            self.recorder
                .iter()
                .rev()
                .filter(|f| f.t <= shot + 0.001)
                .take_while(|f| f.fov > 0.0 && f.fov < 89.0)
                .last()
                .map_or(0.0, |f| at.t - f.t)
        } else {
            0.0
        };
        let airborne_recently = window(0.3).any(|f| !f.on_ground);
        let shots_before = self
            .shots
            .iter()
            .filter(|t| **t < shot - 0.001 && shot - **t <= 3.0)
            .count() as u32;
        Some(ShooterView {
            shot_lead: self.now - shot,
            shot_from: shot_from.to_string(),
            shot_seen,
            fov: at.fov,
            turn_1200ms: turn(1.2),
            side_move: at.side_move,
            forward_move: at.forward_move,
            max_speed_600ms: window(0.6)
                .map(|f| (f.velocity[0].powi(2) + f.velocity[1].powi(2)).sqrt())
                .fold(0.0f32, f32::max),
            angles: at.angles,
            turn_250ms: turn(0.25),
            turn_600ms: turn(0.6),
            peak_turn_speed: peak,
            settled_for,
            scoped,
            scoped_for,
            on_ground: at.on_ground,
            airborne_recently,
            horizontal_speed: (at.velocity[0].powi(2) + at.velocity[1].powi(2)).sqrt(),
            jump_held: at.buttons & IN_JUMP != 0,
            duck_held: at.buttons & IN_DUCK != 0,
            sprint_held: at.buttons & IN_RUN != 0,
            ducking: at.ducking,
            prone: at.prone,
            attack2_held: at.buttons & IN_ATTACK2 != 0,
            health: at.health,
            shots_before,
        })
    }
}

fn point(p: &[f32]) -> [f32; 3] {
    [
        p.first().copied().unwrap_or(0.0),
        p.get(1).copied().unwrap_or(0.0),
        p.get(2).copied().unwrap_or(0.0),
    ]
}

pub fn use_kill_moment_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    match event {
        AnalyzerEvent::Frame(frame) => {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                return;
            };
            state
                .moments
                .data
                .begin_frame(bt.1.sequence_info.incoming_sequence);
            let params = &bt.1.info.refparams;
            let data = state.moments.data.current().copied().unwrap_or_default();
            let recorder = RecorderFrame {
                t: frame.time,
                angles: [
                    params.cl_viewangles.first().copied().unwrap_or(0.0),
                    params.cl_viewangles.get(1).copied().unwrap_or(0.0),
                ],
                eye: point(&params.view_origin),
                on_ground: params.on_ground != 0,
                velocity: point(&params.sim_vel),
                buttons: bt.1.info.usercmd.buttons,
                side_move: bt.1.info.usercmd.side_move,
                forward_move: bt.1.info.usercmd.forward_move,
                fov: data.fov,
                ducking: data.flags & FL_DUCKING != 0,
                prone: data.iuser3 != 0,
                health: params.health,
            };
            // The snapshot as it stands before this frame's messages: what the
            // client was drawing.
            let recorder_index = state.pov_player_index.map(|s| u16::from(s) + 1);
            let mut origins: Vec<(u16, [f32; 3])> = state
                .positions
                .player_origins()
                .filter(|(i, _)| Some(*i) != recorder_index)
                .collect();
            if let (Some(index), Some(origin)) = (recorder_index, state.positions.recorder_origin())
            {
                origins.push((index, origin));
            }
            state.moments.record_frame(
                frame.time,
                &recorder,
                PositionFrame {
                    t: frame.time,
                    origins,
                },
            );
        }
        AnalyzerEvent::EngineMessage(EngineMessage::SvcServerInfo(_)) => {
            state.moments = MomentTracker::default();
        }
        AnalyzerEvent::EngineMessage(message @ EngineMessage::SvcClientData(_)) => {
            state.moments.data.apply(message);
        }
        AnalyzerEvent::UserMessage(UserMessage::TimeLeft(left)) => {
            let now = state.moments.now;
            state.moments.time_left = Some((left.0.as_secs_f32(), now));
        }
        AnalyzerEvent::UserMessage(UserMessage::CurWeapon(msg)) if msg.is_active => {
            let clip = msg.clip_ammo;
            if let Some((weapon, before)) = &state.moments.clip
                && *weapon == msg.weapon
                && clip < *before
            {
                let now = state.moments.now;
                state.moments.shots.push_back(now);
            }
            state.moments.clip = Some((msg.weapon.clone(), clip));
        }
        AnalyzerEvent::UserMessage(UserMessage::DeathMsg(death)) => {
            if death.victim_client_index == 0 {
                return;
            }
            let recorder = state.pov_player_index.map(|s| s + 1);
            let suicide = death.killer_client_index == 0
                || death.killer_client_index == death.victim_client_index;
            let by_recorder = !suicide && Some(death.killer_client_index) == recorder;
            let now = state.moments.now;
            let gun = !death.weapon.is_grenade() && !is_melee(&death.weapon);
            let last_in_window = |times: &VecDeque<f32>| {
                times
                    .iter()
                    .rev()
                    .find(|t| **t <= now + 0.001 && now - **t <= SHOT_WINDOW)
                    .copied()
            };
            // The fire key going down is when the player's own screen fired,
            // to the frame. A held trigger fires many rounds from one press,
            // so an automatic's shot is its last clip drop instead.
            let (shot, shot_from) = if by_recorder && gun {
                let press = last_in_window(&state.moments.presses);
                let clip = last_in_window(&state.moments.shots);
                match (is_automatic(&death.weapon), press, clip) {
                    (false, Some(p), _) => (Some(p), "press"),
                    (_, _, Some(c)) => (Some(c), "clip"),
                    (true, Some(p), None) => (Some(p), "press"),
                    _ => (None, "death"),
                }
            } else {
                (None, "death")
            };
            let shot_time = shot.unwrap_or(now);
            let view = by_recorder
                .then(|| state.moments.view_at_shot(shot_time, shot_from))
                .flatten();
            let samples = SAMPLE_OFFSETS
                .iter()
                .map(|before| {
                    let t = shot_time - before;
                    let killer = if suicide {
                        None
                    } else if by_recorder {
                        state.moments.recorder_at(t).map(|f| f.eye)
                    } else {
                        state
                            .moments
                            .position_at(t, u16::from(death.killer_client_index))
                    };
                    PoseSample {
                        before: *before,
                        killer,
                        killer_eye_is_real: by_recorder,
                        victim: state
                            .moments
                            .position_at(t, u16::from(death.victim_client_index)),
                    }
                })
                .collect();
            let killer = (!suicide)
                .then(|| state.find_player_by_client_index(death.killer_client_index - 1))
                .flatten();
            let victim = state.find_player_by_client_index(death.victim_client_index - 1);
            let teamkill = match (killer, victim) {
                (Some(k), Some(v)) => k.team.is_some() && k.team == v.team,
                _ => false,
            };
            let moment = KillMoment {
                time: state.current_time.clone(),
                weapon: death.weapon.clone(),
                killer: killer.map(|p| p.id.clone()),
                victim: victim.map(|p| p.id.clone()),
                killer_index: death.killer_client_index,
                victim_index: death.victim_client_index,
                teamkill,
                suicide,
                by_recorder,
                victim_is_recorder: Some(death.victim_client_index) == recorder,
                samples,
                view,
                match_time_left: state
                    .moments
                    .time_left
                    .map(|(left, at)| (left - (now - at)).max(0.0)),
            };
            state.kill_moments.push(moment);
        }
        _ => {}
    }
}

/// Weapons that fire round after round from one held trigger.
pub fn is_automatic(weapon: &Weapon) -> bool {
    matches!(
        weapon,
        Weapon::Thompson
            | Weapon::Stg44
            | Weapon::Bar
            | Weapon::Mp40
            | Weapon::Mg42
            | Weapon::Browning30Cal
            | Weapon::Mg34
            | Weapon::GreaseGun
            | Weapon::Fg42
            | Weapon::ScopedFg42
            | Weapon::Sten
            | Weapon::Bren
    )
}

pub fn is_melee(weapon: &Weapon) -> bool {
    matches!(
        weapon,
        Weapon::Kabar
            | Weapon::GermanKnife
            | Weapon::BritishKnife
            | Weapon::Spade
            | Weapon::K98Bayonet
            | Weapon::EnfieldBayonet
            | Weapon::ButtStock
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_between_views() {
        assert!(angle_between([0.0, 0.0], [0.0, 0.0]) < 0.01);
        assert!((angle_between([0.0, 0.0], [0.0, 90.0]) - 90.0).abs() < 0.01);
        assert!((angle_between([0.0, 170.0], [0.0, -170.0]) - 20.0).abs() < 0.01);
        assert!((angle_between([0.0, 0.0], [0.0, 180.0]) - 180.0).abs() < 0.01);
    }

    fn frame(t: f32, yaw: f32, fov: f32) -> RecorderFrame {
        RecorderFrame {
            t,
            angles: [0.0, yaw],
            on_ground: true,
            fov,
            ..Default::default()
        }
    }

    #[test]
    fn a_flick_is_a_big_fast_turn_ending_at_the_shot() {
        let mut m = MomentTracker::default();
        // Still at yaw 0, then 60 degrees in 0.1 s, then the shot.
        for i in 0..50 {
            let t = i as f32 * 0.01;
            let yaw = if t < 0.38 {
                0.0
            } else {
                ((t - 0.38) / 0.1 * 60.0).min(60.0)
            };
            m.record_frame(t, &frame(t, yaw, 90.0), PositionFrame::default());
        }
        let v = m.view_at_shot(0.49, "press").unwrap();
        assert!(v.turn_250ms > 55.0, "{v:?}");
        assert!(v.peak_turn_speed > 400.0, "{v:?}");
        assert!(v.settled_for < 0.05, "{v:?}");
        assert!(!v.scoped);
    }

    #[test]
    fn a_steady_scoped_shot_has_no_turn() {
        let mut m = MomentTracker::default();
        for i in 0..100 {
            let t = i as f32 * 0.01;
            let fov = if t < 0.4 { 90.0 } else { 20.0 };
            m.record_frame(t, &frame(t, 10.0, fov), PositionFrame::default());
        }
        let v = m.view_at_shot(0.99, "press").unwrap();
        assert!(v.turn_600ms < 0.1, "{v:?}");
        assert!(v.scoped);
        assert!((v.scoped_for - 0.59).abs() < 0.02, "{v:?}");
        assert!(v.settled_for > 0.9);
    }

    #[test]
    fn melee_weapons() {
        assert!(is_melee(&Weapon::Kabar));
        assert!(!is_melee(&Weapon::K98));
    }
}
