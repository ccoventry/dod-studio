use super::*;

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

/// The console's own split: `:` is a token of its own.
fn console(line: &str) -> Vec<String> {
    args(&line.replace(':', " : "))
}

fn rule(line: &str) -> Rule {
    parse(&args(line)).unwrap().rule
}

fn err(line: &str) -> String {
    parse(&args(line)).unwrap_err()
}

const ALEX: u64 = 76_561_197_960_265_728 + 2 * 5_550_002 + 1; // STEAM_0:1:5550002
const BEN: u64 = 76_561_197_960_265_728 + 2 * 5_550_001; // STEAM_0:0:5550001

#[test]
fn parameters_come_in_any_order() {
    let a = rule(
        "killer self victim STEAM_0:0:5550001 at 7:42 in dice-wsod25_r03 with killer STEAM_0:1:5550002 weapon garand",
    );
    let b = rule(
        "in dice-wsod25_r03 at 7:42 victim STEAM_0:0:5550001 killer self with weapon garand killer STEAM_0:1:5550002",
    );
    assert_eq!(a, b);
    assert_eq!(a.killer, Some(Player::OwnPov));
    assert_eq!(a.victim, Some(Player::SteamId(BEN)));
    assert_eq!(a.at, Some(462.0));
    assert_eq!(a.demos, vec!["dice-wsod25_r03".to_string()]);
    assert_eq!(
        a.change,
        Change::Set {
            killer: Some(Player::SteamId(ALEX)),
            victim: None,
            weapon: Some(5),
        }
    );
}

#[test]
fn the_console_split_at_colons_is_put_back() {
    let typed = parse(&console("killer STEAM_0:0:5550001 at 7:42 with victim 3"))
        .unwrap()
        .rule;
    assert_eq!(typed.killer, Some(Player::SteamId(BEN)));
    assert_eq!(typed.at, Some(462.0));
}

#[test]
fn times_are_m_ss_h_mm_ss_or_seconds() {
    assert_eq!(parse_time("7:42"), Some(462.0));
    assert_eq!(parse_time("0:05.5"), Some(5.5));
    assert_eq!(parse_time("1:02:03"), Some(3723.0));
    assert_eq!(parse_time("462"), Some(462.0));
    assert_eq!(parse_time("462.25"), Some(462.25));
    for bad in ["7:72", "-3", "7:", ":30", "a:10", "1:2:3:4", "nan", "inf"] {
        assert_eq!(parse_time(bad), None, "{bad}");
    }
    assert!(err("killer 3 at 7:72 with hide").contains("m:ss or seconds"));
}

#[test]
fn in_takes_a_list_patterns_and_strips_dem() {
    let r = rule("killer 3 in Greetz1.dem,wsod25_r03_* with hide");
    assert_eq!(
        r.demos,
        vec!["greetz1".to_string(), "wsod25_r03_*".to_string()]
    );
    let spaced = rule("killer 3 in greetz1, wsod25_r03_* with hide");
    assert_eq!(spaced.demos, r.demos);
    let spaced = rule("killer 3 in greetz1 ,wsod25_r03_* with hide");
    assert_eq!(spaced.demos, r.demos);
}

#[test]
fn hide_and_weapon_only() {
    assert_eq!(rule("victim 4 with hide").change, Change::Hide);
    assert_eq!(
        rule("killer 4 with weapon d_garand").change,
        Change::Set {
            killer: None,
            victim: None,
            weapon: Some(5)
        }
    );
    assert_eq!(
        rule("killer 4 with weapon 43").change,
        Change::Set {
            killer: None,
            victim: None,
            weapon: Some(43)
        }
    );
}

