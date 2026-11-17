use super::*;

fn tokens() -> Vec<&'static str> {
    PATTERN.split_whitespace().collect()
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[test]
fn the_pattern_holds_the_first_site_where_the_table_says() {
    let t = tokens();
    let (first, _) = SITES[0];
    assert_eq!(&t[first..first + 2], &["FF", "15"], "call [imm32]");
    assert_eq!(
        &t[first + CALL_LEN..first + CALL_LEN + 2],
        &["8B", "5C"],
        "the next instruction"
    );
    // The rest lie past the pattern, inside Draw; the verify script
    // checks them against the binary, and `install` checks the bytes
    // before writing.
    for &(offset, _) in &SITES[1..] {
        assert!(offset > t.len());
    }
    assert!(STEP_AT > SITES[SITES.len() - 1].0 + CALL_LEN);
}

#[test]
fn the_slots_are_the_ones_engine_rs_types() {
    use crate::engine::ClEngineFuncsPartial;
    let slot = |offset: usize| offset / size_of::<*const ()>();
    assert_eq!(
        slot(std::mem::offset_of!(
            ClEngineFuncsPartial,
            pfn_draw_console_string
        )),
        Call::Draw.slot()
    );
    assert_eq!(
        slot(std::mem::offset_of!(
            ClEngineFuncsPartial,
            pfn_draw_set_text_color
        )),
        Call::Color.slot()
    );
    assert_eq!(
        slot(std::mem::offset_of!(
            ClEngineFuncsPartial,
            pfn_draw_console_string_len
        )),
        Call::Len.slot()
    );
}

#[test]
fn a_redirect_is_the_same_length_as_the_call_it_replaces() {
    let site = 0x0192_b089;
    let target = 0x6a00_1000;
    let bytes = redirect_bytes(site, target);
    assert_eq!(bytes[0], 0xe8);
    assert_eq!(bytes[5], 0x90);
    let rel = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    assert_eq!((site + 5).wrapping_add(rel as usize), target);
    // And back the other way, as a DLL below client.dll would be.
    let bytes = redirect_bytes(site, 0x0040_1000);
    let rel = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    assert_eq!((site + 5).wrapping_add(rel as usize), 0x0040_1000);
    assert_eq!(
        stock_bytes(0x01a7_6388, Call::Draw.slot()),
        [0xff, 0x15, 0xf4, 0x63, 0xa7, 0x01],
        "the bytes at client+0x2b089"
    );
}

#[test]
fn the_step_stub_reproduces_the_stolen_bytes_and_skips_only_the_substitution() {
    let code = step_stub(0x1111_1111, 0x2222_2222);
    assert_eq!(
        &code[..STEP_STOLEN.len() - 2],
        &STEP_STOLEN[..STEP_STOLEN.len() - 2]
    );
    let jge = code
        .windows(2)
        .position(|w| w == [0x7d, 0x06])
        .expect("a jge");
    let landing = jge + 2 + 6;
    assert_eq!(
        &code[landing..landing + 2],
        &[0x03, 0xd9],
        "jge lands on add ebx, ecx"
    );
    assert_eq!(
        &code[jge + 2..jge + 8],
        &[0x8b, 0x0d, 0x11, 0x11, 0x11, 0x11]
    );
    assert_eq!(&code[landing + 2..], &[0xff, 0x25, 0x22, 0x22, 0x22, 0x22]);
    assert!(STEP_STOLEN.len() >= 5, "room for the jump");
}

