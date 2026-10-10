//! Where each kill happened: both players' positions and the distance between
//! them (#448, first slice).
//!
//! Positions come from the entity snapshots (`entity_replay`), read at the
//! moment the `DeathMsg` arrives. A packet carries its reliable messages
//! before its entity update, so what is read is the snapshot from the
//! previous packet: about 10ms old on the demos measured, which is nothing
//! at running speed.
//!
//! What a demo can know differs by type, and `position_field_probe` measured
//! it:
//!
//! - an HLTV demo carries every player in every snapshot, so both positions
//!   are known for every kill;
//! - a POV demo carries teammates always, but enemies only while the
//!   recording player's view could reach them. A third of kills had one side
//!   out of the snapshot on the two POV demos measured; those keep what is
//!   known and leave the rest `None` rather than guess from a stale position.
//! - the recording player's own entity never carries an origin (the client
//!   predicts its own movement), so that player's position comes from
//!   `svc_clientdata` instead. Without it, every death of the recording
//!   player sat at the world origin.

use crate::entity_replay::{ClientDataReplay, EntityFields, EntityReplay, field_f32};
use crate::{AnalyzerEvent, AnalyzerState, player::PlayerGlobalId, time::GameTime};
use dem::types::{Delta, EngineMessage, FrameData};
use dod::{Team, UserMessage, Weapon};

/// The one thing the analyzer keeps per player entity. Small on purpose: this
/// is updated for every player on nearly every packet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerPose {
    pub origin: [f32; 3],
    /// Some origin field has been seen. A player entity's baseline has none,
    /// so a `false` here means "position unknown", not "at 0,0,0".
    pub has_origin: bool,
}

impl PlayerPose {
    pub fn known_origin(&self) -> Option<[f32; 3]> {
        self.has_origin.then_some(self.origin)
    }
}

/// Working state behind `kill_positions`. Never serialized.
#[derive(Debug, Default)]
pub(crate) struct PositionTracker {
    players: EntityReplay<PlayerPose>,
    recorder: ClientDataReplay<PlayerPose>,
}