#[test]
fn refusals_say_why() {
    assert!(err("at 7:42 with hide").contains("`killer` or the `victim`"));
    assert!(err("in greetz1 with killer 3").contains("`killer` or the `victim`"));
    assert!(err("killer 3 with").contains("nothing after `with`"));
    assert!(err("killer 3").contains("no `with`"));
    assert!(err("killer 3 vicitm 4 with hide").contains("unknown word `vicitm`"));
    assert!(err("killer 3 with hide frob").contains("unknown word `frob`"));
    assert!(err("killer 3 with at 7:42 hide").contains("goes before `with`"));
    assert!(err("killer 3 weapon garand with hide").contains("goes after `with`"));
    assert!(err("killer 3 killer 4 with hide").contains("twice"));
    assert!(err("killer 3 with weapon garand weapon kar").contains("twice"));
    assert!(err("killer 3 with hide with hide").contains("twice"));
    assert!(err("killer 3 with hide weapon garand").contains("alone"));
    assert!(err("killer bob with hide").contains("slot number, a SteamID or self"));
    assert!(err("killer 40 with hide").contains("out of range"));
    assert!(err("killer 3 with weapon rocket").contains("no weapon called"));
    assert!(err("killer").contains("needs a value"));
    assert!(err("killer 3 in with hide").contains("`in` needs a value"));
    assert!(err("killer 3 in , with hide").contains("needs a demo name"));
}

#[test]
fn self_or_a_slot_without_in_warns_but_is_kept() {
    let p = parse(&args("killer self victim 4 with killer 7")).unwrap();
    assert_eq!(p.warnings.len(), 3, "{:?}", p.warnings);
    assert!(p.warnings[0].starts_with("self is a different player in each POV demo"));
    assert!(p.warnings[1].starts_with("slot 4 "));
    assert!(p.warnings[2].starts_with("slot 7 "));
    let p = parse(&args("killer self in greetz1 with killer 7")).unwrap();
    assert!(p.warnings.is_empty());
    let p = parse(&args("killer 76561197971365756 with hide")).unwrap();
    assert!(p.warnings.is_empty());
}

#[test]
fn list_shows_the_line_in_usage_order() {
    let r = rule("in a,b* at 462.5 victim self with weapon 15 killer STEAM_0:1:5550002");
    assert_eq!(
        r.describe(),
        "victim self at 7:42.5 in a,b* with killer STEAM_0:1:5550002 weapon 15"
    );
    assert_eq!(
        rule("killer 3 with weapon 5").describe(),
        "killer 3 with weapon garand"
    );
    assert_eq!(
        rule("killer 3 at 1:02:03 with hide").describe(),
        "killer 3 at 1:02:03 with hide"
    );
}

#[test]
fn globs() {
    assert!(glob("greetz1", "greetz1"));
    assert!(!glob("greetz1", "greetz10"));
    assert!(glob("wsod25_r03_*", "wsod25_r03_alex"));
    assert!(glob("*greetz*", "solifotw-greetz1"));
    assert!(glob("g?eetz1", "greetz1"));
    assert!(glob("*", ""));
    assert!(!glob("a*b", "acd"));
    assert!(glob("a*b*c", "aXbYbZc"));
    assert_eq!(
        demo_stem("../../Half-Life - PRE/dod/Solifotw-Greetz1.DEM"),
        "solifotw-greetz1"
    );
    assert_eq!(demo_stem("demos\\x.dem"), "x");
}

fn who(slot: i32, steam_id: u64, own: bool) -> Who {
    Who {
        slot,
        steam_id,
        is_own_pov: own,
    }
}

/// A POV demo: slot 8 is the recorder, slot 3 is Ben, slot 5 is Alex.
fn pov_slot_of(p: Player) -> Option<i32> {
    match p {
        Player::Slot(s) => Some(s),
        Player::OwnPov => Some(8),
        Player::SteamId(id) if id == BEN => Some(3),
        Player::SteamId(id) if id == ALEX => Some(5),
        Player::SteamId(_) => None,
    }
}

fn at(t: f64, demo: Option<&str>) -> Context<'_> {
    Context {
        window: Some((t, t)),
        demo,
    }
}

