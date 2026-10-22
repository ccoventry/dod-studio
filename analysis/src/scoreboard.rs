use crate::{AnalyzerEvent, AnalyzerState, time::GameTime};
use dod::{Team, UserMessage};
use std::collections::HashMap;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TeamScores {
    current_scores: HashMap<Team, i32>,
    timeline: Vec<(GameTime, Team, i32)>,
}

impl TeamScores {
    pub fn get_team_score(&self, team: Team) -> i32 {
        self.timeline
            .iter()
            .rfind(|(_, t, _)| *t == team)
            .map(|(_, _, points)| *points)
            .unwrap_or(0)
    }

    pub fn add_team_score(&mut self, game_time: GameTime, team: Team, points: i32) {
        self.timeline.push((game_time, team, points));
    }

    pub fn iter(&self) -> impl Iterator<Item = &(GameTime, Team, i32)> {
        self.timeline.iter()
    }

    pub(crate) fn reset(&mut self) {
        self.current_scores.clear();
        self.timeline.clear();
    }

    pub(crate) fn convert_allies_to_british(&mut self) {
        for (_, team, _) in &mut self.timeline {
            if *team == Team::Allies {
                *team = Team::British;
            }
        }
        if let Some(val) = self.current_scores.remove(&Team::Allies) {
            self.current_scores.insert(Team::British, val);
        }
    }
}

pub fn use_scoreboard_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    match event {
        AnalyzerEvent::UserMessage(UserMessage::PClass(p_class)) => {
            let player = state.find_player_by_client_index_mut(p_class.client_index - 1);

            if let Some(player) = player {
                player.class = Some(p_class.class.clone());
            };
        }

        AnalyzerEvent::UserMessage(UserMessage::PTeam(p_team)) => {
            let allies_are_british = state.allies_are_british;
            let player = state.find_player_by_client_index_mut(p_team.client_index - 1);

            if let Some(player) = player {
                let mut team = p_team.team.clone();
                if team == Team::Allies && allies_are_british {
                    team = Team::British;
                }
                player.team = Some(team);
            };
        }

        AnalyzerEvent::UserMessage(UserMessage::ScoreShort(score_short)) => {
            let player = state.find_player_by_client_index_mut(score_short.client_index - 1);

            if let Some(player) = player {
                player.update_session_stats(
                    score_short.score as i32,
                    score_short.kills as i32,
                    score_short.deaths as i32,
                );
            }
        }

        AnalyzerEvent::UserMessage(UserMessage::ScoreInfo(score_info)) => {
            let allies_are_british = state.allies_are_british;
            let player = state.find_player_by_client_index_mut(score_info.client_index - 1);

            if let Some(player) = player {
                let mut team = score_info.team.clone();
                if team == Team::Allies && allies_are_british {
                    team = Team::British;
                }
                player.class = Some(score_info.class.clone());
                player.team = Some(team);
                player.update_session_stats(
                    score_info.points as i32,
                    score_info.kills as i32,
                    score_info.deaths as i32,
                );
            }
        }

        AnalyzerEvent::UserMessage(UserMessage::ScoreInfoLong(score_info_long)) => {
            let allies_are_british = state.allies_are_british;
            let player = state.find_player_by_client_index_mut(score_info_long.client_index - 1);

            if let Some(player) = player {
                let mut team = score_info_long.team.clone();
                if team == Team::Allies && allies_are_british {
                    team = Team::British;
                }
                player.class = Some(score_info_long.class.clone());
                player.team = Some(team);
                player.update_session_stats(
                    score_info_long.score as i32,
                    score_info_long.frags as i32,
                    score_info_long.deaths as i32,
                );
            }
        }

        AnalyzerEvent::UserMessage(UserMessage::ObjScore(obj_score)) => {
            let player = state.find_player_by_client_index_mut(obj_score.client_index - 1);

            if let Some(player) = player {
                player.update_obj_score(obj_score.score as i32);
            }
        }

        AnalyzerEvent::UserMessage(UserMessage::Frags(frags)) => {
            let player = state.find_player_by_client_index_mut(frags.client_index - 1);

            if let Some(player) = player {
                player.update_frags(frags.frags as i32);
            }
        }

        AnalyzerEvent::Finalization => {
            state
                .players
                .sort_by(|left, right| match (&left.team, &right.team) {
                    (Some(left_team), Some(right_team)) if left_team == right_team => {
                        let by_points = left.stats.0.cmp(&right.stats.0).reverse();
                        let by_kills = left.stats.1.cmp(&right.stats.1).reverse();
                        let by_deaths = left.stats.2.cmp(&right.stats.2);

                        by_points.then(by_kills).then(by_deaths)
                    }

                    (left_team, right_team) => {
                        let rank = |team: &Option<Team>| match team {
                            Some(Team::Allies) | Some(Team::British) => 0,
                            Some(Team::Axis) => 1,
                            Some(Team::Spectators) => 2,
                            Some(Team::Unassigned) => 3,
                            None => 4,
                        };
                        rank(left_team).cmp(&rank(right_team))
                    }
                });
        }

        _ => {}
    };
}

pub fn use_team_score_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    if let AnalyzerEvent::UserMessage(UserMessage::TeamScore(team_score)) = event {
        let mut team = team_score.team.clone();
        if team == Team::Allies && state.allies_are_british {
            team = Team::British;
        }
        state
            .team_scores
            .add_team_score(state.current_time.clone(), team, team_score.score as i32);
    }
}
