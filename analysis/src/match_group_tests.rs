use super::*;
use crate::{AnalyzerState, Player, PlayerGlobalId, Weapon, kill::KillStreak, time::GameTime};
use std::time::Duration;

fn d(secs: f64) -> Duration {
    Duration::from_secs_f64(secs)
}

/// Player names after the tag, with no shared start, so the tag is the
/// whole of what a team's names share.
const WORDS: [&str; 6] = ["ace", "bo", "cy", "dax", "eli", "fen"];

/// One synthetic recording: two six-player teams named `<tag>|<word>`
/// with ids from `<tag><n>`, a kill feed, chat and clock.
#[derive(Clone)]
struct Spec {
    map: &'static str,
    allied: &'static str,
    axis: &'static str,
    hltv: bool,
    /// The recording player's id key; the first Allied player by default.
    recorder: Option<&'static str>,
    /// Every kill in the feed is with this weapon: another weapon is
    /// another game.
    weapon: Weapon,
    /// Added to every kill's clock time (an HLTV recording's offset).
    shift: f64,
    kills: usize,
    /// `(id key, name)`: a player who shows another name.
    renamed: Vec<(&'static str, &'static str)>,
    /// `(recording secs, sender, text)`; no sender is the server.
    chat: Vec<(f64, Option<&'static str>, &'static str)>,
    /// Recording secs the match went live, when the recording saw it.
    live: Option<f64>,
    /// The server clock at the recording's start.
    clock_start: f64,
    length: f64,
    /// Players with a per-demo `CONNECTION_n` id instead of a SteamID.
    no_ids: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            map: "dod_avalanche",
            allied: "alpha",
            axis: "bravo",
            hltv: false,
            recorder: None,
            weapon: Weapon::Garand,
            shift: 0.0,
            kills: 20,
            renamed: vec![],
            chat: vec![],
            live: None,
            clock_start: 0.0,
            length: 1800.0,
            no_ids: false,
        }
    }
}

impl Spec {
    fn analysis(&self) -> Analysis {
        let mut state = AnalyzerState::default();
        let recorder = self
            .recorder
            .map_or(format!("{}1", self.allied), String::from);
        for (side, tag) in [(Team::Allies, self.allied), (Team::Axis, self.axis)] {
            for n in 1..=6 {
                let key = format!("{tag}{n}");
                let slot = state.players.len() as u8;
                if key == recorder && !self.hltv {
                    state.pov_player_index = Some(slot);
                }
                let mut p = Player::new_mock(slot, &key);
                if self.no_ids {
                    p.id =
                        serde_json::from_str::<PlayerGlobalId>(&format!("\"CONNECTION_{slot}\""))
                            .unwrap();
                }
                p.name = self
                    .renamed
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map_or(format!("{tag}|{}", WORDS[n - 1]), |(_, name)| {
                        name.to_string()
                    });
                p.team = Some(side.clone());
                state.players.push(p);
            }
        }
        for i in 0..self.kills {
            let killer = i % 6;
            let victim = 6 + (i / 2) % 6;
            let victim_id = state.players[victim].id.clone();
            let secs = 30.0 + i as f64 * 31.7;
            let time = GameTime {
                real_offset: d(secs),
                viewdemo_offset: d(self.clock_start + secs + self.shift),
                frame_index: 0,
            };
            state.players[killer].kill_streaks.push(KillStreak {
                kills: vec![(time, self.weapon.clone(), victim_id)],
                slot: None,
            });
        }
        for &(secs, sender, text) in &self.chat {
            state.chat_messages.push(ChatMessage {
                time: GameTime {
                    real_offset: d(secs),
                    viewdemo_offset: d(self.clock_start + secs),
                    frame_index: 0,
                },
                frame_index: 0,
                chat_type: if sender.is_some() {
                    ChatType::Mm1
                } else {
                    ChatType::System
                },
                sender_name: sender.map(String::from),
                sender_team: None,
                sender_dead: false,
                text: text.to_string(),
                system_token: None,
                system_args: vec![],
            });
        }
        if let Some(live) = self.live {
            state.match_start_witnessed = true;
            state.rounds = vec![Round::Active {
                allies_kills: 0,
                axis_kills: 0,
                start_time: GameTime {
                    real_offset: d(live),
                    viewdemo_offset: d(self.clock_start + live),
                    frame_index: 0,
                },
            }];
        }
        state.current_time = GameTime {
            real_offset: d(self.length),
            viewdemo_offset: d(self.clock_start + self.length),
            frame_index: 0,
        };
        state.hltv_name = Some("Test Server - HLTV".into());
        Analysis {
            demo_info: crate::DemoInfo {
                map_name: self.map.to_string(),
                demo_type: if self.hltv { "HLTV" } else { "POV" }.to_string(),
                playback_time: self.length as f32,
                ..Default::default()
            },
            state,
        }
    }
}

