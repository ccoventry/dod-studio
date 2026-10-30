//! Tests on `test-fixtures/ci_fixture.dem`, a real POV recording checked in so
//! CI runs the analyzer on real network traffic (see `test-fixtures/README.md`).
//! The expected values are what the recording contains: the recorder played
//! Allies rifleman against five Sturmbot bots on dod_anzio.

use crate::*;
use dod::Class;
use std::collections::HashMap;

pub(crate) fn fixture() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/ci_fixture.dem"
    );
    std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub(crate) fn analysis() -> &'static Analysis {
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
fn cap_breaks() {
    // Two timed captures, both completed: nothing to break (#192). This pins
    // that a recording with no cancel credits nobody and counts nothing.
    let state = &analysis().state;
    let outcomes: Vec<_> = state
        .objectives
        .attempts
        .iter()
        .map(|a| (a.outcome.clone(), a.cancel_cause.clone()))
        .collect();
    assert_eq!(
        outcomes,
        [
            (AttemptOutcome::Captured, None),
            (AttemptOutcome::Captured, None)
        ]
    );
    assert_eq!(state.objectives.blocks_unattributed, 0);
    assert_eq!(state.objectives.attempts_abandoned, 0);
    assert!(state.players.iter().all(|p| p.cap_breaks == 0));
}

#[test]
fn highlights_keep_their_kill_time_slots() {
    // Everyone is still in the game at this demo's end, so each highlight's
    // slot is also the one they end in, and none is dropped.
    let found = crate::highlights::highlights(analysis());
    let mut per_player: Vec<(&str, usize)> = Vec::new();
    for h in &found.all {
        let p = player(&h.player);
        let Connection::Connected { client_id } = p.connection else {
            panic!("{} left before the end", h.player);
        };
        assert_eq!(h.slot, Some(client_id), "{}", h.player);
        match per_player.iter_mut().find(|(name, _)| *name == p.name) {
            Some((_, n)) => *n += 1,
            None => per_player.push((p.name.as_str(), 1)),
        }
    }
    assert_eq!(per_player, EXPECTED_HIGHLIGHTS);
}

/// Highlights per player, in the analysis' player order.
const EXPECTED_HIGHLIGHTS: [(&str, usize); 4] = [
    ("Cpl. Preddy", 1),
    ("CI_FIXTURE DEMO TESTER", 3),
    ("Pvt. Clopton", 1),
    ("Gefr. Fortune", 2),
];

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

/// #811: a damaged copy of the fixture is analysed up to the break, and says
/// where that is. Built here, so CI runs it.
mod damaged {
    use super::*;

    fn total_kills(analysis: &Analysis) -> usize {
        analysis
            .state
            .players
            .iter()
            .map(|p| p.stats.1.max(0) as usize)
            .sum()
    }

    fn assert_partial(damaged: &Analysis) {
        let full = analysis();
        let damage = damaged
            .demo_info
            .damage
            .as_ref()
            .expect("the break is noted");
        let end = full.state.current_time.viewdemo_offset.as_secs_f32();
        assert!(
            damage.read_up_to_secs > 0.0 && damage.read_up_to_secs < end,
            "{damage:?}, the whole demo runs to {end}"
        );
        // The analysis is the demo's up to there.
        assert!(
            damaged.state.current_time.viewdemo_offset.as_secs_f32() <= damage.read_up_to_secs,
            "{damage:?}"
        );
        assert!(total_kills(damaged) <= total_kills(full));
        assert!(damage.bytes_unread > 0 && !damage.reason.is_empty());
        let summary = damage.summary();
        assert!(
            summary.starts_with(&format!(
                "Damaged demo: {}, the rest (",
                damage.read_up_to_text()
            )),
            "{summary}"
        );
    }

    #[test]
    fn an_intact_demo_is_not_damaged() {
        assert_eq!(analysis().demo_info.damage, None);
    }

    #[test]
    fn a_file_cut_short_is_read_up_to_the_cut() {
        let bytes = fixture();
        let damaged =
            Analysis::try_from_bytes(&bytes[..bytes.len() / 2]).expect("half a demo reads");
        assert_partial(&damaged);
    }

    #[test]
    fn a_bad_message_length_is_read_up_to_its_frame() {
        let bytes = fixture();
        let demo = dem::open_demo_from_bytes(&bytes).unwrap();
        let spans: Vec<_> = demo
            .directory
            .entries
            .iter()
            .flat_map(|e| &e.frames)
            .filter_map(|f| match &f.frame_data {
                FrameData::NetworkMessage(n) => n.1.source_span.clone(),
                _ => None,
            })
            .collect();
        let span = &spans[spans.len() * 2 / 3];
        let mut mangled = bytes.clone();
        mangled[span.start - 4..span.start].copy_from_slice(&0x0010_0000u32.to_le_bytes());

        let damaged = Analysis::try_from_bytes(&mangled).expect("the frames before it read");
        assert_partial(&damaged);
        let damage = damaged.demo_info.damage.unwrap();
        assert_eq!(damage.reason, "message length too long: 1048576");
        assert!(damage.byte_offset < span.start as u64);
    }

