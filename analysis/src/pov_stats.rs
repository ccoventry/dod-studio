//! The recorder's own stats in a POV demo: shots fired, reloads, damage taken,
//! kills by weapon, scoped or not.

use crate::{AnalyzerEvent, AnalyzerState, wire_index::wire_slot};
use dem::types::EngineMessage;
use dod::{UserMessage, Weapon};

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WeaponPovStats {
    pub bullets_fired: u32,
    pub reloads: u32,
    pub kills: u32,
    pub noscopes: u32,
    pub scoped_kills: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PovStats {
    pub is_scoped: bool,
    pub hits_taken: u32,
    pub total_damage_taken: u32,
    pub suicides: u32,
    pub teamkills_committed: u32,
    pub teamkills_suffered: u32,
    pub weapon_stats: std::collections::HashMap<Weapon, WeaponPovStats>,

    // Tracking state
    pub current_weapon: Option<Weapon>,
    pub prev_clip_ammo: u32,
    pub prev_health: u32,
    pub has_received_health: bool,
}

pub fn use_pov_stats_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    if let AnalyzerEvent::EngineMessage(EngineMessage::SvcServerInfo(msg)) = event {
        state.pov_player_index = Some(msg.player_index);
    }

    if let AnalyzerEvent::UserMessage(user_msg) = event {
        match user_msg {
            UserMessage::Scope(_) => {
                state.pov_stats.is_scoped = !state.pov_stats.is_scoped;
            }
            UserMessage::CurWeapon(msg) => {
                if let Some(ref prev_weapon) = state.pov_stats.current_weapon {
                    if prev_weapon == &msg.weapon {
                        if (msg.clip_ammo as u32) < state.pov_stats.prev_clip_ammo {
                            let entry = state
                                .pov_stats
                                .weapon_stats
                                .entry(msg.weapon.clone())
                                .or_default();
                            entry.bullets_fired += 1;
                        }
                    } else {
                        state.pov_stats.is_scoped = false;
                    }
                } else {
                    state.pov_stats.is_scoped = false;
                }
                state.pov_stats.current_weapon = Some(msg.weapon.clone());
                state.pov_stats.prev_clip_ammo = msg.clip_ammo as u32;
            }
            UserMessage::ReloadDone(_) => {
                if let Some(ref active_weapon) = state.pov_stats.current_weapon {
                    let entry = state
                        .pov_stats
                        .weapon_stats
                        .entry(active_weapon.clone())
                        .or_default();
                    entry.reloads += 1;
                }
                state.pov_stats.is_scoped = false;
            }
            UserMessage::ResetHUD(_) => {
                state.pov_stats.is_scoped = false;
                state.pov_stats.has_received_health = false;
            }
            UserMessage::Health(msg) => {
                let health_val = msg.0 as u32;
                if state.pov_stats.has_received_health && health_val < state.pov_stats.prev_health {
                    state.pov_stats.hits_taken += 1;
                    state.pov_stats.total_damage_taken += state.pov_stats.prev_health - health_val;
                }
                state.pov_stats.prev_health = health_val;
                state.pov_stats.has_received_health = true;
            }
            UserMessage::DeathMsg(msg) => {
                if let Some(pov_idx) = state.pov_player_index {
                    let is_killer_pov = wire_slot(msg.killer_client_index) == Some(pov_idx);
                    let is_victim_pov = wire_slot(msg.victim_client_index) == Some(pov_idx);

                    if is_killer_pov {
                        if msg.killer_client_index == msg.victim_client_index {
                            state.pov_stats.suicides += 1;
                        } else {
                            let killer_team = state
                                .find_player_by_wire_index(msg.killer_client_index)
                                .and_then(|p| p.team.clone());
                            let victim_team = state
                                .find_player_by_wire_index(msg.victim_client_index)
                                .and_then(|p| p.team.clone());
                            if killer_team.is_some()
                                && victim_team.is_some()
                                && killer_team == victim_team
                            {
                                state.pov_stats.teamkills_committed += 1;
                            } else {
                                let entry = state
                                    .pov_stats
                                    .weapon_stats
                                    .entry(msg.weapon.clone())
                                    .or_default();
                                entry.kills += 1;
                                if matches!(
                                    msg.weapon,
                                    Weapon::Springfield
                                        | Weapon::ScopedK98
                                        | Weapon::ScopedFg42
                                        | Weapon::ScopedLeeEnfield
                                ) {
                                    if state.pov_stats.is_scoped {
                                        entry.scoped_kills += 1;
                                    } else {
                                        entry.noscopes += 1;
                                    }
                                }
                            }
                        }
                    } else if msg.killer_client_index == 0 && is_victim_pov {
                        state.pov_stats.suicides += 1;
                    }

                    if is_victim_pov {
                        state.pov_stats.is_scoped = false;
                        if msg.killer_client_index > 0 && !is_killer_pov {
                            let killer_team = state
                                .find_player_by_wire_index(msg.killer_client_index)
                                .and_then(|p| p.team.clone());
                            let victim_team = state
                                .find_player_by_wire_index(msg.victim_client_index)
                                .and_then(|p| p.team.clone());
                            if killer_team.is_some()
                                && victim_team.is_some()
                                && killer_team == victim_team
                            {
                                state.pov_stats.teamkills_suffered += 1;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Player, Team};

    fn send(state: &mut AnalyzerState, message: UserMessage) {
        use_pov_stats_updates(state, &AnalyzerEvent::UserMessage(message));
    }

    fn cur_weapon(weapon: Weapon, clip_ammo: u8) -> UserMessage {
        UserMessage::CurWeapon(dod::CurWeapon {
            is_active: true,
            weapon,
            clip_ammo,
        })
    }

    fn death(killer: u8, victim: u8, weapon: Weapon) -> UserMessage {
        UserMessage::DeathMsg(dod::DeathMsg {
            killer_client_index: killer,
            victim_client_index: victim,
            weapon,
        })
    }

    /// The recorder is wire index 1 (Allies); 2 is a teammate, 3 an enemy.
    fn match_in_progress() -> AnalyzerState {
        let mut state = AnalyzerState {
            pov_player_index: Some(0),
            ..Default::default()
        };
        for (slot, name, team) in [
            (0, "pov", Team::Allies),
            (1, "mate", Team::Allies),
            (2, "enemy", Team::Axis),
        ] {
            let mut player = Player::new_mock(slot, name);
            player.team = Some(team);
            state.players.push(player);
        }
        state
    }

    #[test]
    fn a_falling_clip_is_a_bullet_fired_and_a_new_weapon_clears_the_scope() {
        let mut state = match_in_progress();
        send(&mut state, cur_weapon(Weapon::Springfield, 5));
        send(&mut state, UserMessage::Scope(dod::Scope {}));
        assert!(state.pov_stats.is_scoped);
        send(&mut state, cur_weapon(Weapon::Springfield, 4));
        send(&mut state, cur_weapon(Weapon::Springfield, 4));
        let fired = &state.pov_stats.weapon_stats[&Weapon::Springfield];
        assert_eq!(fired.bullets_fired, 1);
        assert!(state.pov_stats.is_scoped);

        send(&mut state, cur_weapon(Weapon::Kabar, 0));
        assert!(!state.pov_stats.is_scoped);
        assert_eq!(state.pov_stats.current_weapon, Some(Weapon::Kabar));
    }

    #[test]
    fn a_reload_counts_for_the_held_weapon_and_clears_the_scope() {
        let mut state = match_in_progress();
        send(&mut state, UserMessage::ReloadDone(dod::ReloadDone {}));
        assert!(state.pov_stats.weapon_stats.is_empty());
        send(&mut state, cur_weapon(Weapon::K98, 5));
        send(&mut state, UserMessage::Scope(dod::Scope {}));
        send(&mut state, UserMessage::ReloadDone(dod::ReloadDone {}));
        assert_eq!(state.pov_stats.weapon_stats[&Weapon::K98].reloads, 1);
        assert!(!state.pov_stats.is_scoped);
    }

    #[test]
    fn health_that_drops_is_a_hit_but_the_first_reading_and_a_respawn_are_not() {
        let mut state = match_in_progress();
        send(&mut state, UserMessage::Health(dod::Health(100)));
        send(&mut state, UserMessage::Health(dod::Health(70)));
        send(&mut state, UserMessage::Health(dod::Health(80)));
        send(&mut state, UserMessage::Health(dod::Health(75)));
        assert_eq!(state.pov_stats.hits_taken, 2);
        assert_eq!(state.pov_stats.total_damage_taken, 35);

        send(&mut state, UserMessage::ResetHUD(dod::ResetHUD {}));
        send(&mut state, UserMessage::Health(dod::Health(100)));
        assert_eq!(state.pov_stats.hits_taken, 2);
        assert!(!state.pov_stats.is_scoped);
    }

    #[test]
    fn a_kill_of_an_enemy_counts_for_the_weapon_scoped_or_not() {
        let mut state = match_in_progress();
        send(&mut state, death(1, 3, Weapon::Springfield));
        let stats = &state.pov_stats.weapon_stats[&Weapon::Springfield];
        assert_eq!((stats.kills, stats.noscopes, stats.scoped_kills), (1, 1, 0));

        send(&mut state, UserMessage::Scope(dod::Scope {}));
        send(&mut state, death(1, 3, Weapon::Springfield));
        send(&mut state, death(1, 3, Weapon::Garand));
        let stats = &state.pov_stats.weapon_stats[&Weapon::Springfield];
        assert_eq!((stats.kills, stats.noscopes, stats.scoped_kills), (2, 1, 1));
        let garand = &state.pov_stats.weapon_stats[&Weapon::Garand];
        assert_eq!(
            (garand.kills, garand.noscopes, garand.scoped_kills),
            (1, 0, 0)
        );
    }

    #[test]
    fn killing_a_teammate_or_oneself_is_not_a_kill() {
        let mut state = match_in_progress();
        send(&mut state, death(1, 2, Weapon::Garand));
        assert_eq!(state.pov_stats.teamkills_committed, 1);
        send(&mut state, death(1, 1, Weapon::Garand));
        assert_eq!(state.pov_stats.suicides, 1);
        assert!(state.pov_stats.weapon_stats.is_empty());
    }

    #[test]
    fn a_world_death_of_the_recorder_is_a_suicide() {
        let mut state = match_in_progress();
        send(&mut state, UserMessage::Scope(dod::Scope {}));
        send(&mut state, death(0, 1, Weapon::Unknown));
        assert_eq!(state.pov_stats.suicides, 1);
        assert!(!state.pov_stats.is_scoped);
        send(&mut state, death(0, 3, Weapon::Unknown));
        assert_eq!(state.pov_stats.suicides, 1);
    }

    #[test]
    fn dying_to_a_teammate_is_suffered_and_to_an_enemy_is_not() {
        let mut state = match_in_progress();
        send(&mut state, UserMessage::Scope(dod::Scope {}));
        send(&mut state, death(3, 1, Weapon::Garand));
        assert_eq!(state.pov_stats.teamkills_suffered, 0);
        assert!(!state.pov_stats.is_scoped);
        send(&mut state, death(2, 1, Weapon::Garand));
        assert_eq!(state.pov_stats.teamkills_suffered, 1);
        // Other players' deaths change nothing.
        send(&mut state, death(2, 3, Weapon::Garand));
        assert_eq!(state.pov_stats.teamkills_suffered, 1);
        assert_eq!(state.pov_stats.teamkills_committed, 0);
    }

    #[test]
    fn deaths_before_the_recorder_is_known_are_ignored() {
        let mut state = match_in_progress();
        state.pov_player_index = None;
        send(&mut state, death(1, 3, Weapon::Garand));
        assert!(state.pov_stats.weapon_stats.is_empty());
    }
}
