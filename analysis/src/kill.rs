use crate::{
    AnalyzerEvent, AnalyzerState, mortality::MortalityState, player::PlayerGlobalId, time::GameTime,
};
use dod::{RoundState, Team, UserMessage, Weapon};

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct KillStreak {
    pub kills: Vec<(GameTime, Weapon, PlayerGlobalId)>,
}

pub fn use_kill_streak_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    if let AnalyzerEvent::UserMessage(UserMessage::DeathMsg(death_msg)) = event {
        let current_time = state.current_time.clone();

        let killer = if death_msg.killer_client_index > 0 {
            state.find_player_by_client_index(death_msg.killer_client_index - 1)
        } else {
            None
        };
        let victim = state.find_player_by_client_index(death_msg.victim_client_index - 1);

        let is_teamkill = match (killer, victim) {
            (Some(fst), Some(snd)) => fst.team == snd.team,
            _ => false,
        };

        let victim_id = victim.map(|v| v.id.clone());

        let victim = state.find_player_by_client_index_mut(death_msg.victim_client_index - 1);

        if let Some(victim) = victim {
            // End the victim's current streak by adding a new record
            victim.kill_streaks.push(KillStreak::default());
        }

        if is_teamkill {
            return;
        }

        let killer = if death_msg.killer_client_index > 0 {
            state.find_player_by_client_index_mut(death_msg.killer_client_index - 1)
        } else {
            None
        };

        if let (Some(killer), Some(v_id)) = (killer, victim_id) {
            if killer.kill_streaks.is_empty() {
                killer.kill_streaks.push(KillStreak::default());
            }

            let streak = if killer.is_dead() && death_msg.weapon.is_grenade() {
                if killer.kill_streaks.len() >= 2 {
                    let prev_streak_index = killer.kill_streaks.len() - 2;
                    killer.kill_streaks.get_mut(prev_streak_index)
                } else {
                    killer.kill_streaks.iter_mut().last()
                }
            } else {
                killer.kill_streaks.iter_mut().last()
            };

            if let Some(streak) = streak {
                streak
                    .kills
                    .push((current_time, death_msg.weapon.clone(), v_id));
            }
        }
    } else if let AnalyzerEvent::UserMessage(UserMessage::RoundState(RoundState::Reset)) = event {
        // Active kill streaks must be terminated when round is reset (i.e., after all objectives are captured)
        for player in state.players.iter_mut() {
            player.kill_streaks.push(KillStreak::default());
        }
    }
}

pub fn use_weapon_breakdown_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    if let AnalyzerEvent::UserMessage(UserMessage::DeathMsg(death_msg)) = event {
        let killer = if death_msg.killer_client_index > 0 {
            state.find_player_by_client_index(death_msg.killer_client_index - 1)
        } else {
            None
        };
        let victim = state.find_player_by_client_index(death_msg.victim_client_index - 1);

        let is_teamkill = match (killer, victim) {
            (Some(fst), Some(snd)) => fst.team == snd.team,
            _ => false,
        };

        let killer = if death_msg.killer_client_index > 0 {
            state.find_player_by_client_index_mut(death_msg.killer_client_index - 1)
        } else {
            None
        };

        if let Some(killer) = killer {
            let (kills, teamkills) = killer
                .weapon_breakdown
                .entry(death_msg.weapon.clone())
                .or_insert((0, 0));

            if is_teamkill {
                *teamkills += 1;
            } else {
                *kills += 1;
            }
        }
    }
}

/// Per-player teamkill and suicide counts (#192).
///
/// A death with killer index 0 (the world) or the victim's own index is the
/// victim's suicide. Otherwise it is the killer's teamkill when both players'
/// teams, as they stand at the moment of the kill, are the same playing team.
/// Resolving at the moment matters: a demo that spans a side swap would
/// otherwise turn ordinary kills into teamkills.
pub fn use_teamkill_and_suicide_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    let AnalyzerEvent::UserMessage(UserMessage::DeathMsg(death_msg)) = event else {
        return;
    };
    let Some(victim_slot) = death_msg.victim_client_index.checked_sub(1) else {
        return;
    };

    if death_msg.killer_client_index == 0
        || death_msg.killer_client_index == death_msg.victim_client_index
    {
        if let Some(victim) = state.find_player_by_client_index_mut(victim_slot) {
            victim.suicides += 1;
        }
        return;
    }

    let killer_slot = death_msg.killer_client_index - 1;
    let team_of = |slot| {
        state
            .find_player_by_client_index(slot)
            .and_then(|p| p.team.clone())
    };
    let is_teamkill = match (team_of(killer_slot), team_of(victim_slot)) {
        (Some(killer), Some(victim)) => {
            killer == victim && matches!(killer, Team::Allies | Team::British | Team::Axis)
        }
        _ => false,
    };
    if is_teamkill && let Some(killer) = state.find_player_by_client_index_mut(killer_slot) {
        killer.teamkills += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Player;

    fn death(state: &mut AnalyzerState, killer: u8, victim: u8) {
        // DeathMsg is u8 killer, u8 victim, u8 weapon (10 = K98).
        let Ok(msg) = UserMessage::new(b"DeathMsg", &[killer, victim, 10]) else {
            panic!("DeathMsg did not parse");
        };
        use_teamkill_and_suicide_updates(state, &AnalyzerEvent::UserMessage(msg));
    }

    #[test]
    fn teamkills_and_suicides_are_counted_per_player() {
        let mut state = AnalyzerState::default();
        for (slot, name, team) in [
            (0, "axis1", Team::Axis),
            (1, "axis2", Team::Axis),
            (2, "allies1", Team::Allies),
        ] {
            let mut player = Player::new_mock(slot, name);
            player.team = Some(team);
            state.players.push(player);
        }

        death(&mut state, 1, 2); // axis1 kills axis2: a teamkill
        death(&mut state, 1, 3); // axis1 kills allies1: a kill
        death(&mut state, 0, 3); // the world kills allies1: a suicide
        death(&mut state, 2, 2); // axis2 kills themselves: a suicide

        let stats: Vec<_> = state
            .players
            .iter()
            .map(|p| (p.name.as_str(), p.teamkills, p.suicides))
            .collect();
        assert_eq!(stats, [("axis1", 1, 0), ("axis2", 0, 1), ("allies1", 0, 1)]);
    }

    #[test]
    fn a_kill_with_an_unknown_team_is_not_a_teamkill() {
        let mut state = AnalyzerState::default();
        state.players.push(Player::new_mock(0, "a"));
        state.players.push(Player::new_mock(1, "b"));
        death(&mut state, 1, 2);
        assert_eq!(state.players[0].teamkills, 0);
    }
}
