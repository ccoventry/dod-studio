//! Allies become British for the whole analysis once any player picks a
//! British class.

use crate::{AnalyzerState, Round, Team};

pub(crate) fn check_and_promote_british(state: &mut AnalyzerState) {
    if state.allies_are_british {
        for player in &mut state.players {
            if player.team == Some(Team::Allies) {
                player.team = Some(Team::British);
            }
        }
    } else {
        let any_british = state
            .players
            .iter()
            .any(|p| p.class.as_ref().map(|c| c.is_british()).unwrap_or(false));
        if any_british {
            state.allies_are_british = true;
            for player in &mut state.players {
                if player.team == Some(Team::Allies) {
                    player.team = Some(Team::British);
                }
            }
            state.team_scores.convert_allies_to_british();
            state.objectives.convert_allies_to_british();
            for round in &mut state.rounds {
                if let Round::Completed {
                    winner_stats: Some((winner_team, _)),
                    ..
                } = round
                    && *winner_team == Team::Allies
                {
                    *winner_team = Team::British;
                }
            }
            for chat in &mut state.chat_messages {
                if chat.sender_team == Some(Team::Allies) {
                    chat.sender_team = Some(Team::British);
                }
            }
            for kill in &mut state.kill_positions {
                for team in [&mut kill.killer_team, &mut kill.victim_team] {
                    if *team == Some(Team::Allies) {
                        *team = Some(Team::British);
                    }
                }
            }
        }
    }
}
