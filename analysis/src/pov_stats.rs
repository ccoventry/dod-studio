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
