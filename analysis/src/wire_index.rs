//! The player number a demo message carries.
//!
//! Messages number players from 1, and 0 means no player (the world, or a
//! damaged demo). The analyzer's own slots start at 0.

/// The 0-based slot a message's client index names, or `None` for index 0.
/// Subtracting by hand would panic in a debug build on a damaged demo.
pub(crate) fn wire_slot(client_index: u8) -> Option<u8> {
    client_index.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::wire_slot;
    use crate::{AnalyzerEvent, AnalyzerState, Player, chat, kill, mortality, position, round};
    use dod::{Team, UserMessage};

    fn send(state: &mut AnalyzerState, name: &[u8], bytes: &[u8]) {
        let Ok(msg) = UserMessage::new(name, bytes) else {
            panic!("{} did not parse", String::from_utf8_lossy(name));
        };
        let event = AnalyzerEvent::UserMessage(msg);
        crate::scoreboard::use_scoreboard_updates(state, &event);
        chat::use_chat_updates(state, &event);
        kill::use_kill_streak_updates(state, &event);
        kill::use_weapon_breakdown_updates(state, &event);
        kill::use_teamkill_and_suicide_updates(state, &event);
        mortality::with_mortality_detection(state, &event);
        position::use_position_updates(state, &event);
        round::use_rounds_updates(state, &event);
    }

    fn state_with_slot_zero() -> AnalyzerState {
        let mut state = AnalyzerState::default();
        state.players.push(Player::new_mock(0, "first"));
        state
    }

    #[test]
    fn index_zero_has_no_slot_and_index_one_is_slot_zero() {
        assert_eq!(wire_slot(0), None);
        assert_eq!(wire_slot(1), Some(0));
        assert_eq!(wire_slot(255), Some(254));
    }

    #[test]
    fn score_and_team_messages_naming_player_zero_change_nobody() {
        let mut state = state_with_slot_zero();
        send(&mut state, b"PTeam", &[0, 1]);
        send(&mut state, b"Frags", &[0, 5, 0]);
        assert_eq!(state.players[0].team, None);
        assert_eq!(state.players[0].session_stats, (0, 0, 0));

        send(&mut state, b"PTeam", &[1, 1]);
        send(&mut state, b"Frags", &[1, 5, 0]);
        assert_eq!(state.players[0].team, Some(Team::Allies));
        assert_eq!(state.players[0].session_stats.1, 5);
    }

    #[test]
    fn death_messages_naming_player_zero_attribute_nothing() {
        let mut state = state_with_slot_zero();
        for (killer, victim) in [(0, 0), (0, 1), (0, 2)] {
            send(&mut state, b"DeathMsg", &[killer, victim, 10]);
            send(&mut state, b"PStatus", &[victim, 0]);
        }
        let player = &state.players[0];
        // Only the victim-1 deaths count; index 0 never reaches this player.
        assert_eq!(player.kill_streaks.len(), 2);
        assert!(player.kill_streaks.iter().all(|s| s.kills.is_empty()));
        assert!(player.weapon_breakdown.is_empty());
        assert_eq!(player.teamkills, 0);
    }

    #[test]
    fn a_kill_by_index_one_is_credited_to_slot_zero() {
        let mut state = state_with_slot_zero();
        let mut second = Player::new_mock(1, "second");
        second.team = Some(Team::Axis);
        state.players.push(second);
        state.players[0].team = Some(Team::Allies);
        send(&mut state, b"DeathMsg", &[1, 2, 10]);
        assert_eq!(state.players[0].kill_streaks[0].kills.len(), 1);
        assert_eq!(state.players[0].kill_streaks[0].slot, Some(0));
    }

    #[test]
    fn chat_from_player_zero_has_no_sender() {
        let mut state = state_with_slot_zero();
        send(
            &mut state,
            b"SayText",
            &[
                0, b'f', b'i', b'r', b's', b't', b' ', b':', b' ', b'h', b'i', 0,
            ],
        );
        assert_eq!(
            state.chat_messages[0].sender_name.as_deref(),
            Some("Console/Server")
        );

        send(
            &mut state,
            b"SayText",
            &[
                1, b'f', b'i', b'r', b's', b't', b' ', b':', b' ', b'h', b'i', 0,
            ],
        );
        assert_eq!(state.chat_messages[1].sender_name.as_deref(), Some("first"));
    }
}