#[test]
fn font_arguments_parse_as_typed() {
    assert_eq!(parse_args(&argv(&["default"])), Ok(None));
    assert_eq!(
        parse_args(&argv(&["Verdana", "28", "bold", "shadow"])),
        Ok(Some(FontSpec {
            face: "Verdana".into(),
            tall: 28,
            bold: true,
            shadow: true,
            outline: false,
        }))
    );
    // An unquoted multi-word face.
    let spec = parse_args(&argv(&["Times", "New", "Roman", "20"]))
        .unwrap()
        .unwrap();
    assert_eq!(spec.face, "Times New Roman");
    assert_eq!(spec.to_string(), "\"Times New Roman\" 20");
    assert_eq!(
        parse_args(&argv(&["Verdana", "28", "BOLD", "Outline"]))
            .unwrap()
            .unwrap()
            .to_string(),
        "\"Verdana\" 28 bold outline"
    );
    assert!(parse_args(&argv(&["Verdana"])).is_err(), "no size");
    assert!(parse_args(&argv(&["28"])).is_err(), "no face");
    assert!(parse_args(&argv(&["Verdana", "3"])).is_err(), "too small");
    assert!(parse_args(&argv(&["Verdana", "201"])).is_err(), "too big");
    assert!(
        parse_args(&argv(&["Verdana", "28", "italic"])).is_err(),
        "not a flag"
    );
    assert!(parse_args(&argv(&[])).is_err());
}

#[test]
fn flags_and_weight_are_vgui2s() {
    let plain = parse_args(&argv(&["Verdana", "20"])).unwrap().unwrap();
    assert_eq!((plain.flags(), plain.weight()), (0, 0));
    let all = parse_args(&argv(&["Verdana", "20", "bold", "shadow", "outline"]))
        .unwrap()
        .unwrap();
    assert_eq!(all.flags(), 0x80 | 0x200);
    assert_eq!(all.weight(), 700);
}

#[test]
fn a_font_asked_for_again_reuses_its_handle() {
    let spec = parse_args(&argv(&["Verdana", "20"])).unwrap().unwrap();
    let bold = parse_args(&argv(&["Verdana", "20", "bold"]))
        .unwrap()
        .unwrap();
    let mut kept = Vec::new();
    assert!(keep_handle(
        &mut kept,
        HandleKey::new("Verdana", &spec, true),
        7
    ));
    assert!(keep_handle(
        &mut kept,
        HandleKey::new("Tahoma", &spec, false),
        8
    ));
    assert_eq!(
        kept_handle(&kept, &HandleKey::new("verdana", &spec, true)),
        Some(7),
        "Windows matches a face in any case"
    );
    assert_eq!(
        kept_handle(&kept, &HandleKey::new("Verdana", &spec, false)),
        None,
        "a plain handle is not the widened one"
    );
    assert_eq!(
        kept_handle(&kept, &HandleKey::new("Verdana", &bold, true)),
        None
    );
    assert!(keep_handle(
        &mut kept,
        HandleKey::new("Verdana", &spec, true),
        9
    ));
    assert_eq!(
        kept_handle(&kept, &HandleKey::new("Verdana", &spec, true)),
        Some(7),
        "the first handle stays"
    );
}

#[test]
fn the_handle_cache_is_bounded() {
    let spec = |tall| FontSpec {
        face: "Verdana".into(),
        tall,
        bold: false,
        shadow: false,
        outline: false,
    };
    let mut kept = Vec::new();
    for tall in 0..HANDLES_KEPT as i32 {
        assert!(keep_handle(
            &mut kept,
            HandleKey::new("Verdana", &spec(tall), false),
            1
        ));
    }
    assert!(!keep_handle(
        &mut kept,
        HandleKey::new("Verdana", &spec(999), false),
        2
    ));
    assert_eq!(kept.len(), HANDLES_KEPT);
}

#[test]
fn con_color_parses_as_the_engine_reads_it() {
    assert_eq!(con_color_from("255 180 30"), Some((255, 180, 30)));
    assert_eq!(con_color_from(" 10  20  300 "), Some((10, 20, 255)));
    assert_eq!(con_color_from("255 180"), None);
    assert_eq!(con_color_from("255 180 30 1"), None);
    assert_eq!(con_color_from("red"), None);
    assert_eq!(CON_COLOR_DEFAULT, (255, 180, 30));
}

