//! What a highlight is, once, for everything that lists them (#573): the
//! Capture page's Highlights table (through `native::patch::scanner`) and the
//! in-game DoD Studio window's Highlights tab.
//!
//! A highlight is one life's kills by one player: `Player::kill_streaks`, which
//! `kill.rs` already cuts at each death, round reset and map change (a grenade
//! thrown before dying still counts for the life it was thrown in).
//!
//! **Whose** ([`Rule`]): a POV demo's footage follows its recorder, so only
//! the recorder's highlights are worth listing; an HLTV demo follows no one,
//! so everyone's are (#247's decision D18). [`Highlights::shown`] applies it;
//! [`Highlights::all`] keeps every player's for a caller that filters itself.

use crate::{Analysis, Connection, Weapon};

/// One kill in a highlight.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightKill {
    /// The 1-based frame the kill was recorded in: what capture turns into a
    /// patcher tick.
    pub frame_index: usize,
    /// Seconds from the start of the recording.
    pub real_secs: f32,
    /// Seconds on the `viewdemo` bar (what `dodstudio_seek_to` takes).
    pub viewdemo_secs: f32,
    pub weapon: Weapon,
}

/// One life's kills by one player.
#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    pub player: String,
    /// The analysis' player id: a SteamID64 when the demo has one.
    pub player_id: String,
    /// The player's slot (0-based; their entity number is this + 1), while
    /// they were still in the game at the demo's end. Capture needs it.
    pub slot: Option<u8>,
    /// They recorded this POV demo.
    pub recorder: bool,
    /// At least one, in the order `kill.rs` recorded them.
    pub kills: Vec<HighlightKill>,
}

/// Whose highlights a list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A POV demo: the recorder's alone.
    Recorder,
    /// An HLTV demo, or a POV demo whose recorder can't be told: everyone's.
    Everyone,
}

/// Every highlight in a demo, and which of them to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Highlights {
    /// Every player's: players in the analysis' order, each one's highlights
    /// in the order they happened (the Capture page's row order, which saved
    /// projects index into).
    pub all: Vec<Highlight>,
    pub hltv: bool,
    /// A POV demo's recording player.
    pub recorder: Option<String>,
    pub rule: Rule,
}

impl Highlights {
    /// The highlights [`Rule`] keeps, in [`Highlights::all`]'s order.
    pub fn shown(&self) -> impl Iterator<Item = &Highlight> {
        self.all
            .iter()
            .filter(move |h| self.rule == Rule::Everyone || h.recorder)
    }
}

/// Every highlight in `analysis`.
pub fn highlights(analysis: &Analysis) -> Highlights {
    let hltv = analysis.demo_info.demo_type == "HLTV";
    let recorder_slot = analysis.state.pov_player_index;
    let all: Vec<Highlight> = analysis
        .state
        .players
        .iter()
        .flat_map(|player| {
            let slot = match player.connection {
                Connection::Connected { client_id } => Some(client_id),
                Connection::Disconnected => None,
            };
            let recorder = !hltv && slot.is_some() && slot == recorder_slot;
            player
                .kill_streaks
                .iter()
                .filter(|s| !s.kills.is_empty())
                .map(move |streak| {
                    let kills: Vec<HighlightKill> = streak
                        .kills
                        .iter()
                        .map(|(time, weapon, _victim)| HighlightKill {
                            frame_index: time.frame_index,
                            real_secs: time.real_offset.as_secs_f32(),
                            viewdemo_secs: time.viewdemo_offset.as_secs_f32(),
                            weapon: weapon.clone(),
                        })
                        .collect();
                    Highlight {
                        player: player.name.clone(),
                        player_id: player.id.to_string(),
                        slot,
                        recorder,
                        kills,
                    }
                })
        })
        .collect();
    let recorder = all
        .iter()
        .find(|h| h.recorder)
        .map(|h| h.player.clone())
        .or_else(|| {
            (!hltv)
                .then(|| {
                    analysis.state.players.iter().find(|p| {
                        matches!(p.connection, Connection::Connected { client_id }
                        if Some(client_id) == recorder_slot)
                    })
                })?
                .map(|p| p.name.clone())
        });
    let rule = rule_for(hltv, recorder.is_some());
    Highlights {
        all,
        hltv,
        recorder,
        rule,
    }
}

/// D18: POV demos list the recorder's highlights, HLTV demos everyone's, and a
/// POV demo whose recorder can't be told everyone's rather than none.
pub fn rule_for(hltv: bool, recorder_known: bool) -> Rule {
    if !hltv && recorder_known {
        Rule::Recorder
    } else {
        Rule::Everyone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlight(player: &str, recorder: bool, at: f32) -> Highlight {
        Highlight {
            player: player.to_string(),
            player_id: format!("id_{player}"),
            slot: Some(1),
            recorder,
            kills: vec![HighlightKill {
                frame_index: 1,
                real_secs: at,
                viewdemo_secs: at,
                weapon: Weapon::K98,
            }],
        }
    }

    #[test]
    fn a_pov_demo_shows_the_recorder_and_an_hltv_demo_everyone() {
        assert_eq!(rule_for(false, true), Rule::Recorder);
        assert_eq!(rule_for(true, false), Rule::Everyone);
        assert_eq!(rule_for(false, false), Rule::Everyone);

        let mut list = Highlights {
            all: vec![highlight("milo", false, 1.0), highlight("brain", true, 2.0)],
            hltv: false,
            recorder: Some("brain".to_string()),
            rule: Rule::Recorder,
        };
        let shown: Vec<_> = list.shown().map(|h| h.player.as_str()).collect();
        assert_eq!(shown, ["brain"]);
        list.rule = Rule::Everyone;
        assert_eq!(list.shown().count(), 2);
    }

    #[test]
    fn an_empty_analysis_has_no_highlights() {
        let found = highlights(&Analysis::default());
        assert!(found.all.is_empty());
        assert!(found.recorder.is_none());
        assert_eq!(found.rule, Rule::Everyone);
    }
}
