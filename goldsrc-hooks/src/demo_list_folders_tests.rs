use super::*;

#[test]
fn empty_folders_are_hidden_only_while_folders_are_counted() {
    assert!(hides_empty(true, true));
    assert!(!hides_empty(true, false));
    assert!(!hides_empty(false, true), "no counts, no way to know");
    assert!(!hides_empty(false, false));
    assert!(!SHOW_COUNTS.on(), "nothing is counted by default");
    assert!(!HIDE_EMPTY.on() && !COUNT_SUBFOLDERS.on());
}

#[test]
fn a_drive_rooted_listing_path_is_told_from_one_under_dod() {
    assert!(is_drive_path("D:/"));
    assert!(is_drive_path("d:\\demos\\"));
    assert!(is_drive_path("\"C:/Program Files (x86)/x/\""));
    assert!(!is_drive_path(""));
    assert!(!is_drive_path("demos/"));
    assert!(!is_drive_path("../"));
    assert!(!is_drive_path("D:"));
    assert!(is_drive_root("D:/"));
    assert!(is_drive_root("\"d:/\""));
    assert!(!is_drive_root("D:/demos/"));
    let dod = Path::new("C:/games/dod");
    assert_eq!(full_path(dod, "demos/x.dem"), dod.join("demos/x.dem"));
    assert_eq!(
        full_path(dod, "\"temp demos/x.dem\""),
        dod.join("temp demos/x.dem")
    );
    assert_eq!(full_path(dod, "D:/x/y.dem"), PathBuf::from("D:/x/y.dem"));
    assert_eq!(full_path(dod, ""), dod);
}

#[test]
fn drive_paths_resolve_and_climb_back_to_dod() {
    assert_eq!(resolve("D:/"), "D:/");
    assert_eq!(resolve("D:/a/b/../"), "D:/a/");
    assert_eq!(resolve("D:/../"), "");
}

#[test]
fn studios_analysed_demos_are_one_list_whose_up_row_goes_to_dod() {
    assert!(is_studio_place("studio:/"));
    assert!(is_studio_place("\"studio:/\""));
    assert!(!is_studio_place("studio:/../"));
    assert!(!is_drive_path(STUDIO_PLACE));
    assert_eq!(resolve("studio:/../"), "");
    let paths = [
        "D:\\demos\\b.dem".to_string(),
        "D:/demos/A.dem".to_string(),
        "D:/gone/x.dem".to_string(),
        "D:/demos/A.dem".to_string(),
        "relative/x.dem".to_string(),
    ];
    assert_eq!(
        studio_rows(paths.clone(), |path| !path.contains("gone"), true),
        vec!["studio:/../", "\"D:/demos/A.dem\"", "\"D:/demos/b.dem\""]
    );
    assert_eq!(
        studio_rows(paths, |_| true, false),
        vec![
            "\"D:/demos/A.dem\"",
            "\"D:/demos/b.dem\"",
            "\"D:/gone/x.dem\""
        ]
    );
}

#[test]
fn drive_rows_follow_the_up_row_and_drive_demos_are_quoted() {
    let listed = with_drives(
        vec!["../".to_string(), "demos/".to_string()],
        &['c', 'D'],
        true,
    );
    assert_eq!(listed, vec!["../", "C:/", "D:/", "demos/"]);
    assert_eq!(with_drives(Vec::new(), &['D'], true), vec!["D:/"]);
    // The Demos tab's list has no up row: the drives come first.
    assert_eq!(
        with_drives(vec!["demos/".to_string()], &['D'], false),
        vec!["D:/", "demos/"]
    );
    let entries = [("x.dem".to_string(), false), ("sub".to_string(), true)];
    assert_eq!(
        rows("D:/demos/", entries.clone(), |_, _| true, true),
        vec!["D:/demos/../", "D:/demos/sub/", "\"D:/demos/x.dem\""]
    );
    assert_eq!(
        rows("D:/demos/", entries, |_, _| true, false),
        vec!["D:/demos/sub/", "\"D:/demos/x.dem\""]
    );
}

#[test]
fn back_and_forward_walk_the_folders_visited() {
    let mut nav = Nav::new();
    assert!(nav.go(""));
    assert!(!nav.go(""), "the same folder again is not a step");
    assert!(nav.go("demos/"));
    assert!(!nav.can_go_forward());
    assert!(nav.go("D:/"));
    assert!(nav.can_go_back());
    assert!(nav.back());
    assert!(nav.can_go_forward());
    assert_eq!(nav.current(), "demos/");
    assert!(nav.back());
    assert_eq!(nav.current(), "");
    assert!(!nav.back());
    assert!(nav.forward());
    assert_eq!(nav.current(), "demos/");
    // A new folder from here forgets what was Forward.
    assert!(nav.go("E:/"));
    assert!(!nav.forward());
    assert_eq!(nav.recent(5), vec!["demos/", ""]);
    assert_eq!(nav.recent(1), vec!["demos/"]);
}