#[test]
fn colour_components_truncate_to_bytes() {
    assert_eq!(color_byte(1.0), 255);
    assert_eq!(color_byte(0.0), 0);
    assert_eq!(color_byte(0.5), 127);
    assert_eq!(color_byte(2.0), 255);
    assert_eq!(color_byte(-1.0), 0);
    assert_eq!(color_byte(f32::NAN), 0);
}

#[test]
fn names_become_terminated_utf16_with_controls_as_spaces() {
    assert_eq!(
        to_wide(b"m00cat", true),
        vec![0x6d, 0x30, 0x30, 0x63, 0x61, 0x74, 0]
    );
    // UTF-8, as the engine converts it.
    assert_eq!(to_wide("é".as_bytes(), true), vec![0xe9, 0]);
    assert_eq!(to_wide(b"a\tb\x01", true), vec![0x61, 0x20, 0x62, 0x20, 0]);
    assert_eq!(to_wide(b"", true), vec![0]);
}

fn wide(text: &str) -> String {
    let units = to_wide(text.as_bytes(), true);
    String::from_utf16(&units[..units.len() - 1]).unwrap()
}

#[test]
fn the_seven_eighths_name_is_drawn_in_the_chosen_face() {
    // The WSOD25 player whose name broke the line (#1052): U+215E, e2 85 9e.
    let name = "NoGo ! kroD- \u{215e}";
    assert_eq!(&name.as_bytes()[name.len() - 3..], &[0xe2, 0x85, 0x9e]);
    assert_eq!(wide(name), "NoGo ! kroD- 7/8");
    assert!(wide(name).chars().all(|c| c <= '\u{ff}'));
    // A Tahoma face draws it itself.
    let units = to_wide(name.as_bytes(), false);
    assert_eq!(units[units.len() - 2], 0x215e);
    // And the engine's own font gets the same bytes.
    assert_eq!(
        engine_name(name.as_bytes()).as_deref(),
        Some(&b"NoGo ! kroD- 7/8"[..])
    );
}

#[test]
fn compatibility_forms_keep_the_meaning() {
    for (from, to) in [
        ("\u{215b}\u{215c}\u{215d}", "1/83/85/8"),
        ("1\u{2044}2", "1/2"),
        ("x\u{00b2}\u{2074}\u{2089}", "x\u{00b2}49"),
        ("Brand\u{2122}", "BrandTM"),
        ("\u{2163} \u{216b} \u{2170}", "IV XII i"),
        ("\u{fb01}re", "fire"),
        ("\u{ff21}\u{ff42}\u{ff10}\u{ff01}\u{3000}x", "Ab0! x"),
        ("wait\u{2026}", "wait..."),
        ("No\u{2116}1", "NoNo1"),
    ] {
        assert_eq!(wide(from), to, "{from:?}");
    }
}

#[test]
fn characters_without_a_plain_form_are_left_alone() {
    // Accented letters stay themselves (Tahoma draws them), as do symbols
    // with no Latin-1 form, CJK and emoji: measured as drawn instead.
    for name in [
        "\u{0141}ukasz",
        "\u{2605}ace\u{2605}",
        "\u{4e2d}\u{6587}",
        "\u{1f600}",
        "\u{20ac}100",
        "caf\u{e9} & bar \u{00bd}",
    ] {
        assert_eq!(wide(name), name, "{name:?}");
        assert_eq!(engine_name(name.as_bytes()), None, "{name:?}");
    }
    assert!(matches!(with_compat_forms("m00cat"), Cow::Borrowed(_)));
    assert_eq!(engine_name(b"m00cat"), None);
    // Not UTF-8: the engine converts it as it always did.
    assert_eq!(engine_name(b"bad \xe2\x85"), None);
}

