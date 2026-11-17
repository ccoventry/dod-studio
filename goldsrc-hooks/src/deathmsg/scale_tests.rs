use super::*;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// `d_skull` and `d_garand` from DoD's hud.txt (640 set).
const SKULL: WRect = WRect {
    left: 328,
    right: 328 + 74,
    top: 326,
    bottom: 326 + 52,
};
const GARAND: WRect = WRect {
    left: 27,
    right: 27 + 201,
    top: 108,
    bottom: 108 + 34,
};

#[test]
fn a_scaled_rect_keeps_its_corner_and_grows_right_and_down() {
    assert_eq!(scale_rect(SKULL, 1.0), SKULL);
    let twice = scale_rect(SKULL, 2.0);
    assert_eq!(
        twice,
        WRect {
            left: 328,
            right: 328 + 148,
            top: 326,
            bottom: 326 + 104
        }
    );
    // 1.5 x 201 = 301.5 rounds up; 1.5 x 34 = 51 exactly.
    let garand = scale_rect(GARAND, 1.5);
    assert_eq!(garand.right - garand.left, 302);
    assert_eq!(garand.bottom - garand.top, 51);
    // An empty rect stays empty rather than becoming one pixel.
    let empty = WRect::default();
    assert_eq!(scale_rect(empty, 3.0), empty);
}

#[test]
fn the_layout_in_the_wrect_order_the_game_uses() {
    // `left, right, top, bottom`: the anchor reads +4 (right) then +0
    // (left), the step +0xc (bottom) minus +8 (top).
    assert_eq!(std::mem::offset_of!(WRect, left), 0);
    assert_eq!(std::mem::offset_of!(WRect, right), 4);
    assert_eq!(std::mem::offset_of!(WRect, top), 8);
    assert_eq!(std::mem::offset_of!(WRect, bottom), 12);
    assert_eq!(size_of::<WRect>(), 16, "VidInit's shl 4");
}

#[test]
fn the_unscaled_rect_comes_from_the_table_and_the_division_agrees() {
    let stock = [SKULL, GARAND, SKULL];
    for s in [1.5, 2.0, 3.0] {
        let copy: Vec<WRect> = stock.iter().map(|r| scale_rect(*r, s)).collect();
        for (i, r) in stock.iter().enumerate() {
            assert_eq!(find_unscaled(copy[i], &copy, &stock), Some(*r));
            assert_eq!(unscale_rect(copy[i], s), *r, "scale {s}, rect {i}");
        }
    }
    assert_eq!(
        find_unscaled(WRect::default(), &[scale_rect(SKULL, 2.0)], &[SKULL]),
        None
    );
}

#[test]
fn the_quad_lands_where_the_scaled_rect_advances() {
    // The engine draws (x+0.5)..(x+0.5+w); under the matrix it covers
    // S x that, which is the scaled rect's width up to rounding.
    for s in [1.0f32, 1.5, 2.0, 3.0] {
        let scaled = scale_rect(GARAND, s);
        let (start, end) = quad_extent(100, GARAND.right - GARAND.left, s);
        assert_eq!(start, 100.5);
        let advance = (scaled.right - scaled.left) as f32;
        assert!(
            (end - start - advance).abs() <= 0.5,
            "scale {s}: quad {} px, advance {advance}",
            end - start
        );
    }
}

#[test]
fn the_step_is_the_taller_and_the_icon_is_centred_only_under_a_taller_font() {
    assert_eq!(line_step(104, 0), 104, "no font: the icon's own height");
    assert_eq!(line_step(104, 28), 104);
    assert_eq!(line_step(40, 60), 60);
    assert_eq!(
        icon_y(20, 104, 28),
        20,
        "icon taller: at the top, as the game"
    );
    assert_eq!(icon_y(20, 40, 60), 30, "font taller: centred");
    assert_eq!(icon_y(20, 41, 60), 29, "odd gap rounds toward the top");
    assert_eq!(icon_y(20, 60, 60), 20);
}

#[test]
fn names_are_centred_on_the_line_beside_a_taller_icon() {
    // `scale 2` + `font "Verdana" 28`: a 2x skull is 104 px, the line
    // 104, the names (104 - 28) / 2 = 38 px down.
    let skull2 = scale_rect(SKULL, 2.0);
    let icon = skull2.bottom - skull2.top;
    assert_eq!(line_step(icon, 28), 104);
    assert_eq!(name_y(20, icon, 28, 28), 58);
    // `scale 2`, no font: the engine font's own height (16 px here) is
    // centred on the 104 px line, and the line is the icon's.
    assert_eq!(line_step(icon, 0), 104);
    assert_eq!(name_y(20, icon, 0, 16), 64);
    // An odd gap rounds toward the top, as the icon's does.
    assert_eq!(name_y(20, 41, 0, 16), 32);
}

