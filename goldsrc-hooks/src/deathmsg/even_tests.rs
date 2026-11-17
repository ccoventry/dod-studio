use super::*;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// The table `killfeed_icons.py --even` wrote from the POST install,
/// cut to three icons.
const TABLE_TEXT: &str = "// DoD Studio's kill-feed atlas (#1022), written by killfeed_icons.py --even.\n\
        // ds_hud_deathmsg layout even draws each icon from sprites/dodstudio_killfeed.spr: name frame x y w h.\n\
        dodstudio_killfeed 1 2\n\
        d_30cal 0 156 226 71 26\n\
        d_mp44 1 2 2 59 26\n\
        d_world 1 42 58 37 26\n";

fn sprite(frames: &[(i32, i32)]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"IDSP");
    for v in [2i32, 2, 3] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&181.0f32.to_le_bytes());
    for v in [256i32, 256, frames.len() as i32] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&0.0f32.to_le_bytes());
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(&256i16.to_le_bytes());
    b.extend_from_slice(&[0u8; 768]);
    for &(w, h) in frames {
        for v in [0, -w / 2, h / 2, w, h] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend(std::iter::repeat_n(255u8, (w * h) as usize));
    }
    b
}

#[test]
fn layout_arguments_parse_as_typed() {
    assert_eq!(parse_args(&argv(&["even"])), Ok(Layout::Even));
    assert_eq!(parse_args(&argv(&["EVEN"])), Ok(Layout::Even));
    assert_eq!(parse_args(&argv(&["default"])), Ok(Layout::Default));
    assert!(parse_args(&argv(&[])).is_err());
    assert!(parse_args(&argv(&["uniform"])).is_err());
    assert!(parse_args(&argv(&["even", "now"])).is_err());
}

#[test]
fn the_table_reads_as_the_tool_writes_it() {
    let t = parse_table(TABLE_TEXT).unwrap();
    assert_eq!(t.frames, 2);
    assert_eq!(t.boxes.len(), 3);
    assert_eq!(
        t.boxes[1],
        (
            "d_mp44".to_string(),
            AtlasBox {
                frame: 1,
                rect: WRect {
                    left: 2,
                    right: 61,
                    top: 2,
                    bottom: 28
                }
            }
        )
    );
}

#[test]
fn a_bad_table_is_refused_with_its_line() {
    let bad = |body: &str| parse_table(&format!("dodstudio_killfeed 1 2\n{body}"));
    assert!(parse_table("").is_err());
    assert!(parse_table("// only a comment\n").is_err());
    assert!(
        parse_table("dodstudio_killfeed 2 1\nd_kar 0 0 0 1 1\n").is_err(),
        "a version this DLL doesn't read"
    );
    assert!(
        parse_table("dodstudio_killfeed 1 0\nd_kar 0 0 0 1 1\n").is_err(),
        "no frames"
    );
    assert!(
        parse_table("dodstudio_killfeed 1 33\nd_kar 0 0 0 1 1\n").is_err(),
        "more frames than the bits"
    );
    assert!(parse_table("hud 1 1\nd_kar 0 0 0 1 1\n").is_err());
    assert!(bad("").is_err(), "no boxes");
    assert!(bad("d_kar 2 0 0 10 10").is_err(), "frame past the count");
    assert!(bad("d_kar 0 0 0 0 10").is_err(), "empty box");
    assert!(bad("d_kar 0 4090 0 10 10").is_err(), "past any frame");
    assert!(bad("d_kar 0 -1 0 10 10").is_err(), "negative");
    assert!(bad("d_kar 0 0 0 10").is_err(), "a number short");
    assert!(bad("d_kar 0 0 0 10 10 3").is_err(), "a number over");
    assert!(bad("d_kar 0 0 0 10 x").is_err());
    assert!(bad("weapon_kar 0 0 0 10 10").is_err(), "not a d_ name");
    assert!(
        bad("d_aaaaaaaaaaaaaaaaaaaaaaaa 0 0 0 10 10").is_err(),
        "longer than 24 bytes"
    );
    assert!(
        bad("d_kar 0 0 0 10 10\nD_KAR 1 0 0 10 10").is_err(),
        "twice"
    );
    let err = bad("d_kar 0 0 0 10 10\nd_mp40 9 0 0 10 10").unwrap_err();
    assert!(err.starts_with("line 3:"), "{err}");
}

