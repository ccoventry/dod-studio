//! A side's clan tag, read from its players' names (#445).
//!
//! Demos don't carry clan names, but players tag their names. The tag is the
//! longest start (or end) that most of a side's names share, cleaned down to
//! letters, digits and single spaces. "Most" is at least 60% and at least 3
//! players, so one untagged ringer doesn't break it. Measured on the movie
//! install's analyzer cache (2026-09-28): a tag for 117 of 120 team rosters,
//! the 3 misses being mixed or untagged teams.

use crate::AnalyzerState;
use dod::Team;
use std::collections::HashMap;

/// Fewest names that must share a part for it to count.
const MIN_PLAYERS: usize = 3;
/// Share of a side's names that must share a part for it to count.
const MIN_SHARE: f64 = 0.6;
/// A cleaned part shorter than this (in characters) is not a tag.
const MIN_TAG_CHARS: usize = 2;

/// One playing side of a demo and the tag found on it, if any.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TeamTag {
    /// "Allies", "British" or "Axis".
    pub side: String,
    pub tag: Option<String>,
}

/// The tag shared by most of `names`, or `None` when there isn't one.
///
/// Identical names count once. When both a shared start and a shared end
/// qualify, the longer cleaned one wins, the start on a tie.
pub fn detect_team_tag<S: AsRef<str>>(names: &[S]) -> Option<String> {
    let mut unique: Vec<Vec<char>> = Vec::new();
    for name in names {
        let chars: Vec<char> = name.as_ref().chars().collect();
        if !unique.contains(&chars) {
            unique.push(chars);
        }
    }
    let needed = MIN_PLAYERS.max((unique.len() as f64 * MIN_SHARE).ceil() as usize);
    if unique.len() < needed {
        return None;
    }

    let start = clean_tag(&shared_start(&unique, needed));
    let reversed: Vec<Vec<char>> = unique
        .iter()
        .map(|n| n.iter().rev().copied().collect())
        .collect();
    let mut end_chars = shared_start(&reversed, needed);
    end_chars.reverse();
    let end = clean_tag(&end_chars);

    let start_len = start.chars().count();
    let end_len = end.chars().count();
    if start_len >= MIN_TAG_CHARS && start_len >= end_len {
        Some(start)
    } else if end_len >= MIN_TAG_CHARS {
        Some(end)
    } else {
        None
    }
}

/// The longest start at least `needed` of `names` share. Since `needed` is
/// over half, at most one group of names can reach it at each length.
fn shared_start(names: &[Vec<char>], needed: usize) -> Vec<char> {
    let mut best: &[char] = &[];
    let longest = names.iter().map(Vec::len).max().unwrap_or(0);
    for len in 1..=longest {
        let mut counts: HashMap<&[char], usize> = HashMap::new();
        for name in names.iter().filter(|n| n.len() >= len) {
            *counts.entry(&name[..len]).or_default() += 1;
        }
        match counts.into_iter().find(|&(_, count)| count >= needed) {
            Some((start, _)) => best = start,
            None => break,
        }
    }
    best.to_vec()
}

