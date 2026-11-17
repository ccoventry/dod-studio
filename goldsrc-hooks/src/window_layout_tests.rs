use super::*;

/// #609: the Console tab, drawn for 536x240, came out 60 px taller. Its
/// input line must still follow the bottom when the window grows, and
/// the history above it stretch.
#[test]
fn a_taller_tab_still_keeps_the_bottom_row_at_the_bottom() {
    let controls = [(8, 6, 520, 196), (8, 208, 436, 24), (452, 208, 76, 24)];
    let (design, now) = content_frame(&controls, (536, 300), (536, 400));
    assert_eq!(design, (536, 240));
    assert_eq!(now, (536, 340));
    assert_eq!(fit_rect(controls[1], design, now), (8, 308, 436, 24));
    assert_eq!(fit_rect(controls[0], design, now), (8, 6, 520, 296));
    // Without it, the line stays put and the history runs under it.
    assert_eq!(
        fit_rect(controls[1], (536, 300), (536, 400)),
        (8, 208, 436, 24)
    );
}

#[test]
fn a_layout_that_fills_its_tab_is_unchanged() {
    let controls = [(8, 6, 520, 226)];
    assert_eq!(
        content_frame(&controls, (536, 240), (700, 500)),
        ((536, 240), (700, 500))
    );
}

/// The Studio tab's two rows end far above its bottom on purpose: its
/// height is left as drawn. (Its width ends 40 px short, which is margin.)
#[test]
fn a_short_layout_is_left_as_drawn() {
    let controls = [(8, 8, 160, 24), (8, 44, 480, 24)];
    let ((_, design_h), (_, now_h)) = content_frame(&controls, (536, 240), (536, 400));
    assert_eq!((design_h, now_h), (240, 400));
}

#[test]
fn a_tab_list_takes_the_height_whatever_its_share() {
    // Under half the design height: the half rule leaves it alone...
    assert_eq!(fit_rect((8, 106, 520, 96), (536, 240), (900, 600)).3, 96);
    // ...the list rule doesn't, and keeps its top and the width rule.
    let r = fit_rect_tall((8, 106, 520, 96), (536, 240), (900, 600));
    assert_eq!(r, (8, 106, 520 + 364, 96 + 360));
    // Shrinking never goes below a pixel.
    assert_eq!(fit_rect_tall((8, 106, 520, 96), (536, 240), (536, 10)).3, 1);
}

#[test]
fn the_layout_file_round_trips() {
    let mut saved = BTreeMap::new();
    saved.insert(
        "GameUI/DemoPlayerDialog".to_string(),
        Saved {
            at: Rect {
                x: 10,
                y: -4,
                w: 700,
                h: 120,
            },
            design: Some((464, 96)),
        },
    );
    saved.insert(
        "GameUI/GameConsole".to_string(),
        Saved {
            at: Rect {
                x: 0,
                y: 0,
                w: 640,
                h: 480,
            },
            design: None,
        },
    );
    assert_eq!(parse(&render(&saved)), saved);
    assert!(render(&saved).contains("GameUI/DemoPlayerDialog\t10 -4 700 120 464 96\n"));
}

#[test]
fn a_bad_line_is_skipped_not_fatal() {
    let parsed = parse(
        "junk\nGameUI/A\t1 2 3\nGameUI/B\t1 2 0 5\nGameUI/C\t1 2 3 4\n\
             GameUI/D\t1 2 3 4 5\nGameUI/E\t1 2 3 4 0 9\nGameUI/F\t1 2 3 4 5 6 7\n",
    );
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed["GameUI/C"],
        Saved {
            at: Rect {
                x: 1,
                y: 2,
                w: 3,
                h: 4
            },
            design: None,
        }
    );
}

/// #1020: the demo bar saved on POST (464x96, its own size) and put back
/// on PRE, where the user's layout is 464x128, keeps PRE's 128.
#[test]
fn a_window_saved_from_another_layout_keeps_its_own_size() {
    let saved = |w, h, design| Saved {
        at: Rect {
            x: 66,
            y: 285,
            w,
            h,
        },
        design,
    };
    assert_eq!(
        restored_size(saved(464, 96, Some((464, 96))), Some((464, 128))),
        Some((464, 128))
    );
    // Resized on POST: the same change carries over.
    assert_eq!(
        restored_size(saved(600, 110, Some((464, 96))), Some((464, 128))),
        Some((600, 142))
    );
    // The same layout gets exactly what was saved.
    assert_eq!(
        restored_size(saved(600, 110, Some((464, 96))), Some((464, 96))),
        Some((600, 110))
    );
    // Shrunk below a pixel never happens.
    assert_eq!(
        restored_size(saved(10, 10, Some((464, 128))), Some((100, 50))),
        Some((1, 1))
    );
}