/// The first half of a match: alpha on the Allied side.
fn h1() -> Spec {
    Spec::default()
}

/// Its second half: sides swapped, another kill feed, the same recorder.
fn h2() -> Spec {
    Spec {
        allied: "bravo",
        axis: "alpha",
        recorder: Some("alpha1"),
        weapon: Weapon::K98,
        ..Spec::default()
    }
}

fn hltv(spec: Spec) -> Spec {
    Spec {
        hltv: true,
        shift: 1.342,
        ..spec
    }
}

type Input = (&'static str, Spec, Option<u64>);

fn group(specs: &[Input]) -> Vec<MatchGroup<&'static str>> {
    let analyses: Vec<Analysis> = specs.iter().map(|(_, s, _)| s.analysis()).collect();
    let input: Vec<(&'static str, &Analysis, Option<u64>)> = specs
        .iter()
        .zip(&analyses)
        .map(|((id, _, t), a)| (*id, a, *t))
        .collect();
    group_matches(&input)
}

fn order_of(groups: &[MatchGroup<&'static str>], id: &'static str) -> (Half, HalfReason) {
    let (_, half) = find_demo(groups, &id).expect("grouped");
    (half.call.half, half.call.reason)
}

/// Three players saying `text` at 1790 s, after a live at 60 s.
fn said(text: &'static str) -> Vec<(f64, Option<&'static str>, &'static str)> {
    ["alpha|p2", "bravo|p3", "alpha|p4"]
        .into_iter()
        .map(|who| (1790.0, Some(who), text))
        .collect()
}

#[test]
fn two_recordings_of_each_half_make_one_match_of_two_halves() {
    let first = Spec {
        chat: said("gh"),
        live: Some(60.0),
        ..h1()
    };
    let second = Spec {
        chat: said("gg"),
        live: Some(60.0),
        ..h2()
    };
    // The second half first, to show the output is ordered by half.
    let groups = group(&[
        ("h2_pov", second.clone(), None),
        ("h1_pov", first.clone(), None),
        ("h2_hltv", hltv(second), None),
        ("h1_hltv", hltv(first), None),
    ]);
    assert_eq!(groups.len(), 1, "{groups:#?}");
    let g = &groups[0];
    assert_eq!(g.map, "dod_avalanche");
    assert_eq!(g.halves.len(), 2);
    assert_eq!(g.halves[0].demos, ["h1_pov", "h1_hltv"]);
    assert_eq!(g.halves[1].demos, ["h2_pov", "h2_hltv"]);
    assert_eq!(g.halves[0].order(), Some(1));
    assert_eq!(g.halves[1].order(), Some(2));
    assert_eq!(g.halves[0].call.reason, HalfReason::EndOfHalfChat);
    assert_eq!(g.halves[0].sides, [Side::Allied, Side::Axis]);
    assert_eq!(g.halves[1].sides, [Side::Axis, Side::Allied]);
    assert_eq!(g.teams[0].tag.as_deref(), Some("alpha"));
    assert_eq!(g.teams[1].tag.as_deref(), Some("bravo"));
    assert_eq!(g.teams[0].players.len(), 6);
    assert!(g.teams[0].players.iter().all(|id| id.contains(":alpha")));
}

#[test]
fn trimmed_analyses_group_the_same() {
    let first = Spec {
        chat: said("gh"),
        live: Some(60.0),
        ..h1()
    };
    let second = Spec {
        chat: said("gg"),
        live: Some(60.0),
        ..h2()
    };
    let specs = [
        ("h2_pov", second.clone()),
        ("h1_pov", first.clone()),
        ("h2_hltv", hltv(second)),
        ("h1_hltv", hltv(first)),
    ];
    let whole: Vec<Analysis> = specs.iter().map(|(_, s)| s.analysis()).collect();
    let mut trimmed: Vec<Analysis> = specs.iter().map(|(_, s)| s.analysis()).collect();
    trimmed.iter_mut().for_each(trim_for_grouping);
    let input = |analyses: &[Analysis]| -> Vec<MatchGroup<&'static str>> {
        let demos: Vec<(&'static str, &Analysis, Option<u64>)> = specs
            .iter()
            .zip(analyses)
            .map(|((id, _), a)| (*id, a, None))
            .collect();
        group_matches(&demos)
    };
    assert_eq!(input(&whole), input(&trimmed));
}

#[test]
fn different_maps_never_group() {
    let other_map = Spec {
        map: "dod_anzio",
        ..h1()
    };
    assert_eq!(group(&[("a", h1(), None), ("b", other_map, None)]).len(), 2);
}

#[test]
fn the_same_clan_against_another_opponent_is_another_match() {
    // Six of twelve players shared: under the 60% bar.
    let other = Spec {
        axis: "charlie",
        ..h1()
    };
    let groups = group(&[("a", h1(), None), ("b", other, None)]);
    assert_eq!(groups.len(), 2, "{groups:#?}");
}

#[test]
fn a_rematch_on_the_same_sides_shares_no_kills_and_stays_apart() {
    // Same teams, map and sides, another game (no kill pairs), and some
    // players have restyled their names since.
    let rematch = Spec {
        weapon: Weapon::Thompson,
        renamed: vec![
            ("alpha1", "alpha.p1"),
            ("alpha2", "alpha.p2"),
            ("bravo1", "bravo.p1"),
            ("bravo2", "bravo.p2"),
        ],
        ..h1()
    };
    let groups = group(&[("a", h1(), None), ("b", rematch, None)]);
    assert_eq!(groups.len(), 2, "{groups:#?}");
}

#[test]
fn known_risk_a_rematch_with_unchanged_names_merges() {
    // The open risk the module docs name: no kills in common, but every
    // name agrees, so it reads as pieces of one half. This pins today's
    // answer so a fix has to change it on purpose.
    let rematch = Spec {
        weapon: Weapon::Thompson,
        ..h1()
    };
    let groups = group(&[("a", h1(), None), ("b", rematch, None)]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].halves[0].demos, ["a", "b"]);
}

#[test]
fn same_sides_without_kills_need_the_names_to_agree() {
    let piece = Spec { kills: 0, ..h1() };
    assert_eq!(
        group(&[("a", h1(), None), ("b", piece.clone(), None)]).len(),
        1
    );
    let restyled = Spec {
        renamed: vec![
            ("alpha1", "x1"),
            ("alpha2", "x2"),
            ("bravo1", "x3"),
            ("bravo2", "x4"),
        ],
        ..piece
    };
    assert_eq!(group(&[("a", h1(), None), ("b", restyled, None)]).len(), 2);
}

#[test]
fn a_half_with_two_candidate_partners_takes_the_closer_names_and_the_other_stays_alone() {
    // `x2` and `y2` both have swapped sides against `x1` and both pass
    // the name bar, but they disagree with each other on four names, so
    // they aren't one half. A match has two halves at most: x1 pairs
    // with the one whose names agree best.
    let x2 = Spec {
        renamed: vec![("alpha1", "x.1")],
        ..h2()
    };
    let y2 = Spec {
        weapon: Weapon::Thompson,
        renamed: vec![("alpha3", "y.3"), ("bravo4", "y.4"), ("bravo5", "y.5")],
        ..h2()
    };
    let groups = group(&[("y2", y2, None), ("x1", h1(), None), ("x2", x2, None)]);
    assert_eq!(groups.len(), 2, "{groups:#?}");
    let (g, _) = find_demo(&groups, &"x1").unwrap();
    let ids: Vec<_> = g.halves.iter().flat_map(|h| h.demos.clone()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"x2"), "{groups:#?}");
    let (alone, _) = find_demo(&groups, &"y2").unwrap();
    assert_eq!(alone.halves.len(), 1);
}