#[test]
fn every_form_is_latin1_and_every_listed_character_has_one() {
    for cp in (0x100..0x1_0000).filter_map(char::from_u32) {
        let mut out = String::new();
        if push_compat_form(cp, &mut out) {
            assert!(!out.is_empty(), "{cp:?}");
            assert!(
                out.chars().all(|c| (' '..='\u{ff}').contains(&c)),
                "{cp:?} -> {out:?}"
            );
        }
    }
    for c in ['\u{215e}', '\u{2044}', '\u{2122}', '\u{ff5e}', '\u{217f}'] {
        assert!(push_compat_form(c, &mut String::new()), "{c:?}");
    }
    // Latin-1 itself is never rewritten.
    for c in (0x20u8..=0xff).map(char::from) {
        assert_eq!(wide(&c.to_string()), c.to_string());
    }
}

#[test]
fn the_width_is_what_draw_print_text_advances_past() {
    // A made-up font: 10 px a character, `&` and `⅞` included, with a
    // negative a and c to show all three parts count.
    let abc = |c: u16| {
        if c == u16::from(b'i') {
            (-1, 4, -1)
        } else {
            (1, 8, 1)
        }
    };
    let units = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    assert_eq!(drawn_width(&units("ab"), abc), 20);
    assert_eq!(drawn_width(&units("a&b"), abc), 30, "GetTextSize skips &");
    assert_eq!(
        drawn_width(&units("kroD- \u{215e}"), abc),
        70,
        "nor a character iswprint rejects"
    );
    assert_eq!(drawn_width(&units("ii"), abc), 4);
    assert_eq!(drawn_width(&units(""), abc), 0);
    // Stops at the terminator, as the draw does.
    assert_eq!(drawn_width(&[0x61, 0, 0x62], abc), 10);
    assert_eq!(drawn_width(&units("i"), |_| (-5, 1, -5)), 0);
}

#[test]
fn the_font_manager_is_found_through_both_builds_thunks() {
    // `AddGlyphSetToFont`'s slot as shipped (hw.dll +0x3d90 PRE,
    // +0x22cf60 Anniversary): `call FontManager; mov ecx, eax; call ...`.
    let pre = [
        0x55, 0x8b, 0xec, 0x8b, 0x45, 0x20, 0x8b, 0x4d, 0x1c, 0x8b, 0x55, 0x18, 0x50, 0x8b, 0x45,
        0x14, 0x51, 0x8b, 0x4d, 0x10, 0x52, 0x8b, 0x55, 0x0c, 0x50, 0x8b, 0x45, 0x08, 0x51, 0x52,
        0x50, 0xe8, 0x4c, 0xbc, 0x0c, 0x00, 0x8b, 0xc8, 0xe8, 0xb5, 0xbf, 0x0c, 0x00, 0x5d, 0xc2,
        0x24,
    ];
    let post = [
        0x55, 0x8b, 0xec, 0xff, 0x75, 0x20, 0xff, 0x75, 0x1c, 0xff, 0x75, 0x18, 0xff, 0x75, 0x14,
        0xff, 0x75, 0x10, 0xff, 0x75, 0x0c, 0xff, 0x75, 0x08, 0xe8, 0x43, 0x02, 0x04, 0x00, 0x8b,
        0xc8, 0xe8, 0x8c, 0xfb, 0x03, 0x00, 0x5d, 0xc2, 0x24, 0x00,
    ];
    // PRE: thunk at 0x1d03d90 calls FontManager() at 0x1dcfa00.
    let (at, rel) = manager_call(&pre).unwrap();
    assert_eq!((0x1d0_3d90 + at + 5).wrapping_add(rel as usize), 0x1dc_fa00);
    // Anniversary: 0x1022cf60 calls 0x1026d1c0.
    let (at, rel) = manager_call(&post).unwrap();
    assert_eq!(
        (0x1022_cf60 + at + 5).wrapping_add(rel as usize),
        0x1026_d1c0
    );
    assert_eq!(manager_call(&pre[..0x20]), None, "cut short");
    // `mov eax, 0x24e4ab8; ret` (PRE).
    assert_eq!(
        returns_constant(&[0xb8, 0xb8, 0x4a, 0x4e, 0x02, 0xc3]),
        Some(0x024e_4ab8)
    );
    assert_eq!(
        returns_constant(&[0xb8, 0xb8, 0x4a, 0x4e, 0x02, 0x90]),
        None
    );
    assert_eq!(returns_constant(&[0x55, 0x8b]), None);
}