#[test]
fn the_sprite_is_checked_before_the_engine_reads_it() {
    let ok = sprite(&[(256, 256), (256, 256)]);
    assert_eq!(
        parse_sprite(&ok),
        Ok(SpriteInfo {
            frames: vec![(256, 256), (256, 256)]
        })
    );
    assert!(parse_sprite(b"IDSQ").is_err());
    assert!(parse_sprite(&ok[..0x20]).is_err(), "header cut short");
    assert!(
        parse_sprite(&ok[..ok.len() - 1]).is_err(),
        "pixels cut short"
    );
    let mut additive = ok.clone();
    additive[12] = 2;
    assert!(parse_sprite(&additive).is_err(), "not alpha test");
    let mut grouped = ok.clone();
    grouped[0x2a + 768] = 1;
    assert!(parse_sprite(&grouped).is_err(), "a frame group");
    let mut huge = ok.clone();
    huge[0x2a + 768 + 12..0x2a + 768 + 16].copy_from_slice(&i32::MAX.to_le_bytes());
    assert!(parse_sprite(&huge).is_err(), "a frame too wide");
    let mut none = ok.clone();
    none[0x1c..0x20].copy_from_slice(&0i32.to_le_bytes());
    assert!(parse_sprite(&none).is_err(), "no frames");
    assert!(
        parse_sprite(&sprite(&[(1, 1); 33])).is_err(),
        "more frames than the bits"
    );
}

#[test]
fn the_table_must_fit_the_sprite() {
    let t = parse_table(TABLE_TEXT).unwrap();
    let two = parse_sprite(&sprite(&[(256, 256), (256, 256)])).unwrap();
    assert_eq!(check_fit(&t, &two), Ok(()));
    let one = parse_sprite(&sprite(&[(256, 256)])).unwrap();
    assert!(check_fit(&t, &one).is_err(), "frame count differs");
    let small = parse_sprite(&sprite(&[(256, 256), (60, 60)])).unwrap();
    assert!(
        check_fit(&t, &small).is_err(),
        "d_world's box runs off frame 1"
    );
}

#[test]
fn each_sprite_index_gets_its_box_by_name() {
    let t = parse_table(TABLE_TEXT).unwrap();
    let mut names = vec![0u8; 5 * SPRITE_NAME_LEN];
    for (i, n) in ["number_0", "d_mp44", "D_30CAL", "d_world", "d_skull"]
        .iter()
        .enumerate()
    {
        names[i * SPRITE_NAME_LEN..i * SPRITE_NAME_LEN + n.len()].copy_from_slice(n.as_bytes());
    }
    let map = boxes_by_index(&names, 5, &t);
    assert_eq!(map.len(), 5);
    assert_eq!(map[0], None, "not in the table");
    assert_eq!(map[1], Some(t.boxes[1].1));
    assert_eq!(map[2], Some(t.boxes[0].1), "case aside");
    assert_eq!(map[3], Some(t.boxes[2].1));
    assert_eq!(map[4], None, "d_skull isn't in this table");
    // A count past the array gives no box rather than a read past it.
    assert_eq!(boxes_by_index(&names, 7, &t)[6], None);
    assert_eq!(index_of(&names, 5, "d_world"), Some(3));
    assert_eq!(index_of(&names, 5, "D_MP44"), Some(1));
    assert_eq!(index_of(&names, 5, "d_garandbutt"), None);
    assert_eq!(
        index_of(&names, 9, "d_garandbutt"),
        None,
        "no read past the array"
    );
}