#[test]
fn names_stay_at_the_top_when_the_font_is_the_taller() {
    // `scale 1` + `font "Verdana" 60`: the line is the font's, so the
    // names fill it and the icon is the one centred.
    assert_eq!(line_step(40, 60), 60);
    assert_eq!(name_y(20, 40, 60, 60), 20);
    assert_eq!(icon_y(20, 40, 60), 30);
    // Unknown text height (the stub or the length call not run yet):
    // the game's y.
    assert_eq!(name_y(20, 104, 0, 0), 20);
    // Text taller than the line cannot go up past its top.
    assert_eq!(name_y(20, 10, 0, 16), 20);
}

#[test]
fn scale_1_with_no_font_draws_the_names_where_the_game_does() {
    assert_eq!(scale(), 1.0);
    assert_eq!(font::tall(), 0);
    // Whatever the last line's icon was, the stock layout is untouched.
    ICON_TALL.store(104, Ordering::Relaxed);
    assert_eq!(name_draw_y(20, 16), 20);
    assert_eq!(name_draw_y(20, 0), 20);
    // And a scaled rect at 1 is the game's.
    assert_eq!(scale_rect(GARAND, 1.0), GARAND);
    assert_eq!(icon_y(20, 34, 0), 20);
}

#[test]
fn the_victim_keeps_clear_of_the_right_edge() {
    // 2560x1440 with the WSOD25 font (Bahnschrift SemiBold 32): 16 px.
    assert_eq!(right_margin(2560, 32), 16);
    assert_eq!(right_margin(1920, 28), 12);
    assert_eq!(right_margin(1280, 16), 8);
    // A huge face needs more than the screen's share.
    assert_eq!(right_margin(1920, 120), 30);
    // Unknown sizes never pull the line right of the game's.
    assert_eq!(right_margin(0, 0), 0);
    assert_eq!(right_margin(-1, -5), 0);
    // The game's own layout at scale 1 with no font.
    assert!(!layout_is_ours());
}

#[test]
fn the_line_stub_reproduces_the_stolen_bytes_around_the_height_store() {
    let code = line_stub(0x4444_4444, 0x3333_3333, 0x1111_1111, 0x2222_2222);
    assert_eq!(
        &code[..6],
        &[0x89, 0x3d, 0x44, 0x44, 0x44, 0x44],
        "mov [ICON_SLOT], edi: the line's slot offset"
    );
    let code = &code[6..];
    assert_eq!(
        &code[..6],
        &[0x89, 0x1d, 0x33, 0x33, 0x33, 0x33],
        "mov [ICON_INDEX], ebx: the index, before the shift"
    );
    let code = &code[6..];
    assert_eq!(&code[..3], &LINE_STOLEN[..3], "shl ebx, 4 next");
    assert_eq!(
        &code[3..11],
        &[0x8b, 0x54, 0x03, 0x0c, 0x2b, 0x54, 0x03, 0x08],
        "edx = [ebx+eax+0xc] - [ebx+eax+8]: bottom - top"
    );
    assert_eq!(&code[11..17], &[0x89, 0x15, 0x11, 0x11, 0x11, 0x11]);
    assert_eq!(&code[17..20], &LINE_STOLEN[3..], "add esp, 0xc last");
    assert_eq!(&code[20..], &[0xff, 0x25, 0x22, 0x22, 0x22, 0x22]);
    assert!(LINE_STOLEN.len() >= 5, "room for the jump");
    // The stub's reads are the rect's top and bottom.
    assert_eq!(std::mem::offset_of!(WRect, top), 8);
    assert_eq!(std::mem::offset_of!(WRect, bottom), 0xc);
}

#[test]
fn the_line_is_known_by_the_stubs_index_only_when_the_copy_agrees() {
    let copy = [SKULL, scale_rect(GARAND, 2.0), SKULL];
    assert_eq!(line_entry(1, copy[1], &copy), Some(1));
    assert_eq!(line_entry(2, SKULL, &copy), Some(2), "not the first match");
    assert_eq!(
        line_entry(1, SKULL, &copy),
        None,
        "a rect the copy doesn't hold there"
    );
    assert_eq!(line_entry(-1, SKULL, &copy), None, "the stub not run yet");
    assert_eq!(line_entry(3, SKULL, &copy), None, "past the copy");
}