#[test]
fn the_servers_announcement_decides_before_chat() {
    // Chat says the opposite (a "gg" in the first half's recording),
    // and the server text wins.
    let first = Spec {
        chat: said("gg"),
        live: Some(60.0),
        ..h1()
    };
    let mut second_chat = said("gh");
    second_chat.push((5.0, None, "=== 2nd HALF STARTING ==="));
    let second = Spec {
        chat: second_chat,
        live: Some(60.0),
        ..h2()
    };
    let groups = group(&[("a", first, None), ("b", second, None)]);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::First, HalfReason::ServerText)
    );
    assert_eq!(
        order_of(&groups, "b"),
        (Half::Second, HalfReason::ServerText)
    );
}

#[test]
fn warm_up_gg_before_live_is_not_end_of_match_chat() {
    // Before live, three players say gg about the previous map; after
    // it, one says "good half". Counted from the start, this first half
    // would read as the end of the match.
    let mut chat: Vec<_> = said("gg")
        .into_iter()
        .map(|(_, who, text)| (20.0, who, text))
        .collect();
    chat.push((1795.0, Some("bravo|p1"), "good half"));
    let first = Spec {
        chat,
        live: Some(60.0),
        ..h1()
    };
    let second = Spec {
        chat: vec![(1790.0, Some("alpha|p1"), "gg")],
        live: Some(60.0),
        ..h2()
    };
    let groups = group(&[("a", first, None), ("b", second, None)]);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::First, HalfReason::EndOfHalfChat)
    );
    assert_eq!(
        order_of(&groups, "b"),
        (Half::Second, HalfReason::EndOfHalfChat)
    );
}