#[test]
fn the_butt_variants_count_only_as_a_full_set_of_one_size() {
    let full = parse_table(
        "dodstudio_killfeed 1 2\n\
             d_garandbutt 1 2 2 53 26\n\
             d_garandbutt_us 1 57 2 53 26\n\
             d_k43butt_axis 1 112 2 53 26\n\
             d_k43butt_us 1 167 2 53 26\n\
             d_mp44 0 2 2 59 26\n",
    )
    .unwrap();
    let boxes = butt_boxes(&full).unwrap();
    assert_eq!(boxes[0], full.boxes[0].1, "hud.txt's own first");
    assert_eq!(boxes[3], full.boxes[3].1);
    // An atlas from before #1089: the British lines draw the US heads.
    assert_eq!(boxes[4], boxes[1], "Garand, British -> Garand, US");
    assert_eq!(boxes[5], boxes[3], "K43, British -> K43, US");
    let british = parse_table(
        "dodstudio_killfeed 1 2\n\
             d_garandbutt 1 2 2 53 26\n\
             d_garandbutt_us 1 57 2 53 26\n\
             d_k43butt_axis 1 112 2 53 26\n\
             d_k43butt_us 1 167 2 53 26\n\
             d_garandbutt_brit 1 2 30 53 26\n\
             d_k43butt_brit 1 57 30 53 26\n",
    )
    .unwrap();
    let boxes = butt_boxes(&british).unwrap();
    assert_eq!(boxes[4], british.boxes[4].1);
    assert_eq!(boxes[5], british.boxes[5].1);
    let narrow_british = parse_table(
        "dodstudio_killfeed 1 2\n\
             d_garandbutt 1 2 2 53 26\n\
             d_garandbutt_us 1 57 2 53 26\n\
             d_k43butt_axis 1 112 2 53 26\n\
             d_k43butt_us 1 167 2 53 26\n\
             d_garandbutt_brit 1 2 30 50 26\n\
             d_k43butt_brit 1 57 30 53 26\n",
    )
    .unwrap();
    assert_eq!(butt_boxes(&narrow_british), None);
    // An atlas from before #1088: only d_garandbutt.
    assert_eq!(butt_boxes(&parse_table(TABLE_TEXT).unwrap()), None);
    let uneven = parse_table(
        "dodstudio_killfeed 1 2\n\
             d_garandbutt 1 2 2 53 26\n\
             d_garandbutt_us 1 57 2 53 26\n\
             d_k43butt_axis 1 112 2 50 26\n\
             d_k43butt_us 1 167 2 53 26\n",
    )
    .unwrap();
    assert_eq!(
        butt_boxes(&uneven),
        None,
        "a narrower box would not fit the line"
    );
}

#[test]
fn an_atlas_rebuilt_after_the_engine_uploaded_it_is_refused() {
    let spr = sprite(&[(256, 256)]);
    let first = files_key(&spr, TABLE_TEXT.as_bytes());
    assert_ne!(first, 0, "0 means nothing uploaded yet");
    assert_eq!(check_unchanged(0, first), Ok(()), "the first load");
    assert_eq!(
        check_unchanged(first, first),
        Ok(()),
        "the same files again"
    );
    // Two boxes swapped in the table, the sprite's bytes the same: the
    // kept pixels would draw the wrong icons.
    let swapped = TABLE_TEXT
        .replace("d_mp44", "d_tmp")
        .replace("d_30cal", "d_mp44");
    let later = files_key(&spr, swapped.as_bytes());
    assert!(check_unchanged(first, later).is_err());
    let mut repacked = spr.clone();
    *repacked.last_mut().unwrap() = 0;
    assert!(check_unchanged(first, files_key(&repacked, TABLE_TEXT.as_bytes())).is_err());
}

#[test]
fn a_24_byte_name_has_no_terminator() {
    let full = [b'd'; SPRITE_NAME_LEN];
    assert_eq!(entry_name(&full).len(), SPRITE_NAME_LEN);
    assert_eq!(entry_name(b"d_kar\0junk"), b"d_kar");
}

