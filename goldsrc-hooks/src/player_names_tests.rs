use super::*;

fn s(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn rule(parsed: Result<Parsed, String>) -> Rule {
    match parsed.expect("parses").command {
        Command::Add(rule) => rule,
        other => panic!("not an add: {other:?}"),
    }
}

const STEAM: u64 = 76_561_197_960_265_728 + 2 * 5_550_002 + 1;

#[test]
fn add_takes_its_parameters_in_any_order() {
    let want = Rule {
        id: Id::Steam(STEAM),
        demos: vec!["greetz1".to_string()],
        name: "dicE Alex".to_string(),
    };
    // As the console splits STEAM_0:1:5550002 unquoted.
    let id = ["id", "STEAM_0", ":", "1", ":", "5550002"];
    for order in [
        [&id[..], &["in", "greetz1", "to", "dicE Alex"][..]].concat(),
        [&["to", "dicE Alex", "in", "greetz1"][..], &id[..]].concat(),
        [&["in", "greetz1"][..], &id[..], &["to", "dicE Alex"][..]].concat(),
    ] {
        let parsed = parse(&s(&[&["add"][..], &order[..]].concat()));
        assert_eq!(rule(parsed), want, "{order:?}");
    }
}

#[test]
fn a_quoted_name_arrives_whole_and_keeps_its_colons() {
    let parsed = parse(&s(&[
        "add",
        "id",
        "self",
        "in",
        "x",
        "to",
        "dicE[: :]Gorilla[bc",
    ]));
    assert_eq!(rule(parsed).name, "dicE[: :]Gorilla[bc");
}

#[test]
fn an_unquoted_name_with_a_colon_is_glued_back() {
    let parsed = parse(&s(&[
        "add", "id", "3", "in", "x", "to", "dicE[", ":", "]Alex",
    ]));
    assert_eq!(rule(parsed).name, "dicE[:]Alex");
}

#[test]
fn an_unquoted_name_with_a_colon_warns_that_spaces_are_gone() {
    // `to dicE[: :]Alex` typed without quotes.
    let parsed = parse(&s(&[
        "add", "id", "3", "in", "x", "to", "dicE[", ":", ":", "]Alex",
    ]))
    .unwrap();
    assert!(
        parsed
            .warnings
            .iter()
            .any(|w| w.contains("without quotes") && w.contains("\"dicE[::]Alex\"")),
        "{:?}",
        parsed.warnings
    );
    // Quoted, or a glued SteamID or demo list: no such warning.
    for quiet in [
        &["add", "id", "3", "in", "x", "to", "dicE[: :]Alex"][..],
        &[
            "add", "id", "STEAM_0", ":", "1", ":", "5", "in", "a", ",", "b", "to", "x",
        ][..],
    ] {
        let parsed = parse(&s(quiet)).unwrap();
        assert!(
            parsed.warnings.is_empty(),
            "{quiet:?}: {:?}",
            parsed.warnings
        );
    }
}

#[test]
fn in_takes_a_comma_list_and_patterns() {
    // `,` is a token of its own too.
    let parsed = parse(&s(&[
        "add",
        "id",
        "76561197960265729",
        "in",
        "WSOD25_r03_m00cat",
        ",",
        "dod/wsod25_r03_alex.dem",
        ",",
        "wsod25_r04_*",
        "to",
        "x",
    ]));
    let rule = rule(parsed);
    assert_eq!(
        rule.demos,
        ["wsod25_r03_m00cat", "wsod25_r03_alex", "wsod25_r04_*"]
    );
    assert!(rule.applies_to(Some("wsod25_r03_alex")));
    assert!(rule.applies_to(Some("wsod25_r04_bob")));
    assert!(!rule.applies_to(Some("wsod25_r03_bob")));
    assert!(!rule.applies_to(None), "no demo playing");
}

#[test]
fn no_in_means_every_demo() {
    let rule = rule(parse(&s(&["add", "id", "76561197960265729", "to", "x"])));
    assert!(rule.demos.is_empty());
    assert!(rule.applies_to(Some("anything")));
    assert!(rule.applies_to(None));
}

#[test]
fn self_or_a_slot_without_in_is_warned_not_refused() {
    let parsed = parse(&s(&["add", "id", "self", "to", "x"])).unwrap();
    assert_eq!(parsed.warnings, [SELF_WARNING]);
    assert!(matches!(parsed.command, Command::Add(_)));
    let parsed = parse(&s(&["add", "id", "8", "to", "x"])).unwrap();
    assert!(parsed.warnings[0].contains("slot 8 is a different player"));
    for quiet in [
        &["add", "id", "self", "in", "a", "to", "x"][..],
        &["add", "id", "76561197960265729", "to", "x"][..],
    ] {
        assert!(parse(&s(quiet)).unwrap().warnings.is_empty(), "{quiet:?}");
    }
}

#[test]
fn add_without_id_or_to_is_refused() {
    for args in [
        &["add", "to", "x"][..],
        &["add", "id", "self"][..],
        &["add"][..],
    ] {
        let why = parse(&s(args)).unwrap_err();
        assert!(why.contains("add needs `id"), "{args:?}: {why}");
    }
}

#[test]
fn an_unknown_word_is_named() {
    let why = parse(&s(&["add", "id", "self", "vicitm", "3", "to", "x"])).unwrap_err();
    assert!(why.contains("unknown word `vicitm`"), "{why}");
    let why = parse(&s(&["rename", "x"])).unwrap_err();
    assert!(why.contains("unknown word `rename`"), "{why}");
    let why = parse(&s(&["list", "all"])).unwrap_err();
    assert!(why.contains("unknown word `all`"), "{why}");
    // A name with a space typed without quotes: the hint says so.
    let why = parse(&s(&["add", "id", "self", "in", "a", "to", "dicE", "Alex"])).unwrap_err();
    assert!(
        why.contains("unknown word `Alex`") && why.contains("quotes"),
        "{why}"
    );
}

#[test]
fn bad_values_are_refused_with_the_reason() {
    let cases: &[(&[&str], &str)] = &[
        (&["add", "id", "40", "to", "x"], "is not a slot"),
        (&["add", "id", "0", "to", "x"], "is not a slot"),
        (&["add", "id", "bob", "to", "x"], "is not a slot"),
        (
            &["add", "id", "self", "id", "3", "to", "x"],
            "`id` is given twice",
        ),
        (&["add", "id", "self", "to"], "`to` needs a name"),
        (&["add", "id", "self", "to", "50%"], "can't contain %"),
        // Quoted, so the tokenizer leaves it whole.
        (&["add", "id", "self", "in", " , ", "to", "x"], "`in` needs"),
        (&["remove"], "remove needs"),
        (&["remove", "0"], "not a number"),
        (&["remove", "two"], "not a number"),
    ];
    for (args, want) in cases {
        let why = parse(&s(args)).unwrap_err();
        assert!(why.contains(want), "{args:?}: {why}");
    }
    let long = "x".repeat(MAX_NAME_BYTES + 1);
    let why = parse(&s(&["add", "id", "self", "to", &long])).unwrap_err();
    assert!(why.contains("at most 63"), "{why}");
    let longest = "x".repeat(MAX_NAME_BYTES);
    assert!(parse(&s(&["add", "id", "self", "in", "a", "to", &longest])).is_ok());
}

#[test]
fn the_other_subcommands_parse() {
    assert_eq!(parse(&[]).unwrap().command, Command::List);
    assert_eq!(parse(&s(&["LIST"])).unwrap().command, Command::List);
    assert_eq!(parse(&s(&["clear"])).unwrap().command, Command::Clear);
    assert_eq!(
        parse(&s(&["remove", "2"])).unwrap().command,
        Command::Remove(2)
    );
}

#[test]
fn demo_names_compare_by_file_name() {
    assert_eq!(
        demo_key("../../Half-Life/dod/Solifotw-Greetz1.DEM"),
        "solifotw-greetz1"
    );
    assert_eq!(demo_key("folder\\match"), "match");
    assert!(glob("solifotw-*", "solifotw-greetz1"));
    assert!(glob("*greetz*", "solifotw-greetz1"));
    assert!(glob("a*b*c", "axxbyyc"));
    assert!(!glob("a*b*c", "axxbyy"));
    assert!(glob("greetz1", "greetz1"));
    assert!(!glob("greetz1", "greetz12"), "no * means the whole name");
    assert!(glob("*", ""));
}

fn who(slot: i32, steam_id: u64, own_pov: bool) -> Who {
    Who {
        slot,
        steam_id,
        own_pov,
    }
}

fn r(id: Id, demos: &[&str], name: &str) -> Rule {
    Rule {
        id,
        demos: demos.iter().map(|d| d.to_string()).collect(),
        name: name.to_string(),
    }
}

#[test]
fn ids_match_by_slot_steam_id_or_self() {
    let me = who(8, STEAM, true);
    assert!(Id::Slot(8).matches(&me));
    assert!(!Id::Slot(9).matches(&me));
    assert!(Id::Steam(STEAM).matches(&me));
    assert!(!Id::Steam(STEAM + 1).matches(&me));
    assert!(Id::OwnPov.matches(&me));
    assert!(!Id::OwnPov.matches(&who(8, STEAM, false)));
    // A bot has no SteamID, and 0 matches nothing.
    assert!(!Id::Steam(0).matches(&who(3, 0, false)));
}

#[test]
fn a_rename_with_in_beats_one_without_then_the_last_wins() {
    let rules = [
        r(Id::Steam(STEAM), &["greetz1"], "only here"),
        r(Id::Steam(STEAM), &[], "everywhere"),
        r(Id::OwnPov, &[], "self later"),
    ];
    let refs: Vec<&Rule> = rules.iter().collect();
    let me = who(8, STEAM, true);
    assert_eq!(pick(&refs, &me, Some("greetz1")), Some(0));
    assert_eq!(pick(&refs, &me, Some("other")), Some(2));
    assert_eq!(pick(&refs, &who(8, STEAM, false), Some("other")), Some(1));
    assert_eq!(pick(&refs, &who(9, 1, false), Some("greetz1")), None);
}

#[test]
fn adding_the_same_player_and_demos_again_changes_the_name() {
    let mut entries = Vec::new();
    let add = |name: &str| Command::Add(r(Id::OwnPov, &["a"], name));
    apply(&mut entries, add("one")).unwrap();
    let reply = apply(&mut entries, add("one")).unwrap();
    assert!(reply.contains("already"), "{reply}");
    apply(&mut entries, add("two")).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].rule.name, "two");
    assert_eq!(entries[0].name.to_bytes(), b"two");
    apply(&mut entries, Command::Add(r(Id::OwnPov, &[], "three"))).unwrap();
    assert_eq!(entries.len(), 2, "other demos are another rename");
    assert!(
        apply(&mut entries, Command::Remove(3))
            .unwrap_err()
            .contains("1 to 2")
    );
    apply(&mut entries, Command::Remove(1)).unwrap();
    assert_eq!(entries[0].rule.name, "three");
    apply(&mut entries, Command::Clear).unwrap();
    assert!(entries.is_empty());
    assert!(
        apply(&mut entries, Command::Remove(1))
            .unwrap_err()
            .contains("no renames")
    );
}

