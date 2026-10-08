//! Tests on `test-fixtures/ci_fixture.dem`, a real POV recording checked in so
//! CI runs the analyzer on real network traffic (see `test-fixtures/README.md`).
//! The expected values are what the recording contains: the recorder played
//! Allies rifleman against five Sturmbot bots on dod_anzio.

use crate::*;
use dod::Class;
use std::collections::HashMap;

fn fixture() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/ci_fixture.dem"
    );
    std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn analysis() -> &'static Analysis {
    static ANALYSIS: std::sync::OnceLock<Analysis> = std::sync::OnceLock::new();
    ANALYSIS.get_or_init(|| Analysis::try_from_bytes(&fixture()).unwrap())
}

const RECORDER: &str = "76561197961831029";

fn player(name: &str) -> &'static Player {
    analysis()
        .state
        .players
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no player {name}"))
}

#[test]
fn demo_header_and_server() {
    let a = analysis();
    assert_eq!(a.demo_info.demo_type, "POV");
    assert_eq!(a.demo_info.map_name, "dod_anzio");
    assert_eq!(a.demo_info.demo_protocol, 5);
    assert_eq!(a.demo_info.network_protocol, 48);
    assert_eq!(a.state.server_name.as_deref(), Some("DoD v1.2"));
    assert!(!a.state.allies_are_british);
}

#[test]
fn scoreboard() {
    // (name, team, score, kills, deaths), as the game's scoreboard ended.
    let expected = [
        ("CI_FIXTURE DEMO TESTER", Team::Allies, 4, 8, 3),
        ("Cpl. Preddy", Team::Allies, 7, 3, 4),
        ("Pvt. Clopton", Team::Allies, 3, 2, 4),
        ("Gefr. Fortune", Team::Axis, 8, 5, 2),
        ("Maj. Metzler", Team::Axis, 6, 3, 5),
        ("Gmnr. Krause", Team::Axis, 6, 0, 6),
    ];
    assert_eq!(analysis().state.players.len(), expected.len());
    for (name, team, score, kills, deaths) in expected {
        let p = player(name);
        assert_eq!(p.team, Some(team), "{name}");
        assert_eq!(p.stats, (score, kills, deaths), "{name}");
    }
    let recorder = player("CI_FIXTURE DEMO TESTER");
    assert_eq!(recorder.id.to_string(), RECORDER);
    assert_eq!(recorder.class, Some(Class::Rifleman));
}

#[test]
fn team_scores() {
    let scores = &analysis().state.team_scores;
    assert_eq!(scores.get_team_score(Team::Allies), 10);
    assert_eq!(scores.get_team_score(Team::Axis), 66);
}

#[test]
fn weapon_breakdowns() {
    // weapon -> (kills, team kills)
    let recorder: HashMap<Weapon, (u32, u32)> = [
        (Weapon::Garand, (4, 0)),
        (Weapon::Kabar, (2, 0)),
        (Weapon::M1911, (1, 0)),
        (Weapon::Mk2Grenade, (1, 1)),
        (Weapon::Unknown, (0, 1)),
    ]
    .into();
    assert_eq!(player("CI_FIXTURE DEMO TESTER").weapon_breakdown, recorder);

    let fortune: HashMap<Weapon, (u32, u32)> =
        [(Weapon::ScopedK98, (3, 0)), (Weapon::Spade, (2, 0))].into();
    assert_eq!(player("Gefr. Fortune").weapon_breakdown, fortune);
}

#[test]
fn rounds() {
    let rounds = &analysis().state.rounds;
    assert_eq!(rounds.len(), 3);
    assert!(matches!(
        rounds[1],
        Round::Completed {
            winner_stats: Some((Team::Axis, 5)),
            ..
        }
    ));
}

#[test]
fn chat() {
    let said = |kind: ChatType, text: &str| {
        analysis().state.chat_messages.iter().any(|c| {
            c.chat_type == kind
                && c.text == text
                && c.sender_name.as_deref() == Some("CI_FIXTURE DEMO TESTER")
        })
    };
    assert!(said(ChatType::Mm1, "hello"), "all chat");
    assert!(said(ChatType::Mm2, "hi"), "team chat");
}

#[test]
fn flag_captures() {
    let objectives = &analysis().state.objectives;
    assert_eq!(objectives.flags.len(), 5);
    assert_eq!(objectives.captures.len(), 15);
    let by_recorder: Vec<_> = objectives
        .captures
        .iter()
        .filter(|c| {
            c.capper
                .as_ref()
                .is_some_and(|id| id.to_string() == RECORDER)
        })
        .map(|c| c.flag_name.as_str())
        .collect();
    assert_eq!(by_recorder, ["POINT_ANZIO_HILL", "POINT_ANZIO_HILL"]);
}

#[test]
fn optimized_matches_unoptimized() {
    crate::tests::assert_optimized_matches_unoptimized(&fixture());
}

#[test]
fn pov_stats() {
    let pov = &analysis().state.pov_stats;
    assert_eq!(pov.suicides, 2);
    assert_eq!(pov.hits_taken, 4);
    let garand = &pov.weapon_stats[&Weapon::Garand];
    assert_eq!(
        (garand.kills, garand.bullets_fired, garand.reloads),
        (4, 10, 1)
    );
}