#[test]
fn the_stolen_kill_becomes_the_teammates() {
    let rules = vec![rule(
        "killer self victim STEAM_0:0:5550001 at 7:42 in solifotw-greetz1 with killer STEAM_0:1:5550002",
    )];
    let (me, ben) = (who(8, 1, true), who(3, BEN, false));
    let ctx = at(463.5, Some("solifotw-greetz1"));
    assert_eq!(
        decide(&rules, [8, 3, 5], &me, &ben, &ctx, pov_slot_of),
        Outcome::Rewrite(0, [5, 3, 5])
    );
    // Outside the 2 s window, in another demo, or another victim: kept.
    let late = at(464.5, Some("solifotw-greetz1"));
    assert_eq!(
        decide(&rules, [8, 3, 5], &me, &ben, &late, pov_slot_of),
        Outcome::Keep
    );
    let other = at(462.0, Some("solifotw-greetz2"));
    assert_eq!(
        decide(&rules, [8, 3, 5], &me, &ben, &other, pov_slot_of),
        Outcome::Keep
    );
    let none = at(462.0, None);
    assert_eq!(
        decide(&rules, [8, 3, 5], &me, &ben, &none, pov_slot_of),
        Outcome::Keep
    );
    let someone = who(4, 77, false);
    assert_eq!(
        decide(&rules, [8, 4, 5], &me, &someone, &ctx, pov_slot_of),
        Outcome::Keep
    );
    // No clock: a rule with `at` can't match.
    let no_clock = Context {
        window: None,
        demo: Some("solifotw-greetz1"),
    };
    assert_eq!(
        decide(&rules, [8, 3, 5], &me, &ben, &no_clock, pov_slot_of),
        Outcome::Keep
    );
}

#[test]
fn a_kill_read_during_a_seek_step_matches_within_the_span() {
    let rules = vec![rule("killer 8 at 100 with hide")];
    let span = Context {
        window: Some((95.0, 100.0 + 3.0)),
        demo: None,
    };
    let k = who(8, 0, false);
    let v = who(2, 0, false);
    assert_eq!(
        decide(&rules, [8, 2, 1], &k, &v, &span, pov_slot_of),
        Outcome::Hide(0)
    );
    let before = Context {
        window: Some((85.0, 97.9)),
        demo: None,
    };
    assert_eq!(
        decide(&rules, [8, 2, 1], &k, &v, &before, pov_slot_of),
        Outcome::Keep
    );
}

#[test]
fn parts_left_out_stay_and_the_first_match_wins() {
    let rules = vec![
        rule("victim 2 with weapon kar"),
        rule("victim 2 with hide"),
        rule("killer 9 with victim 4"),
    ];
    let ctx = Context::default();
    let (k, v) = (who(8, 0, false), who(2, 0, false));
    assert_eq!(
        decide(&rules, [8, 2, 5], &k, &v, &ctx, pov_slot_of),
        Outcome::Rewrite(0, [8, 2, 10])
    );
    let (k, v) = (who(9, 0, false), who(3, 0, false));
    assert_eq!(
        decide(&rules, [9, 3, 5], &k, &v, &ctx, pov_slot_of),
        Outcome::Rewrite(2, [9, 4, 5])
    );
}

#[test]
fn a_new_player_missing_from_the_demo_leaves_the_line() {
    let rules = vec![rule("killer 8 with killer STEAM_0:1:1")];
    let (k, v) = (who(8, 0, false), who(2, 0, false));
    assert!(matches!(
        decide(&rules, [8, 2, 5], &k, &v, &Context::default(), pov_slot_of),
        Outcome::Unresolved(0, Player::SteamId(_))
    ));
}

#[test]
fn a_steam_id_matches_whatever_slot_the_player_has() {
    let rules = vec![rule("victim STEAM_0:0:5550001 with hide")];
    let k = who(8, 1, true);
    for slot in [3, 17] {
        let v = who(slot, BEN, false);
        assert_eq!(
            decide(
                &rules,
                [8, slot as u8, 5],
                &k,
                &v,
                &Context::default(),
                pov_slot_of
            ),
            Outcome::Hide(0)
        );
    }
}

#[test]
fn the_same_line_twice_parses_equal() {
    // `command` keeps one copy of a line a batch sends again per demo.
    assert_eq!(
        rule("killer 3 at 7:42 with hide"),
        rule("at 462 killer 3 with hide")
    );
}

#[test]
fn clock_text_round_trips_through_parse_time() {
    for t in [0.0, 5.5, 462.0, 3723.0, 59.9] {
        assert_eq!(parse_time(&clock_text(t)), Some(t), "{t}");
    }
}