#[test]
fn the_game_window_shows_short_names() {
    assert_eq!(short_row_name("H:/DoD Demos/Philly LAN/"), " Philly LAN/");
    assert_eq!(short_row_name("H:/DoD Demos/../"), " ../");
    assert_eq!(short_row_name("D:/"), " D:/");
    assert_eq!(short_row_name("../"), " ../");
    assert_eq!(short_row_name("temp demos/"), " temp demos/");
    assert_eq!(short_row_name("temp demos/x.dem"), "x.dem");
    assert_eq!(short_row_name("\"H:/DoD Demos/a b.dem\""), "a b.dem");
    assert_eq!(short_row_name("x.dem"), "x.dem");
}

#[test]
fn this_pc_lists_every_drive() {
    assert_eq!(drives_rows(&['C', 'd'], false), vec!["C:/", "D:/"]);
    assert_eq!(
        drives_rows(&['C'], true),
        vec!["drives:/../".to_string(), "C:/".to_string()]
    );
    // Its up row goes to dod/.
    assert_eq!(resolve("drives:/../"), "");
    assert!(is_place(DRIVES_PLACE) && is_place(STUDIO_PLACE));
    assert!(!is_drive_path(DRIVES_PLACE) && !is_place("D:/"));
}

#[test]
fn climbing_to_a_drive_root_lands_on_the_drive() {
    let dod = Path::new("C:/Steam/common/HL/dod");
    assert_eq!(drive_root_of(dod, "../../../../"), Some("C:/".to_string()));
    // Past the root, as a saved state from before this could hold.
    assert_eq!(
        drive_root_of(dod, "../../../../../../../"),
        Some("C:/".to_string())
    );
    assert_eq!(drive_root_of(dod, "../../../"), None);
    assert_eq!(drive_root_of(dod, "temp demos/"), None);
    assert_eq!(drive_root_of(dod, ""), None);
    assert_eq!(drive_root_of(dod, "D:/"), None);
    assert_eq!(drive_root_of(dod, STUDIO_PLACE), None);
    assert_eq!(drive_root_of(dod, DRIVES_PLACE), None);
    // Up from a drive's root is This PC, and stops there.
    assert_eq!(up_from("C:/"), DRIVES_PLACE);
    assert_eq!(up_from(DRIVES_PLACE), DRIVES_PLACE);
    assert_eq!(up_from("D:/demos/"), "D:/");
    assert_eq!(up_from("temp demos/"), "");
    assert_eq!(up_from(STUDIO_PLACE), "");
}

#[test]
fn the_state_file_round_trips() {
    let text = state_text("D:/demos/", &["".to_string(), "temp demos/".to_string()]);
    assert_eq!(text, "folder=D:/demos/\nrecent=\nrecent=temp demos/\n");
    assert_eq!(
        parse_state(&text),
        ("D:/demos/".to_string(), vec!["temp demos/".to_string()])
    );
    assert_eq!(parse_state(""), (String::new(), Vec::new()));
}

#[test]
fn the_go_to_list_names_each_place_once() {
    let studio = [
        "C:/Games/Half-Life/dod/".to_string(),
        "D:/demos/".to_string(),
    ];
    let recent = [
        "D:/demos/".to_string(),
        "temp demos/".to_string(),
        "../../".to_string(),
        "../".to_string(),
        "../../Half-Life/dod/".to_string(),
    ];
    let dod = Path::new("C:/Steam/common/HL/dod");
    let places = build_places(dod, "dod", &['C', 'D'], &studio, &recent);
    let labels: Vec<&str> = places.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "Game: dod/",
            "Studio's analysed demos",
            "This PC (every drive)",
            "Drive: C:/",
            "Drive: D:/",
            "Studio: C:/.../Half-Life/dod/",
            "Studio: D:/demos/",
            "Recent: C:/.../dod/temp demos/",
            "Recent: C:/Steam/common/",
            "Recent: C:/.../common/HL/",
            "Recent: C:/.../Half-Life/dod/",
        ]
    );
    assert_eq!(places[1].folder, STUDIO_PLACE);
    assert_eq!(places[2].folder, DRIVES_PLACE);
    assert_eq!(places[5].folder, "C:/Games/Half-Life/dod/");
    // Two different folders with the same drive and last two names stay
    // apart; on different drives the drive tells them apart.
    let twins = ["C:/a/x/dod/".to_string(), "C:/b/x/dod/".to_string()];
    let places = build_places(dod, "dod", &[], &twins, &[]);
    assert_eq!(places[3].label, "Studio: C:/.../x/dod/");
    assert_eq!(places[4].label, "Studio: C:/.../x/dod/ (2)");
    let apart = ["C:/a/x/dod/".to_string(), "D:/b/x/dod/".to_string()];
    let places = build_places(dod, "dod", &[], &apart, &[]);
    assert_eq!(places[4].label, "Studio: D:/.../x/dod/");
}