#[test]
fn only_the_managers_own_split_is_widened() {
    assert!(split_as_expected([0, 0xff, 0x1000, 0x100, 0xffff, 0x2000]));
    // Tahoma itself (one range), a face that failed, the same font twice,
    // or ranges someone else changed: left alone.
    assert!(!split_as_expected([
        0, 0xffff, 0x1000, 0x100, 0xffff, 0x2000
    ]));
    assert!(!split_as_expected([0, 0xff, 0, 0x100, 0xffff, 0x2000]));
    assert!(!split_as_expected([0, 0xff, 0x1000, 0x100, 0xffff, 0x1000]));
    assert!(!split_as_expected([0, 0xff, 0x1000, 0x100, 0xfffe, 0x2000]));
}

#[test]
fn coverage_holds_the_reported_ranges_and_never_a_surrogate() {
    let cov = Coverage::from_ranges([
        (0x20, 0x5f),
        (0x400, 0x100),
        (0xd800, 0x800),
        (0xfff0, 0x20),
    ]);
    assert!(cov.has(u16::from(b'A')) && cov.has(0x7e) && !cov.has(0x7f));
    assert!(cov.has(0x414) && cov.has(0x4ff) && !cov.has(0x500));
    assert!(!cov.has(0xd83d) && !cov.has(0xde00), "half an emoji");
    assert!(cov.has(0xffff), "clipped at the end of the BMP");
}

#[test]
fn names_split_into_runs_by_who_has_the_glyph() {
    let units = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    // Bahnschrift-like: Latin Extended and Cyrillic, not ★ or emoji; no
    // backups, so 1 is the fallback.
    let has = |u: u16| font_for(u, |u| (0x100..0x500).contains(&u), &[]);
    assert_eq!(font_runs(&units("m00cat"), has), vec![(0, 0..6)]);
    assert_eq!(font_runs(&units("\u{141}ukasz"), has), vec![(0, 0..6)]);
    assert_eq!(
        font_runs(&units("\u{2605}Ace\u{2605}"), has),
        vec![(1, 0..1), (0, 1..4), (1, 4..5)]
    );
    // Both halves of a surrogate pair go to the fallback together.
    assert_eq!(
        font_runs(&units("a\u{1f600}b"), has),
        vec![(0, 0..1), (1, 1..3), (0, 3..4)]
    );
    assert_eq!(font_runs(&units(""), has), vec![]);
    assert_eq!(
        font_runs(&[0x61, 0, 0x2605], has),
        vec![(0, 0..1)],
        "stops at the terminator"
    );
}

#[test]
fn a_character_the_face_lacks_goes_to_the_first_backup_that_has_it() {
    // Bahnschrift-like face; a Segoe UI-like backup with arrows and
    // Georgian; a Tahoma-like one with ♪ too.
    let face = |u: u16| (0x100..0x500).contains(&u);
    let segoe = Coverage::from_ranges([(0x2190, 0x10), (0x10d0, 0x30)]);
    let tahoma = Coverage::from_ranges([(0x2190, 0x10), (0x266a, 1)]);
    let backups = [&segoe, &tahoma];
    assert_eq!(font_for(u16::from(b'a'), face, &backups), 0);
    assert_eq!(
        font_for(0xe9, |_| false, &backups),
        0,
        "Latin-1 is the face's"
    );
    assert_eq!(font_for(0x414, face, &backups), 0, "the face's own Д");
    assert_eq!(
        font_for(0x2192, face, &backups),
        1,
        "→: both have it, the first wins"
    );
    assert_eq!(font_for(0x10d0, face, &backups), 1, "Georgian");
    assert_eq!(font_for(0x266a, face, &backups), 2, "♪: only the second");
    assert_eq!(
        font_for(0x2605, face, &backups),
        3,
        "★: nobody, the fallback"
    );
    assert_eq!(font_for(0x2605, face, &[]), 1, "no backups: the fallback");
}