#[test]
fn a_server_clock_kept_running_puts_the_later_half_second() {
    // No chat. The second half's recording starts after the first's
    // ends on the server clock. Its warm-up (clock at live) looks long,
    // which alone would say the opposite: the clock decides first.
    let first = Spec {
        live: Some(100.0),
        ..h1()
    };
    let second = Spec {
        live: Some(100.0),
        clock_start: 1900.0,
        ..h2()
    };
    let groups = group(&[("b", second, None), ("a", first, None)]);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::First, HalfReason::ServerClock)
    );
    assert_eq!(
        order_of(&groups, "b"),
        (Half::Second, HalfReason::ServerClock)
    );
    assert_eq!(groups[0].halves[0].demos, ["a"]);
}

#[test]
fn a_long_warm_up_is_the_first_half_only_by_a_wide_margin() {
    // The map reloaded at the half: both recordings start at clock 0.
    let long = Spec {
        live: Some(900.0),
        length: 2700.0,
        ..h1()
    };
    let short = Spec {
        live: Some(150.0),
        ..h2()
    };
    let groups = group(&[("a", long, None), ("b", short, None)]);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::First, HalfReason::WarmUpLength)
    );
    assert_eq!(
        order_of(&groups, "b"),
        (Half::Second, HalfReason::WarmUpLength)
    );
    // 300 s against 200 s is not a clear habit: no answer.
    let a = Spec {
        live: Some(300.0),
        ..h1()
    };
    let b = Spec {
        live: Some(200.0),
        ..h2()
    };
    let groups = group(&[("a", a, None), ("b", b, None)]);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::Unknown, HalfReason::Undecided)
    );
}