    #[test]
    fn the_note_survives_the_cache() {
        let bytes = fixture();
        let damaged = Analysis::try_from_bytes(&bytes[..bytes.len() / 2]).unwrap();
        let json = serde_json::to_string(&damaged).unwrap();
        let back: Analysis = serde_json::from_str(&json).unwrap();
        assert_eq!(back.demo_info.damage, damaged.demo_info.damage);
    }

    #[test]
    fn summary_reads_in_words() {
        let damage = DemoDamage {
            read_up_to_secs: 125.9,
            frame_index: 7,
            byte_offset: 1000,
            bytes_unread: 15 << 20,
            reason: "message length too long: 70000".into(),
        };
        assert_eq!(
            damage.summary(),
            "Damaged demo: read up to 2:05, the rest (15.0 MB) does not parse."
        );
        let small = DemoDamage {
            bytes_unread: 1500,
            ..damage
        };
        assert!(
            small.summary().contains("the rest (2 KB)"),
            "{}",
            small.summary()
        );
    }

    /// The demo #811 was seen on: an HLTV recording whose server crashed a
    /// minute or two after the clan match went live. Not in the tree (19.6 MB);
    /// set `DOD_DAMAGED_DEMO` to it, or keep it in the POST install's `dod\`.
    #[test]
    #[ignore = "needs hltv-2508021304-dod_solitude_b2.dem from #811"]
    fn the_damaged_demo_from_811_reads_up_to_its_break() {
        let path = std::env::var_os("DOD_DAMAGED_DEMO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(concat!(
                    r"C:\Program Files (x86)\Steam\steamapps\common",
                    r"\Half-Life - POST-Anniversary for Movies\dod",
                    r"\hltv-2508021304-dod_solitude_b2.dem"
                ))
            });
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e} (set DOD_DAMAGED_DEMO to the damaged demo from #811)",
                path.display()
            )
        });
        assert!(
            dem::open_demo_from_bytes(&bytes).is_err(),
            "the strict parse refuses it"
        );
        let analysis = Analysis::try_from_bytes(&bytes).expect("the frames before the break read");
        let damage = analysis
            .demo_info
            .damage
            .clone()
            .expect("the break is noted");
        assert!(damage.read_up_to_secs > 60.0, "{damage:?}");
        assert!(!analysis.state.players.is_empty());
        println!(
            "{}: {} at frame {}, byte {} ({}); {} players, {} kills",
            path.display(),
            damage.summary(),
            damage.frame_index,
            damage.byte_offset,
            damage.reason,
            analysis.state.players.len(),
            total_kills(&analysis)
        );
    }

    /// #15's four damaged POV demos (`docs/goldsrc_demo_salvage.md`), each
    /// with holes partway: read up to the first one. Not in the tree (80 to
    /// 115 MB each); set `DOD_SALVAGE_DEMOS` to the folder holding them, or
    /// keep them in the PRE install's `dod\`.
    #[test]
    #[ignore = "needs the four damaged monday-wsod25_r07 demos from #15"]
    fn the_damaged_pov_demos_from_15_read_up_to_their_first_hole() {
        const DEMOS: [&str; 4] = [
            "monday-wsod25_r07_m1_h1.dem",
            "monday-wsod25_r07_m1_h2.dem",
            "monday-wsod25_r07_m3_h1-1.dem",
            "monday-wsod25_r07_m3_h2.dem",
        ];
        let folder = std::env::var_os("DOD_SALVAGE_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        for name in DEMOS {
            let path = folder.join(name);
            let bytes = std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "{}: {e} (set DOD_SALVAGE_DEMOS to the folder holding #15's demos)",
                    path.display()
                )
            });
            let analysis = Analysis::try_from_bytes(&bytes)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let damage = analysis
                .demo_info
                .damage
                .clone()
                .unwrap_or_else(|| panic!("{}: no break noted", path.display()));
            println!(
                "{name}: {} at frame {}, byte {} of {} ({}); {} players, {} kills",
                damage.summary(),
                damage.frame_index,
                damage.byte_offset,
                bytes.len(),
                damage.reason,
                analysis.state.players.len(),
                total_kills(&analysis)
            );
        }
    }
}

#[test]
fn headshots_are_told_from_the_helmet_events() {
    let kills = &analysis().state.kill_positions;
    let count = |wanted| kills.iter().filter(|k| k.headshot == wanted).count();
    // 18 deaths, seven with a helmet pop on their frame; no British victim
    // and no double kill on a frame the position can't settle.
    assert_eq!(kills.len(), 18);
    assert_eq!(count(Headshot::Yes), 7);
    assert_eq!(count(Headshot::No), 11);
    assert_eq!(count(Headshot::Unknown), 0);
}

#[test]
fn a_highlight_counts_its_headshots() {
    let highlights = highlights::highlights(analysis());
    let total: usize = highlights.all.iter().map(|h| h.headshot_counts().0).sum();
    let kills: usize = highlights.all.iter().map(|h| h.kills.len()).sum();
    // Every kill a player scored (no teamkills in this demo) carries its verdict.
    assert!(total > 0 && total <= kills);
    assert!(highlights.all.iter().all(|h| h.headshot_counts().1 == 0));
}