#[test]
fn every_line_steps_the_same_whatever_the_icon() {
    // The line step is the copy's height (font.rs's stub), so 26 rows for
    // every box: 65 px at scale 2.5, 26 at 1.
    let t = parse_table(TABLE_TEXT).unwrap();
    for s in [1.0f32, 2.5, 3.0] {
        let steps: Vec<i32> = t
            .boxes
            .iter()
            .map(|(_, b)| {
                let r = line_rect(*b, s, 18);
                r.bottom - r.top
            })
            .collect();
        assert!(steps.iter().all(|&h| h == steps[0]), "scale {s}: {steps:?}");
    }
    let mp44 = line_rect(t.boxes[1].1, 2.5, 0);
    assert_eq!((mp44.right - mp44.left, mp44.bottom - mp44.top), (148, 65));
    assert_eq!(super::super::scale::line_step(65, 33), 65);
}

#[test]
fn gap_arguments_parse_as_typed() {
    assert_eq!(parse_gap(&argv(&["14"])), Ok(Some(14)));
    assert_eq!(parse_gap(&argv(&["0"])), Ok(Some(0)));
    assert_eq!(parse_gap(&argv(&["64"])), Ok(Some(64)));
    assert_eq!(parse_gap(&argv(&["DEFAULT"])), Ok(None));
    assert!(parse_gap(&argv(&[])).is_err());
    assert!(parse_gap(&argv(&["-1"])).is_err());
    assert!(parse_gap(&argv(&["65"])).is_err());
    assert!(parse_gap(&argv(&["1.5"])).is_err());
    assert!(parse_gap(&argv(&["14", "px"])).is_err());
}

#[test]
fn a_gap_is_the_same_both_sides_counted_from_the_outline() {
    // The game's: nothing added to its 5 px before the icon and none after.
    assert_eq!(gap_extras(None, 0, 0), (0, 0));
    assert_eq!(
        gap_extras(None, 2, 1),
        (0, 0),
        "an outline or a shadow alone moves nothing"
    );
    // 14 px: the killer's 5 becomes 14, the victim gets 14.
    assert_eq!(gap_extras(Some(14), 0, 0), (9, 14));
    // A 2 px outline reaches 2 px past each name: the gap starts there.
    assert_eq!(gap_extras(Some(14), 2, 0), (11, 16));
    // A shadow reaches right: the killer's side only.
    assert_eq!(gap_extras(Some(14), 2, 1), (12, 16));
    // 0 pulls the icon up to the killer's name, 5 px nearer than the game.
    assert_eq!(gap_extras(Some(0), 0, 0), (-GAME_GAP_BEFORE, 0));
    // So a line is as wide as its names, its icon and twice the visible gap.
    for (g, outline, shadow) in [(0, 0, 0), (6, 1, 1), (14, 2, 1), (MAX_GAP, 4, 0)] {
        let (before, after) = gap_extras(Some(g), outline, shadow);
        assert_eq!(GAME_GAP_BEFORE + before - outline - shadow, g);
        assert_eq!(after - outline, g);
    }
}

#[test]
fn the_default_layout_draws_nothing_of_the_atlas() {
    assert_eq!(describe(), "layout = default");
    assert!(!ready());
    assert_eq!(key(), 0);
    assert_eq!(box_for(0), None);
    poll();
    assert!(!ready(), "nothing to follow without `layout even`");
}

/// Studio hears each reason by its own word (#1339), and the state a
/// reason sends never reads as `default` or `even`.
#[test]
fn each_reason_reaches_studio_by_its_word() {
    use super::super::layout_report::{DEFAULT, DRAWING, WAITING, label};
    assert_eq!(report_state(false, Wait::NoHd), DEFAULT);
    assert_eq!(report_state(true, Wait::None), DRAWING);
    for (wait, code) in [
        (Wait::Files, "files"),
        (Wait::HdOff, "hd_off"),
        (Wait::NoHd, "no_hd"),
        (Wait::NoNames, "no_names"),
        (Wait::NoHooks, "no_hooks"),
    ] {
        let state = report_state(true, wait);
        assert!(state >= WAITING, "{wait:?}");
        assert_eq!(Wait::from_u8(state - WAITING), wait);
        assert_eq!(wait.code(), code);
        assert!(
            label(state, wait.code(), "ultrasharp", "why")
                .starts_with(&format!("KILLFEED_LAYOUT\twaiting\treason={code}\t"))
        );
    }
}