/// A line from before #1020 doesn't say which layout it was saved from:
/// a window whose design is known keeps its own size (the squashed PRE
/// bar again otherwise); one that lays itself out, like the console,
/// still gets the saved size.
#[test]
fn an_old_line_resizes_only_windows_without_a_design() {
    let old = Saved {
        at: Rect {
            x: 66,
            y: 285,
            w: 464,
            h: 96,
        },
        design: None,
    };
    assert_eq!(restored_size(old, Some((464, 128))), None);
    assert_eq!(restored_size(old, None), Some((464, 96)));
    let new = Saved {
        design: Some((464, 96)),
        ..old
    };
    assert_eq!(restored_size(new, None), Some((464, 96)));
}

/// What #1020 looked like: the bar's rows in a 128-tall layout squeezed
/// to 96. The button row (64) lands on the slider's row (32) and the time
/// label with it; the slider, 64 px from the bottom, stays.
#[test]
fn squeezing_the_pre_bar_puts_the_buttons_on_the_slider_row() {
    let row = |y, h| fit(Rect { x: 12, y, w: 24, h }, (464, 128), (464, 96)).y;
    assert_eq!(row(32, 32), 32);
    assert_eq!(row(64, 24), 32);
    assert_eq!(row(96, 24), 64);
}

#[test]
fn a_window_saved_off_screen_comes_back_on() {
    let r = |x, y| Rect {
        x,
        y,
        w: 700,
        h: 120,
    };
    // Inside: untouched.
    assert_eq!(on_screen(r(100, 100), 1920, 1080), r(100, 100));
    // Past the right edge of a smaller screen: pulled back, keeping 48 px.
    assert_eq!(on_screen(r(2400, 100), 1280, 720), r(1280 - 48, 100));
    // Off the left: at least 48 px stay visible.
    assert_eq!(on_screen(r(-5000, 100), 1920, 1080), r(48 - 700, 100));
    // Title bar above the top, or below the bottom: pulled back.
    assert_eq!(on_screen(r(100, -30), 1920, 1080), r(100, 0));
    assert_eq!(on_screen(r(100, 5000), 1920, 1080), r(100, 1080 - 48));
}

#[test]
fn a_control_spanning_the_window_grows_with_it() {
    // The Load Demo list: 5..665 of a 692-wide, 380-tall window.
    let list = Rect {
        x: 5,
        y: 40,
        w: 660,
        h: 300,
    };
    assert_eq!(
        fit(list, (692, 380), (900, 520)),
        Rect {
            x: 5,
            y: 40,
            w: 868,
            h: 440
        }
    );
}

#[test]
fn a_control_near_the_far_edge_keeps_its_distance_from_it() {
    // The Load button: bottom left. It follows the bottom, not the right.
    let load = Rect {
        x: 15,
        y: 350,
        w: 60,
        h: 24,
    };
    assert_eq!(
        fit(load, (692, 380), (900, 520)),
        Rect {
            x: 15,
            y: 490,
            w: 60,
            h: 24
        }
    );
    // A button in the top right follows the right edge only.
    let corner = Rect {
        x: 600,
        y: 30,
        w: 60,
        h: 24,
    };
    assert_eq!(
        fit(corner, (692, 380), (900, 520)),
        Rect {
            x: 808,
            y: 30,
            w: 60,
            h: 24
        }
    );
}

#[test]
fn a_row_of_buttons_stays_together() {
    // The VCR bar (464 wide): the transport buttons run 12..276, the time
    // label 284..426 and the stop button 426..450. Only the last two are
    // near the right edge.
    let at = |x, w| fit(Rect { x, y: 64, w, h: 24 }, (464, 128), (600, 128)).x;
    for x in [12, 56, 84, 112, 140, 168, 196, 224, 252] {
        assert_eq!(at(x, 24), x, "button at {x} moved");
    }
    assert_eq!(at(284, 142), 284 + 136);
    assert_eq!(at(426, 24), 426 + 136);
}

#[test]
fn the_design_size_changes_nothing_and_shrinking_never_inverts() {
    let list = Rect {
        x: 5,
        y: 40,
        w: 660,
        h: 300,
    };
    assert_eq!(fit(list, (692, 380), (692, 380)), list);
    let tiny = fit(list, (692, 380), (10, 10));
    assert!(tiny.w >= 1 && tiny.h >= 1);
}

#[test]
fn only_the_frame_s_own_pieces_are_left_to_it() {
    assert!(frame_part(""));
    assert!(frame_part("frame_close"));
    assert!(frame_part("frame_brGrip"));
    assert!(!frame_part("Load"));
    assert!(!frame_part("FileList"));
}

#[test]
fn no_name_is_the_start_of_another() {
    assert!(!RESIZABLE_NAME.starts_with(REMEMBER_NAME));
    assert!(!REMEMBER_NAME.starts_with(RESIZABLE_NAME));
}

#[test]
fn the_builds_are_told_apart() {
    for pair in [
        (
            GAMEUI_BUILDS[0].time_date_stamp,
            GAMEUI_BUILDS[1].time_date_stamp,
        ),
        (
            VGUI2_BUILDS[0].time_date_stamp,
            VGUI2_BUILDS[1].time_date_stamp,
        ),
        (HW_BUILDS[0].time_date_stamp, HW_BUILDS[1].time_date_stamp),
    ] {
        assert_ne!(pair.0, pair.1);
    }
}