#[test]
fn a_name_added_twice_is_one_string() {
    let a = intern("same-name-for-the-pool").unwrap();
    let b = intern("same-name-for-the-pool").unwrap();
    assert_eq!(a.as_ptr(), b.as_ptr());
}

#[test]
fn say_text_swaps_the_senders_name_in_the_line() {
    let payload = b"\x08\x02(TEAM) gskiLL  motiv :  njk\n\0";
    let out = rewrite_say_text(payload, b"gskiLL  motiv", b"dicE[: :]Gorilla[bc").unwrap();
    assert_eq!(out, b"\x08\x02(TEAM) dicE[: :]Gorilla[bc :  njk\n\0");
    // Only the first: the message itself may say the name again.
    let out = rewrite_say_text(b"\x01bob : bob\0", b"bob", b"Robert").unwrap();
    assert_eq!(out, b"\x01Robert : bob\0");
    // Not in the line, or no terminator.
    assert_eq!(rewrite_say_text(b"\x01someone : hi\0", b"bob", b"x"), None);
    assert_eq!(
        rewrite_say_text(b"\x01bob : hi", b"bob", b"x").unwrap(),
        b"\x01x : hi\0"
    );
    assert_eq!(rewrite_say_text(b"", b"bob", b"x"), None);
    assert_eq!(rewrite_say_text(b"\x01bob\0", b"", b"x"), None);
}