#[test]
fn an_atlas_box_replaces_the_game_rect_in_the_copy() {
    use super::super::even::AtlasBox;
    // d_mp44 as the POST atlas holds it: 59 x 26 at 2,2 of frame 1.
    let mp44 = AtlasBox {
        frame: 1,
        rect: WRect {
            left: 2,
            right: 61,
            top: 2,
            bottom: 28,
        },
    };
    let stock_mp44 = WRect {
        left: 167,
        right: 167 + 89,
        top: 23,
        bottom: 23 + 25,
    };
    assert_eq!(copy_rect(stock_mp44, Some(mp44), 1.0, 0), mp44.rect);
    let big = copy_rect(stock_mp44, Some(mp44), 2.5, 0);
    assert_eq!((big.right - big.left, big.bottom - big.top), (148, 65));
    assert_eq!(
        copy_rect(stock_mp44, None, 2.5, 0),
        scale_rect(stock_mp44, 2.5)
    );
    assert_eq!(
        copy_rect(stock_mp44, None, 1.0, 0),
        stock_mp44,
        "no box: the game's"
    );
    // The victim's side of a gap widens the advance after an atlas icon
    // only: the box's corner and height stay, so the draw and the step
    // don't move.
    let gapped = copy_rect(stock_mp44, Some(mp44), 2.5, 16);
    assert_eq!(
        (gapped.left, gapped.top, gapped.bottom),
        (big.left, big.top, big.bottom)
    );
    assert_eq!(gapped.right - gapped.left, 148 + 16);
    assert_eq!(
        copy_rect(stock_mp44, None, 2.5, 16),
        scale_rect(stock_mp44, 2.5),
        "the game's icons keep the game's spacing"
    );
}

#[test]
fn the_names_follow_the_rects_in_chud() {
    assert_eq!(NAMES_IN_HUD - RECTS_IN_HUD, 4);
    assert_eq!(super::super::even::SPRITE_NAME_LEN, 24);
}

#[test]
fn scale_arguments_parse_as_typed() {
    assert_eq!(parse_args(&argv(&["2"])), Ok(2.0));
    assert_eq!(parse_args(&argv(&["1.5"])), Ok(1.5));
    assert_eq!(parse_args(&argv(&["1"])), Ok(1.0));
    assert_eq!(parse_args(&argv(&["default"])), Ok(1.0));
    assert_eq!(parse_args(&argv(&["DEFAULT"])), Ok(1.0));
    assert_eq!(parse_args(&argv(&["8"])), Ok(8.0));
    assert!(parse_args(&argv(&["0.5"])).is_err(), "below 1");
    assert!(parse_args(&argv(&["9"])).is_err(), "above 8");
    assert!(parse_args(&argv(&["nan"])).is_err());
    assert!(parse_args(&argv(&["inf"])).is_err());
    assert!(parse_args(&argv(&["big"])).is_err());
    assert!(parse_args(&argv(&[])).is_err());
    assert!(parse_args(&argv(&["2", "3"])).is_err());
}

#[test]
fn scales_print_without_a_trailing_zero() {
    assert_eq!(format_scale(2.0), "2");
    assert_eq!(format_scale(1.5), "1.5");
    assert_eq!(format_scale(1.0), "1");
    assert_eq!(describe(), "scale = 1");
}

#[test]
fn the_sites_lie_past_the_pattern_inside_draw() {
    // The pattern (font.rs) covers `Draw` from `+0x2b002`; the first rect
    // read is inside it, right after the first text call.
    assert_eq!(
        RECTS_READS[0],
        0x0b + font::CALL_LEN + 4,
        "mov ebx, [esp+0x28] between"
    );
    assert_eq!(
        LINE_AT,
        RECTS_READS[0] + READ_LEN + 6 + 4,
        "the rect read, mov esi, [imm32], mov ecx, [esp+0x3c], then the line start"
    );
    assert_eq!(
        DRAW_HOLES_CALL,
        SPR_SET_CALL + 0x13,
        "SPR_Set, four pushes, DrawHoles"
    );
    assert_eq!(
        RECTS_IN_HUD - 4,
        0x188,
        "m_rghSprites just before m_rgrcRects"
    );
    assert_eq!(DRAW_HOLES_SLOT, 6);
    assert_eq!(SPR_SET_SLOT, 4);
}
