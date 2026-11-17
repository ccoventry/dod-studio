use super::*;

#[test]
fn a_column_minimum_is_never_above_its_width() {
    // Resizable: Valve's 20..10000, the minimum lowered to a narrower width.
    assert_eq!(column_width_range(100, 0), (20, 10_000));
    assert_eq!(column_width_range(12, 0), (12, 10_000));
    // Resizes with the window (2) without the fixed flag: still a range.
    assert_eq!(column_width_range(40, 2), (20, 10_000));
    // Fixed size (1, and the icon column's 1 | 4): the width both ways.
    assert_eq!(column_width_range(22, 1 | 4), (22, 22));
    // Fixed and resize-with-window together is not fixed, as in GameUI.
    assert_eq!(column_width_range(22, 3), (20, 10_000));
    for width in [1, 20, 40, 42, 210, 20_000] {
        for flags in [0, 1, 2, 3, 4, 5] {
            let (min, max) = column_width_range(width, flags);
            assert!(min <= width && width <= max, "{width} {flags}");
        }
    }
}

#[test]
fn each_drag_handle_names_its_column_position() {
    // The Demos tab: Demo File was record 0 at position 0, the icons'
    // record 1 went in at position 0, then Map..Date were appended.
    let order = [1u8, 0, 2, 3, 4, 5];
    let got: Vec<_> = dragger_positions(&order, 6).collect();
    assert_eq!(got, [(1, 0), (0, 1), (2, 2), (3, 3), (4, 4), (5, 5)]);
    // A record index past the allocated records is skipped, not read.
    let got: Vec<_> = dragger_positions(&[0, 7, 1], 2).collect();
    assert_eq!(got, [(0, 0), (1, 2)]);
    assert_eq!(dragger_positions(&[], 0).count(), 0);
}

#[test]
fn the_layouts_follow_the_hd_files_into_dod_addon_only_with_addons() {
    // -addons: dod_addon unless only dod has the folder.
    assert!(ui_in_addon(true, true, false));
    assert!(ui_in_addon(true, true, true));
    assert!(ui_in_addon(true, false, false));
    assert!(!ui_in_addon(true, false, true));
    // Without it the engine never reads dod_addon.
    assert!(!ui_in_addon(false, true, false));
    assert!(!ui_in_addon(false, true, true));
}

#[test]
fn every_enable_button_is_in_its_tab_and_runs_its_setting() {
    for (button, page, source) in ENABLE_BUTTONS {
        let res = PAGES[*page].res.2;
        assert!(res.contains(&format!("\"{button}\"")), "{button}");
        let setting = if *source == VCR_BAR {
            VIEWDEMO_NAME
        } else {
            CONSOLE_NAME
        };
        assert!(res.contains(&format!("engine {setting} 1")), "{button}");
    }
}

#[test]
fn a_check_box_is_bound_by_its_name() {
    assert_eq!(bound_cvar("cvar_hud_draw"), Some("hud_draw"));
    assert_eq!(bound_cvar("cvar_"), None);
    assert_eq!(bound_cvar("cvar_x;quit"), None);
    assert_eq!(bound_cvar("Hint"), None);
}

#[test]
fn a_click_sets_the_cvar_and_the_cvar_moves_the_box() {
    // First sight: the box follows the cvar.
    assert_eq!(settle(false, None, true), (None, true));
    // The user clicked it: the cvar follows the box.
    assert_eq!(settle(true, Some(false), false), (Some(true), false));
    // The cvar changed elsewhere: the box follows.
    assert_eq!(settle(false, Some(false), true), (None, true));
    // Nothing changed.
    assert_eq!(settle(true, Some(true), true), (None, false));
}

#[test]
fn every_settings_box_names_a_known_cvar() {
    let res = PAGES[SETTINGS_PAGE].res.2;
    let boxes: Vec<&str> = res.split('"').filter_map(bound_cvar).collect();
    assert!(boxes.len() >= 10, "{boxes:?}");
    assert!(boxes.contains(&"ds_ui_viewdemo_in_panel"));
    assert!(boxes.contains(&"ds_ui_console_in_panel"));
}

#[test]
fn a_typed_time_reads_as_the_vcr_bar_writes_it() {
    assert_eq!(parse_time("75"), Ok(75.0));
    assert_eq!(parse_time("20:33"), Ok(20.0 * 60.0 + 33.0));
    assert_eq!(parse_time(" 20:33:50 "), Ok(20.0 * 60.0 + 33.5));
    assert_eq!(parse_time("1:2.5"), Ok(62.5));
    assert!(parse_time("").is_err());
    assert!(parse_time("1:2:3:4").is_err());
    assert!(parse_time("-5").is_err());
    assert!(parse_time("abc").is_err());
}