#[test]
fn text_msg_swaps_arguments_equal_to_a_real_name() {
    let renames: [(&[u8], &[u8]); 1] = [(b"motiv", b"m")];
    let joined = b"\x02#game_joined_game\0motiv\0";
    assert_eq!(
        rewrite_text_msg(joined, &renames).unwrap(),
        b"\x02#game_joined_game\0m\0"
    );
    // The key itself is never swapped, and a partial match isn't one.
    assert_eq!(rewrite_text_msg(b"\x02motiv\0", &renames), None);
    assert_eq!(rewrite_text_msg(b"\x02#k\0motivated\0", &renames), None);
    // Several arguments, the second one renamed.
    assert_eq!(
        rewrite_text_msg(b"\x02#k\0a\0motiv\0", &renames).unwrap(),
        b"\x02#k\0a\0m\0"
    );
    assert_eq!(rewrite_text_msg(b"", &renames), None);
}

#[test]
fn the_hash_ties_a_name_to_the_player_it_was_resolved_for() {
    assert_eq!(name_hash(b"abc"), name_hash(b"abc"));
    assert_ne!(name_hash(b"abc"), name_hash(b"abd"));
}

#[test]
fn the_command_has_its_name() {
    assert_eq!(NAME, "ds_player_change_name");
}