/// Keeps letters (accented ones too), digits and single spaces, drops
/// everything else, and trims: `[g/s/k/i/L/L]_` gives `gskiLL`.
pub fn clean_tag(part: &[char]) -> String {
    let mut out = String::new();
    for &c in part {
        if c.is_alphanumeric() {
            out.push(c);
        } else if c.is_whitespace() && !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// Each playing side of the demo with its tag, Allied side first. Sides are
/// each player's team at the end of the demo; a side nobody played on is
/// left out.
pub fn team_tags(state: &AnalyzerState) -> Vec<TeamTag> {
    let mut allied: Vec<&str> = Vec::new();
    let mut axis: Vec<&str> = Vec::new();
    let mut british = state.allies_are_british;
    for player in &state.players {
        match player.team {
            Some(Team::Allies) => allied.push(&player.name),
            Some(Team::British) => {
                british = true;
                allied.push(&player.name);
            }
            Some(Team::Axis) => axis.push(&player.name),
            _ => {}
        }
    }
    let mut tags = Vec::new();
    if !allied.is_empty() {
        tags.push(TeamTag {
            side: if british { "British" } else { "Allies" }.to_string(),
            tag: detect_team_tag(&allied),
        });
    }
    if !axis.is_empty() {
        tags.push(TeamTag {
            side: "Axis".to_string(),
            tag: detect_team_tag(&axis),
        });
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(names: &[&str]) -> Option<String> {
        detect_team_tag(names)
    }

    #[test]
    fn the_issue_examples() {
        assert_eq!(
            tag(&[
                "dicE[: :]warchyld[dd]",
                "dicE[: :]m00cat :D",
                "dicE[: :]Hobo",
                "dicE[: :]zeph",
            ]),
            Some("dicE".into())
        );
        assert_eq!(
            tag(&["-={TEK}=- Gorilla[bc]", "-={TEK}=- Khoi", "-={TEK}=- Rast"]),
            Some("TEK".into())
        );
        assert_eq!(
            tag(&["choochoo ° baronn", "choochoo ° TillJim", "choochoo ° Wez"]),
            Some("choochoo".into())
        );
        assert_eq!(
            tag(&[
                "r3c0i1 has bad manners",
                "cory has bad manners| rip rdk",
                "krod has bad manners",
                "Pyro has bad manners",
            ]),
            Some("has bad manners".into())
        );
    }

    #[test]
    fn the_majority_spelling_wins_and_accents_are_kept() {
        assert_eq!(
            tag(&[
                "-#över. joniii",
                "-#over. KumKum",
                "-#over. Zed",
                "-#over. Bo"
            ]),
            Some("over".into())
        );
        assert_eq!(
            tag(&[
                "-#över. joniii",
                "-#över. KumKum",
                "-#över. Zed",
                "-#over. Bo"
            ]),
            Some("över".into())
        );
    }

    #[test]
    fn one_untagged_ringer_does_not_break_it() {
        assert_eq!(
            tag(&[
                "[g/s/k/i/L/L]_a",
                "[g/s/k/i/L/L]_b",
                "[g/s/k/i/L/L]_c",
                "ringer",
                "[g/s/k/i/L/L]_d"
            ]),
            Some("gskiLL".into())
        );
    }

    #[test]
    fn needs_sixty_percent_and_three_players() {
        // Two of two share it, but two is not enough players.
        assert_eq!(tag(&["[AB] x", "[AB] y"]), None);
        // Three of six is half, under 60%.
        assert_eq!(tag(&["[AB] x", "[AB] y", "[AB] z", "p", "q", "r"]), None);
        // Four of six is 67%.
        assert_eq!(
            tag(&["[AB] x", "[AB] y", "[AB] z", "[AB] w", "q", "r"]),
            Some("AB".into())
        );
    }

    #[test]
    fn identical_names_count_once() {
        assert_eq!(tag(&["[AB] x", "[AB] x", "[AB] x", "p", "q"]), None);
    }

    #[test]
    fn a_part_under_two_characters_is_not_a_tag() {
        assert_eq!(tag(&["x.one", "x.two", "x.three"]), None);
        assert_eq!(tag(&["alpha", "bravo", "charlie"]), None);
        assert_eq!(tag(&[] as &[&str]), None);
    }

    #[test]
    fn cleaning_keeps_letters_digits_and_single_spaces() {
        let clean = |s: &str| clean_tag(&s.chars().collect::<Vec<_>>());
        assert_eq!(clean("[g/s/k/i/L/L]_"), "gskiLL");
        assert_eq!(clean("dicE[: :]"), "dicE");
        assert_eq!(clean("  a  -  b  "), "a b");
        assert_eq!(clean("-#över. "), "över");
        assert_eq!(clean("pb j"), "pb j");
        assert_eq!(clean("[]"), "");
    }

    #[test]
    fn a_start_and_an_end_the_longer_wins() {
        assert_eq!(
            tag(&["=AB= x -longtag-", "=AB= y -longtag-", "=AB= z -longtag-"]),
            Some("longtag".into())
        );
        assert_eq!(
            tag(&["=AB= x -CD-", "=AB= y -CD-", "=AB= z -CD-"]),
            Some("AB".into())
        );
    }
}