#[test]
fn file_dates_count_only_when_a_recorders_two_files_are_a_half_apart() {
    // Recorded: the second file was finished 2000 s after the first.
    let groups = group(&[
        ("b", h2(), Some(1_700_002_000)),
        ("a", h1(), Some(1_700_000_000)),
    ]);
    assert_eq!(order_of(&groups, "a"), (Half::First, HalfReason::FileDates));
    assert_eq!(
        order_of(&groups, "b"),
        (Half::Second, HalfReason::FileDates)
    );
    // Copied: seconds apart, in whatever order the copy ran.
    let groups = group(&[
        ("b", h2(), Some(1_700_000_005)),
        ("a", h1(), Some(1_700_000_000)),
    ]);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::Unknown, HalfReason::Undecided)
    );
    // Another recorder's file says nothing about this one's.
    let other_recorder = Spec {
        recorder: Some("bravo1"),
        ..h2()
    };
    let groups = group(&[
        ("b", other_recorder, Some(1_700_002_000)),
        ("a", h1(), Some(1_700_000_000)),
    ]);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::Unknown, HalfReason::Undecided)
    );
}

#[test]
fn half_of_one_file_uses_its_server_text_then_its_chat_and_never_guesses() {
    let announced = Spec {
        chat: vec![(5.0, None, "=== 2nd HALF STARTING ===")],
        ..h2()
    };
    let call = half_of(&announced.analysis());
    assert_eq!(
        (call.half, call.reason),
        (Half::Second, HalfReason::ServerText)
    );

    let good_half = Spec {
        chat: said("gh"),
        live: Some(60.0),
        ..h1()
    };
    let call = half_of(&good_half.analysis());
    assert_eq!(
        (call.half, call.reason),
        (Half::First, HalfReason::EndOfHalfChat)
    );
    assert_eq!(call.half.number(), Some(1));

    let call = half_of(&h1().analysis());
    assert_eq!(
        (call.half, call.reason),
        (Half::Unknown, HalfReason::Undecided)
    );
    assert_eq!(call.half.number(), None);
}

#[test]
fn a_half_alone_is_ordered_by_its_own_recordings() {
    let second = Spec {
        chat: said("gg"),
        live: Some(60.0),
        ..h2()
    };
    let elsewhere = Spec {
        map: "dod_anzio",
        ..h1()
    };
    let groups = group(&[("a", second, None), ("b", elsewhere, None)]);
    assert_eq!(groups.len(), 2);
    assert_eq!(
        order_of(&groups, "a"),
        (Half::Second, HalfReason::EndOfHalfChat)
    );
    assert_eq!(
        order_of(&groups, "b"),
        (Half::Unknown, HalfReason::Undecided)
    );
}

#[test]
fn demos_without_steam_ids_relate_by_clan_tags() {
    let no_ids = |s: Spec| Spec { no_ids: true, ..s };
    let other = Spec {
        axis: "charlie",
        ..h1()
    };
    let groups = group(&[
        ("h1_a", no_ids(h1()), None),
        ("h1_b", no_ids(hltv(h1())), None),
        ("h2_a", no_ids(h2()), None),
        ("other", no_ids(other), None),
    ]);
    assert_eq!(groups.len(), 2, "{groups:#?}");
    let (g, half) = find_demo(&groups, &"h1_a").unwrap();
    assert_eq!(half.demos, ["h1_a", "h1_b"]);
    assert_eq!(g.halves.len(), 2);
    assert_eq!(g.teams[0].tag.as_deref(), Some("alpha"));
    // No ids, no players listed.
    assert!(g.teams[0].players.is_empty());
}

#[test]
fn the_fixture_is_a_group_of_its_own() {
    let a = crate::tests_fixture::analysis();
    let groups = group_matches(&[("fixture", a, None)]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].map, "dod_anzio");
    assert_eq!(groups[0].halves[0].demos, ["fixture"]);
}

