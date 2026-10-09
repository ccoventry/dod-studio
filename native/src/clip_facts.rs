//! What a clip name can say about a highlight that the streak itself doesn't
//! carry (#441): the killer's side, and each victim's name and side.
//!
//! `CaptureStreak.kills` is `(tick, time, weapon)`, which the patcher uses and
//! which stays as it is. These facts ride beside it on the Studio side only.

use analysis::{Analysis, Connection, Team};

/// A highlight's facts for its clip name, one victim entry per kill.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreakFacts {
    /// "Allies", "British", "Axis", "Spectator" or "Unknown".
    pub faction: String,
    pub victims: Vec<String>,
    pub victim_factions: Vec<String>,
}

/// A side as a clip name spells it. `allies_are_british` covers a demo
/// whose Allies are the British without the team itself saying so.
pub fn faction_name(team: Option<&Team>, allies_are_british: bool) -> &'static str {
    match team {
        Some(Team::Allies) if allies_are_british => "British",
        Some(Team::Allies) => "Allies",
        Some(Team::British) => "British",
        Some(Team::Axis) => "Axis",
        Some(Team::Spectators) => "Spectator",
        Some(Team::Unassigned) | None => "Unknown",
    }
}

/// The facts for the streak `player_index` (a connected client slot, as
/// `CaptureStreak.player_index` holds it) started at `first_kill_tick`, or
/// `None` when no such streak is in the analysis.
///
/// Sides are each player's team at the end of the demo, which is the side
/// they played for a demo that is one half of a match.
pub fn streak_facts(
    analysis: &Analysis,
    player_index: usize,
    first_kill_tick: i32,
) -> Option<StreakFacts> {
    let british = analysis.state.allies_are_british;
    let players = &analysis.state.players;
    let player = players.iter().find(|p| {
        matches!(p.connection, Connection::Connected { client_id } if client_id as usize == player_index)
    })?;
    let streak = player.kill_streaks.iter().find(|s| {
        s.kills
            .first()
            .is_some_and(|(time, _, _)| time.frame_index as i32 == first_kill_tick)
    })?;
    let mut facts = StreakFacts {
        faction: faction_name(player.team.as_ref(), british).to_string(),
        ..StreakFacts::default()
    };
    for (_, _, victim_id) in &streak.kills {
        let victim = players.iter().find(|p| &p.id == victim_id);
        facts
            .victims
            .push(victim.map(|v| v.name.clone()).unwrap_or_default());
        facts
            .victim_factions
            .push(faction_name(victim.and_then(|v| v.team.as_ref()), british).to_string());
    }
    Some(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faction_names() {
        assert_eq!(faction_name(Some(&Team::Allies), false), "Allies");
        assert_eq!(faction_name(Some(&Team::Allies), true), "British");
        assert_eq!(faction_name(Some(&Team::British), false), "British");
        assert_eq!(faction_name(Some(&Team::Axis), true), "Axis");
        assert_eq!(faction_name(Some(&Team::Spectators), false), "Spectator");
        assert_eq!(faction_name(Some(&Team::Unassigned), false), "Unknown");
        assert_eq!(faction_name(None, false), "Unknown");
    }
}