#[test]
fn studios_folders_come_out_of_its_settings_file() {
    let json = r#"{"pinned_folders": ["C:\\Games\\Half-Life\\dod", "D:\\demos\\"],
            "demo_folder_history": ["D:\\demos", "\\\\server\\share", "C:\\x"], "other": 1}"#;
    assert_eq!(
        studio_folders_from(json),
        vec!["C:/Games/Half-Life/dod/", "D:/demos/", "C:/x/"]
    );
    assert!(studio_folders_from("not json").is_empty());
    assert!(studio_folders_from("{}").is_empty());
}

#[test]
fn folders_resolve_to_clean_paths_from_dod() {
    assert_eq!(resolve("test/"), "test/");
    assert_eq!(resolve("test/sub/"), "test/sub/");
    assert_eq!(resolve("test/../"), "");
    assert_eq!(resolve("../"), "../");
    assert_eq!(resolve("../../"), "../../");
    assert_eq!(resolve("../../dod/test/"), "../../dod/test/");
    assert_eq!(resolve("test/sub/../../../"), "../");
    assert_eq!(resolve("temp demos/"), "temp demos/");
}

#[test]
fn rows_are_parent_folders_then_demos_with_spaces_quoted() {
    let entries = [
        (".".to_string(), true),
        ("..".to_string(), true),
        ("temp demos".to_string(), true),
        ("a.dem".to_string(), false),
        ("B.DEM".to_string(), false),
        ("notes.txt".to_string(), false),
        ("my clip.dem".to_string(), false),
    ];
    assert_eq!(
        rows("test/", entries, |_, _| true, true),
        vec![
            "test/../",
            "test/temp demos/",
            "test/a.dem",
            "test/B.DEM",
            "\"test/my clip.dem\"",
        ]
    );
}

#[test]
fn a_name_in_two_search_paths_is_listed_once() {
    let entries = [
        ("a.dem".to_string(), false),
        ("sub".to_string(), true),
        ("A.dem".to_string(), false),
        ("sub".to_string(), true),
    ];
    assert_eq!(
        rows("", entries, |_, _| true, true),
        vec!["../", "sub/", "a.dem"]
    );
}

#[test]
fn only_what_is_on_disk_under_dod_is_listed() {
    let entries = [
        ("valve".to_string(), true),
        ("WindowsCrashDumps".to_string(), true),
        ("temp demos".to_string(), true),
        ("root.dem".to_string(), false),
        ("mine.dem".to_string(), false),
    ];
    let on_disk = |path: &str, _: bool| matches!(path, "temp demos" | "mine.dem");
    assert_eq!(
        rows("", entries, on_disk, true),
        vec!["../", "temp demos/", "mine.dem"]
    );
}

#[test]
fn this_folder_and_the_one_above_are_never_asked_about() {
    let entries = [
        (".".to_string(), true),
        ("..".to_string(), true),
        ("sub".to_string(), true),
    ];
    let asked = std::cell::RefCell::new(Vec::new());
    let listed = rows(
        "test/",
        entries,
        |path, _| {
            asked.borrow_mut().push(path.to_string());
            true
        },
        true,
    );
    assert_eq!(listed, vec!["test/../", "test/sub/"]);
    assert_eq!(
        *asked.borrow(),
        vec!["test/sub"],
        "only the real subfolder is checked or counted"
    );
}

#[test]
fn the_builds_are_told_apart() {
    assert_ne!(
        GAMEUI_BUILDS[0].time_date_stamp,
        GAMEUI_BUILDS[1].time_date_stamp
    );
    assert_ne!(
        FILESYSTEM_BUILDS[0].time_date_stamp,
        FILESYSTEM_BUILDS[1].time_date_stamp
    );
}