/// The WSOD25 set the probe scored 68/68 (#685): every
/// `wsod25_grp<N>_h<1|2>_<recorder>.dem` in `DOD_ANALYSIS_DEMOS` (a
/// folder) or the PRE install's `dod` folder, parsed (the analyzer cache
/// needs Studio) and scored against the names. Slow: run it in release,
/// `cargo test -p analysis --release -- --ignored wsod25 --nocapture`.
#[test]
#[ignore]
fn wsod25_matches_and_halves_match_the_file_names() {
    let folder = std::env::var_os("DOD_ANALYSIS_DEMOS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
            )
        });
    let entries = std::fs::read_dir(&folder)
        .unwrap_or_else(|e| panic!("{}: {e} (set DOD_ANALYSIS_DEMOS)", folder.display()));
    // (file name, match number, half); a split's output and other
    // derived names have more tokens and are left out.
    let mut named: Vec<(String, u32, u8)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name.strip_suffix(".dem")?;
            let tokens: Vec<&str> = stem.split('_').collect();
            let [prefix, grp, half, _recorder] = tokens[..] else {
                return None;
            };
            let group = grp.strip_prefix("grp")?.parse().ok()?;
            let half = match half {
                "h1" => 1,
                "h2" => 2,
                _ => return None,
            };
            (prefix == "wsod25").then_some((name, group, half))
        })
        .collect();
    named.sort();
    assert!(
        !named.is_empty(),
        "no wsod25_grp*_h*_*.dem in {} (set DOD_ANALYSIS_DEMOS)",
        folder.display()
    );

    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    type Parsed = Option<(Analysis, Option<u64>)>;
    let parsed: std::sync::Mutex<Vec<Parsed>> =
        std::sync::Mutex::new((0..named.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((name, _, _)) = named.get(i) else {
                        break;
                    };
                    let path = folder.join(name);
                    let bytes =
                        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    let analysis = Analysis::try_from_bytes(&bytes)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    let modified = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|t| t.as_secs());
                    parsed.lock().unwrap()[i] = Some((analysis, modified));
                }
            });
        }
    });
    let parsed: Vec<(Analysis, Option<u64>)> = parsed
        .into_inner()
        .unwrap()
        .into_iter()
        .map(Option::unwrap)
        .collect();
    let input: Vec<(usize, &Analysis, Option<u64>)> = parsed
        .iter()
        .enumerate()
        .map(|(i, (a, t))| (i, a, *t))
        .collect();
    let groups = group_matches(&input);

    let (mut match_right, mut half_right) = (0, 0);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut misses = Vec::new();
    for (i, (name, grp, half)) in named.iter().enumerate() {
        let (g, h) = find_demo(&groups, &i).unwrap();
        let in_match: BTreeSet<usize> = g.halves.iter().flat_map(|h| h.demos.clone()).collect();
        let truth_match: BTreeSet<usize> =
            (0..named.len()).filter(|&j| named[j].1 == *grp).collect();
        let in_half: BTreeSet<usize> = h.demos.iter().copied().collect();
        let truth_half: BTreeSet<usize> = (0..named.len())
            .filter(|&j| named[j].1 == *grp && named[j].2 == *half)
            .collect();
        let m_ok = in_match == truth_match;
        let h_ok = in_half == truth_half && h.order() == Some(*half);
        match_right += m_ok as usize;
        half_right += h_ok as usize;
        *reasons.entry(format!("{:?}", h.call.reason)).or_default() += 1;
        if !(m_ok && h_ok) {
            misses.push(format!(
                "{name}: match {m_ok}, half {h_ok} ({:?} by {:?}: {})",
                h.call.half, h.call.reason, h.call.detail
            ));
        }
    }
    println!(
        "{} demos, {} matches found: right match {match_right}, right half {half_right}; half decided by {reasons:?}",
        named.len(),
        groups.len()
    );
    for g in &groups {
        println!(
            "  {} {:?} vs {:?}: {:?}",
            g.map,
            g.teams[0].tag,
            g.teams[1].tag,
            g.halves
                .iter()
                .map(|h| (h.order(), h.demos.len(), h.call.detail.clone()))
                .collect::<Vec<_>>()
        );
    }
    assert!(misses.is_empty(), "misses:\n{}", misses.join("\n"));
}