#[test]
fn help_text_is_read_from_a_res_file() {
    let res = "\"x.res\"
{
	\"A\"
	{
		\"labelText\"	\"{not a brace}\"
		\"tooltiptext\"	\"help A\"
	}
// \"B\" { \"tooltiptext\" \"commented\" }
	\"B\"
	{
		\"wide\"	\"5\"
	}
}
";
    assert_eq!(tooltips(res), vec![("A".to_string(), "help A".to_string())]);
}

#[test]
fn every_default_control_has_help() {
    for page in &PAGES {
        let tips = tooltips(page.res.2);
        assert!(!tips.is_empty(), "{} has no help text", page.res.1);
    }
}

#[test]
fn the_commands_tab_lists_every_console_name() {
    // Every console_name!("...") in the crate, but the doc example and
    // the kill feed's second name.
    let mut missing = Vec::new();
    let commands = COMMANDS_TEXT;
    assert_eq!(PAGES[COMMANDS_PAGE].res.1, "Commands.res");
    assert!(PAGES[COMMANDS_PAGE].res.2.contains(COMMAND_LIST));
    for entry in std::fs::read_dir("src")
        .unwrap()
        .chain(std::fs::read_dir("src/anim_fix").unwrap())
    {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for part in text.split("console_name!(\"").skip(1) {
            let name = part.split('"').next().unwrap();
            if name == "status"
                || name == "killfeed"
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                continue;
            }
            let full = format!("{}{name}", console_name!(""));
            if !commands.contains(&full) {
                missing.push(full);
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "missing from Commands.res: {missing:?}");
}

#[test]
fn the_command_takes_1_0_or_reset() {
    assert_eq!(request(None), Ok(Request::Toggle));
    assert_eq!(request(Some("1")), Ok(Request::Open));
    assert_eq!(request(Some("0")), Ok(Request::Close));
    assert_eq!(request(Some("reset")), Ok(Request::Reset));
    assert_eq!(request(Some("highlights")), Ok(Request::Tab(STREAKS_PAGE)));
    assert_eq!(request(Some("demos")), Ok(Request::Tab(DEMOS_PAGE)));
    assert!(request(Some("2")).is_err());
}

#[test]
fn button_commands_are_sorted_into_what_they_do() {
    assert_eq!(action("play"), Action::Vcr("play"));
    assert_eq!(action("Faster"), Action::Vcr("faster"));
    assert_eq!(
        action("engine ds_debug_status"),
        Action::Engine("ds_debug_status")
    );
    assert_eq!(action("Close"), Action::Own);
    assert_eq!(action("goto"), Action::Goto);
    assert_eq!(action("demo_back"), Action::DemoBack);
    assert_eq!(action("Demo_Forward"), Action::DemoForward);
    assert_eq!(action("demo_up"), Action::DemoUp);
    assert_eq!(action("reset_settings"), Action::ResetSettings);
    assert_eq!(action("settings_up"), Action::ScrollSettings(-1));
    assert_eq!(action("settings_down"), Action::ScrollSettings(1));
}

#[test]
fn the_settings_tab_scrolls_between_its_top_and_its_last_row() {
    // 22-pixel lines, a 244-pixel view of 800 pixels of rows.
    assert_eq!(settings_scroll(0, 3, 0, 22, 244, 800), 66);
    assert_eq!(settings_scroll(0, -3, 0, 22, 244, 800), 0);
    assert_eq!(settings_scroll(0, 0, 1, 22, 244, 800), 222);
    assert_eq!(settings_scroll(500, 0, 1, 22, 244, 800), 556);
    assert_eq!(settings_scroll(556, 3, 0, 22, 244, 800), 556);
    // A window tall enough for every row doesn't scroll.
    assert_eq!(settings_scroll(100, 3, 1, 22, 900, 800), 0);
}

/// Every Playback tab button that moves the demo is refused during a
/// review (#826); the decals and "use this tab" buttons are not.
#[test]
fn a_review_refuses_the_playback_buttons_that_move_the_demo() {
    let res = include_str!("../ui/Playback.res");
    let mut commands = res
        .lines()
        .filter_map(|l| l.trim().strip_prefix("\"Command\""))
        .map(|v| v.trim().trim_matches('"').to_string())
        .collect::<Vec<_>>();
    commands.push("goto".to_string());
    assert!(commands.len() > 10, "{commands:?}");
    for command in &commands {
        let keeps = command.contains("clear_decals") || command.contains("viewdemo_in_panel");
        assert_eq!(action_moves_the_demo(&action(command)), !keeps, "{command}");
    }
}

#[test]
fn the_loading_strip_names_each_load_still_running() {
    assert_eq!(
        load_strip_text(Some(45), Some(60), None).as_deref(),
        Some("Loading: Demo 45%   Highlights 60%")
    );
    assert_eq!(
        load_strip_text(Some(0), None, None).as_deref(),
        Some("Loading: Demo 0%")
    );
    assert_eq!(
        load_strip_text(None, Some(100), None).as_deref(),
        Some("Loading: Highlights 100%")
    );
    assert_eq!(load_strip_text(None, None, None), None);
}

#[test]
fn the_loading_strip_keeps_a_damaged_demos_line() {
    let damaged = || Some("This demo is damaged at 8:27.75.".to_string());
    assert_eq!(
        load_strip_text(None, None, damaged()).as_deref(),
        Some("This demo is damaged at 8:27.75.")
    );
    assert_eq!(
        load_strip_text(None, Some(40), damaged()).as_deref(),
        Some("This demo is damaged at 8:27.75.   Loading: Highlights 40%")
    );
}

#[test]
fn the_loading_strip_sits_above_the_controls_it_pushes_down() {
    let (x, y, w, h) = load_strip_bounds(540);
    assert_eq!((x, w), (8, 524));
    assert!(y >= 0 && y + h <= LOAD_STRIP_TALL);
    assert!(load_strip_bounds(4).2 >= 1);
}

#[test]
fn the_tab_strip_fills_the_client_area_inside_a_margin() {
    assert_eq!(
        sheet_bounds((2, 28, 516, 210)),
        (6, 32, 508, 202 - HELP_TALL)
    );
    // Never inverted, however small the window gets.
    let tiny = sheet_bounds((0, 0, 3, 3));
    assert!(tiny.2 >= 1 && tiny.3 >= 1);
}

#[test]
fn every_default_layout_is_built_in_and_names_its_own_file() {
    assert!(WINDOW_RES.2.contains("\"DodStudio\""));
    assert!(WINDOW_RES.2.contains("dodstudio_ui/DodStudio.res"));
    for page in &PAGES {
        let path = page.res.0.to_str().unwrap();
        assert_eq!(path, format!("{RES_DIR}/{}", page.res.1));
        assert!(page.res.2.starts_with(&format!("\"{path}\"")), "{path}");
    }
    // The Playback tab carries every VCR button.
    for command in [
        "start", "slower", "stepb", "pause", "play", "stepf", "faster", "end", "stop",
    ] {
        assert!(
            PAGES[0]
                .res
                .2
                .contains(&format!("\"Command\"\t\t\"{command}\"")),
            "{command}"
        );
    }
}

#[test]
fn the_window_is_never_narrower_than_its_tabs() {
    // Tabs ending at 200 inside a sheet, in a frame 8 px wider than its
    // client area: the tabs, both margins, the border and a little slack.
    assert_eq!(minimum_size(200, 528, 520), (200 + 8 + 8 + 8, MIN_TALL));
}

#[test]
fn every_loan_has_its_slot_on_its_tab() {
    for loan in LOANS.iter().filter(|l| !l.slot.is_empty()) {
        let res = PAGES[loan.page].res.2;
        assert!(res.contains(&format!("\"{}\"", loan.slot)), "{}", loan.slot);
    }
    assert_eq!(PAGES[PLAYBACK_PAGE].name, c"Playback");
    assert_eq!(PAGES[DEMOS_PAGE].name, c"Demos");
    assert_eq!(PAGES[STREAKS_PAGE].name, c"Highlights");
    assert_eq!(PAGES[CONSOLE_PAGE].name, c"Console");
    assert_eq!(PAGES[SETTINGS_PAGE].name, c"Settings");
    assert_eq!(PAGES[COMMANDS_PAGE].name, c"Commands");
}

#[test]
fn no_name_is_the_start_of_another() {
    let names = [NAME, VIEWDEMO_NAME, CONSOLE_NAME];
    for a in names {
        for b in names {
            assert!(a == b || !a.starts_with(b), "{b} starts {a}");
        }
    }
}

#[test]
fn the_builds_match_the_window_layout_table() {
    for (ours, theirs) in BUILDS
        .iter()
        .zip(crate::window_layout::GAMEUI_BUILDS.iter())
    {
        assert_eq!(ours.name, theirs.name);
        assert_eq!(ours.time_date_stamp, theirs.time_date_stamp);
        assert_eq!(ours.size_of_image, theirs.size_of_image);
    }
}