impl EntityFields for PlayerPose {
    fn apply(&mut self, delta: &Delta) {
        // A walk over the keys, not three `get`s: hashing three `String`
        // keys per delta measured nearly twice as slow as comparing the
        // handful a player delta carries. Names keep their NUL terminator
        // (see `entity_replay::field`), and matching only the terminated form
        // lets the length check skip most keys without reading them.
        for (key, value) in delta {
            let axis = match key.as_bytes() {
                b"origin[0]\0" => 0,
                b"origin[1]\0" => 1,
                b"origin[2]\0" => 2,
                _ => continue,
            };
            if let Some(v) = field_f32(value) {
                self.origin[axis] = v;
                self.has_origin = true;
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KillPosition {
    pub time: GameTime,
    pub weapon: Weapon,
    /// `None` when the world or the victim themself did it.
    pub killer: Option<PlayerGlobalId>,
    pub victim: Option<PlayerGlobalId>,
    pub killer_team: Option<Team>,
    pub victim_team: Option<Team>,
    pub teamkill: bool,
    /// World coordinates, `None` when the snapshot did not have the player.
    pub killer_origin: Option<[f32; 3]>,
    pub victim_origin: Option<[f32; 3]>,
    /// Straight-line distance between the two players, in world units (one
    /// unit is about an inch). Only for a kill by another player with both
    /// positions known.
    pub distance: Option<f32>,
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

pub fn use_position_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    match event {
        AnalyzerEvent::Frame(frame) => {
            if let FrameData::NetworkMessage(bt) = &frame.frame_data {
                let sequence = bt.1.sequence_info.incoming_sequence;
                state.positions.players.begin_frame(sequence);
                state.positions.recorder.begin_frame(sequence);
            }
        }
        AnalyzerEvent::EngineMessage(EngineMessage::SvcServerInfo(info)) => {
            // Entities 1..=maxplayers are players; above that, the same
            // indices below 33 are ordinary entities.
            state.positions = PositionTracker {
                players: EntityReplay::new(1..=u16::from(info.max_players.max(1))),
                recorder: ClientDataReplay::default(),
            };
        }
        AnalyzerEvent::EngineMessage(message @ EngineMessage::SvcClientData(_)) => {
            state.positions.recorder.apply(message)
        }
        AnalyzerEvent::EngineMessage(message) => state.positions.players.apply(message),
        AnalyzerEvent::UserMessage(UserMessage::DeathMsg(death)) => {
            if death.victim_client_index == 0 {
                return;
            }
            let by_other = death.killer_client_index != 0
                && death.killer_client_index != death.victim_client_index;
            // DeathMsg indices are entity indices: the client slot plus one.
            let recorder = state.pov_player_index.map(|slot| u16::from(slot) + 1);
            let origin = |client_index: u8| {
                let index = u16::from(client_index);
                let pose = if Some(index) == recorder {
                    state.positions.recorder.current()
                } else {
                    state.positions.players.get(index)
                };
                pose.and_then(PlayerPose::known_origin)
            };
            let victim_origin = origin(death.victim_client_index);
            let killer_origin = by_other
                .then(|| origin(death.killer_client_index))
                .flatten();

            let victim = state.find_player_by_client_index(death.victim_client_index - 1);
            let killer = by_other
                .then(|| state.find_player_by_client_index(death.killer_client_index - 1))
                .flatten();
            let victim_team = victim.and_then(|p| p.team.clone());
            let killer_team = killer.and_then(|p| p.team.clone());
            let teamkill = by_other && killer_team.is_some() && killer_team == victim_team;

            let kill = KillPosition {
                time: state.current_time.clone(),
                weapon: death.weapon.clone(),
                killer: killer.map(|p| p.id.clone()),
                victim: victim.map(|p| p.id.clone()),
                killer_team,
                victim_team,
                teamkill,
                killer_origin,
                victim_origin,
                distance: killer_origin
                    .zip(victim_origin)
                    .map(|(a, b)| distance(a, b)),
            };
            state.kill_positions.push(kill);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Player;
    use crate::entity_replay::tests::{changes, delta, full};
    use dod::DeathMsg;

    fn state_with(players: &[(u8, &str, Team)]) -> AnalyzerState {
        let mut state = AnalyzerState::default();
        for (slot, name, team) in players {
            let mut p = Player::new_mock(*slot, name);
            p.team = Some(team.clone());
            state.players.push(p);
        }
        state
    }

    fn at(x: f32, y: f32, z: f32) -> Delta {
        delta(&[("origin[0]", x), ("origin[1]", y), ("origin[2]", z)])
    }

    fn snapshot(state: &mut AnalyzerState, sequence: i32, message: EngineMessage) {
        state.positions.players.begin_frame(sequence);
        state.positions.recorder.begin_frame(sequence);
        use_position_updates(state, &AnalyzerEvent::EngineMessage(&message));
    }

    fn client_data(delta_sequence: Option<i32>, fields: Delta) -> EngineMessage {
        EngineMessage::SvcClientData(dem::types::SvcClientData {
            has_delta_update_mask: delta_sequence.is_some(),
            delta_update_mask: delta_sequence.map(|s| dem::nbit_num!(s & 0xff, 8)),
            client_data: fields,
            weapon_data: None,
        })
    }

    fn death(state: &mut AnalyzerState, killer: u8, victim: u8, weapon: Weapon) {
        use_position_updates(
            state,
            &AnalyzerEvent::UserMessage(UserMessage::DeathMsg(DeathMsg {
                killer_client_index: killer,
                victim_client_index: victim,
                weapon,
            })),
        );
    }

    #[test]
    fn records_both_positions_and_the_distance() {
        // Client slots are 0-based, entity and DeathMsg indices 1-based.
        let mut state = state_with(&[(0, "killer", Team::Allies), (1, "victim", Team::Axis)]);
        snapshot(
            &mut state,
            1,
            full(&[(1, at(0.0, 0.0, 0.0)), (2, at(30.0, 40.0, 0.0))]),
        );
        // Only x moves: y and z carry over from the snapshot before.
        snapshot(
            &mut state,
            2,
            changes(1, &[(2, Some(delta(&[("origin[0]", 300.0)])))]),
        );
        death(&mut state, 1, 2, Weapon::K98);

        let k = &state.kill_positions[0];
        assert_eq!(k.killer_origin, Some([0.0, 0.0, 0.0]));
        assert_eq!(k.victim_origin, Some([300.0, 40.0, 0.0]));
        assert_eq!(k.distance, Some((300.0f32 * 300.0 + 40.0 * 40.0).sqrt()));
        assert_eq!(k.weapon, Weapon::K98);
        assert_eq!(k.killer_team, Some(Team::Allies));
        assert_eq!(k.victim_team, Some(Team::Axis));
        assert!(!k.teamkill);
        assert!(k.killer.as_ref().unwrap().to_string().ends_with("killer"));
        assert!(k.victim.as_ref().unwrap().to_string().ends_with("victim"));
    }

    #[test]
    fn a_suicide_has_a_death_position_and_no_distance() {
        let mut state = state_with(&[(0, "a", Team::Allies)]);
        snapshot(&mut state, 1, full(&[(1, at(5.0, 6.0, 7.0))]));
        death(&mut state, 0, 1, Weapon::Mk2Grenade);
        death(&mut state, 1, 1, Weapon::Mk2Grenade);

        for k in &state.kill_positions {
            assert_eq!(k.killer, None);
            assert_eq!(k.killer_origin, None);
            assert_eq!(k.victim_origin, Some([5.0, 6.0, 7.0]));
            assert_eq!(k.distance, None);
            assert!(!k.teamkill);
        }
        assert_eq!(state.kill_positions.len(), 2);
    }

    #[test]
    fn a_player_out_of_the_snapshot_has_no_position() {
        // A POV demo: the victim left the recording player's view before the
        // kill. Their last position is not reused.
        let mut state = state_with(&[(0, "k", Team::Allies), (1, "v", Team::Axis)]);
        snapshot(
            &mut state,
            1,
            full(&[(1, at(0.0, 0.0, 0.0)), (2, at(9.0, 9.0, 9.0))]),
        );
        snapshot(&mut state, 2, changes(1, &[(2, None)]));
        death(&mut state, 1, 2, Weapon::Garand);

        let k = &state.kill_positions[0];
        assert_eq!(k.killer_origin, Some([0.0, 0.0, 0.0]));
        assert_eq!(k.victim_origin, None);
        assert_eq!(k.distance, None);
    }

    #[test]
    fn the_recording_player_is_placed_from_client_data() {
        // POV demo recorded from slot 1 (entity 2). Its entity is in the
        // snapshot, but the server never sends it an origin.
        let mut state = state_with(&[(0, "k", Team::Allies), (1, "me", Team::Axis)]);
        state.pov_player_index = Some(1);
        snapshot(
            &mut state,
            1,
            full(&[(1, at(0.0, 0.0, 0.0)), (2, delta(&[("angles[1]", 90.0)]))]),
        );
        snapshot(&mut state, 1, client_data(None, at(10.0, 20.0, 30.0)));
        // Only x moves; y and z come from the frame it names.
        snapshot(
            &mut state,
            2,
            client_data(Some(1), delta(&[("origin[0]", 40.0)])),
        );
        death(&mut state, 1, 2, Weapon::Bar);

        let k = &state.kill_positions[0];
        assert_eq!(k.victim_origin, Some([40.0, 20.0, 30.0]));
        assert!(k.distance.is_some());
    }

    #[test]
    fn an_entity_without_an_origin_has_no_position() {
        // Not the recorder, but present with no origin ever sent: unknown,
        // not the world origin.
        let mut state = state_with(&[(0, "k", Team::Allies), (1, "v", Team::Axis)]);
        snapshot(
            &mut state,
            1,
            full(&[(1, at(5.0, 5.0, 5.0)), (2, delta(&[("angles[1]", 90.0)]))]),
        );
        death(&mut state, 1, 2, Weapon::Bar);
        assert_eq!(state.kill_positions[0].victim_origin, None);
        assert_eq!(state.kill_positions[0].distance, None);
    }

    #[test]
    fn flags_a_teamkill() {
        let mut state = state_with(&[(0, "a", Team::Axis), (1, "b", Team::Axis)]);
        snapshot(
            &mut state,
            1,
            full(&[(1, at(0.0, 0.0, 0.0)), (2, at(1.0, 0.0, 0.0))]),
        );
        death(&mut state, 1, 2, Weapon::Mp40);
        assert!(state.kill_positions[0].teamkill);
        assert_eq!(state.kill_positions[0].distance, Some(1.0));
    }
}