#[test]
fn backup_runs_split_where_the_font_changes() {
    let units = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let face = |u: u16| (0x100..0x500).contains(&u);
    let segoe = Coverage::from_ranges([(0x2190, 0x10)]);
    let which = |u: u16| font_for(u, face, &[&segoe]);
    assert_eq!(
        font_runs(&units("a\u{2192}\u{2192}\u{2605}\u{414}"), which),
        vec![(0, 0..1), (1, 1..3), (2, 3..4), (0, 4..5)]
    );
}

#[test]
fn fallback_arguments_parse_as_typed() {
    assert_eq!(parse_fallback_args(&argv(&["default"])), Ok(vec![]));
    assert_eq!(parse_fallback_args(&argv(&["DEFAULT"])), Ok(vec![]));
    // The console hands a quoted face over as one argument.
    assert_eq!(
        parse_fallback_args(&argv(&["Segoe UI", "Tahoma"])),
        Ok(vec!["Segoe UI".to_string(), "Tahoma".to_string()])
    );
    assert_eq!(
        parse_fallback_args(&argv(&["Arial", "arial", " Tahoma "])),
        Ok(vec!["Arial".to_string(), "Tahoma".to_string()]),
        "named twice: kept once, first place; trimmed"
    );
    assert!(parse_fallback_args(&argv(&[])).is_err());
    assert!(parse_fallback_args(&argv(&[" "])).is_err());
    assert!(parse_fallback_args(&argv(&["a", "b", "c", "d"])).is_ok());
    assert!(parse_fallback_args(&argv(&["a", "b", "c", "d", "e"])).is_err());
    assert!(parse_fallback_args(&argv(&["x".repeat(32).as_str()])).is_err());
}

#[test]
fn status_lists_the_backups_then_the_fallback() {
    assert_eq!(describe_backups(&[]), "fallback = Tahoma (the game's)");
    assert_eq!(
        describe_backups(&argv(&["Segoe UI", "Arial"])),
        "fallback = \"Segoe UI\", \"Arial\", then Tahoma"
    );
    assert_eq!(
        describe_backups(&argv(&["Segoe UI", "tahoma"])),
        "fallback = \"Segoe UI\", \"tahoma\"",
        "Tahoma named: not twice"
    );
}

#[test]
fn the_engine_font_loses_zero_width_characters_rather_than_drawing_spaces() {
    assert_eq!(
        engine_name("zero\u{200b}width\u{200d}join".as_bytes()).as_deref(),
        Some(&b"zerowidthjoin"[..])
    );
    assert_eq!(
        engine_name("\u{feff}a\u{2122}".as_bytes()).as_deref(),
        Some(&b"aTM"[..])
    );
    // Combining marks stay: the engine blanks them, but dropping one
    // changes the name.
    assert_eq!(engine_name("Ze\u{301}ro".as_bytes()), None);
}

#[test]
fn a_character_the_face_has_keeps_its_own_glyph() {
    // Bahnschrift has ™ and €, not ⅞: only ⅞ is swapped.
    let face_has = |c: char| matches!(c, '\u{2122}' | '\u{20ac}');
    assert_eq!(
        with_compat_forms_where("x\u{2122} \u{215e}", face_has),
        "x\u{2122} 7/8"
    );
    assert_eq!(
        with_compat_forms_where("x\u{2122} \u{215e}", |_| false),
        "xTM 7/8"
    );
    assert!(matches!(
        with_compat_forms_where("\u{2122}", face_has),
        Cow::Borrowed(_)
    ));
}

#[test]
fn status_says_the_games_font_until_one_is_set() {
    assert_eq!(describe(), "font = the game's");
}
