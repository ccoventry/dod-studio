//! The Demos tab's filters, columns and rows, worked out without the game:
//! what `hook::demos_tab` shows in our Load Demo window's list.

use std::ffi::CStr;

/// The Demos tab's search box.
pub(super) const DEMO_FILTER: &str = "DemoFilter";

/// The width the Demos tab gives the demo name column. The list is 520 wide
/// and the columns already add up to more, so a long name is dragged wider
/// instead (#801).
pub(super) const NAME_COLUMN_WIDE: i32 = 170;

/// The Demos tab's columns past the demo's own name: key, heading, width.
/// Type is POV or HLTV, read from the demo's header like the Type filter.
/// Player is who recorded a POV demo, from the players file Studio saves
/// with an analysis (blank until one is), and a dash for HLTV.
pub(super) const DEMO_COLUMNS: [(&CStr, &CStr, i32); 4] = [
    (c"map", c"Map", 100),
    (c"type", c"Type", 42),
    (c"player", c"Player", 100),
    (c"date", c"Date", 110),
];
/// Set on a row once its Map, Type and Date are filled in, so a list the Load Demo
/// window refilled on its own (opening a folder) is noticed.
pub(super) const STAMP_KEY: &CStr = c"dodstudio";

/// Whether a demo row matches what was typed in the search box: every word,
/// anywhere in the name, case ignored. An empty search matches everything.
pub(super) fn matches_filter(row: &str, filter: &str) -> bool {
    let row = row.to_ascii_lowercase();
    filter
        .split_whitespace()
        .all(|word| row.contains(&word.to_ascii_lowercase()))
}

/// The Demos tab's other filters: map, HLTV / POV, and age.
pub(super) const MAP_FILTER: &str = "MapFilter";
/// The Type dropdown (All / POV / HLTV), as Studio's Demo Analyzer has it.
pub(super) const DEMO_TYPE: &str = "DemoType";
pub(super) const DEMO_TYPES: [&CStr; 3] = [c"All", c"POV", c"HLTV"];
/// The Demos tab's picked players, and whether a demo needs all of them
/// or any.
pub(super) const PLAYER_CHOSEN: &str = "PlayerChosen";
pub(super) const PLAYER_MATCH: &str = "PlayerMatch";
/// Any first: the usual case is one player under several names.
pub(super) const PLAYER_MATCHES: [&CStr; 2] = [c"Any", c"All"];
/// The Demos tab's Go to dropdown (#779): its first row is this heading;
/// picking any other row goes to that folder, and the heading shows again.
pub(super) const DEMO_PLACE: &str = "DemoPlace";
pub(super) const PLACE_HEADING: &CStr = c"Go to...";
/// What a drive row (`D:/`) says in the Type column, where a folder row says
/// Folder; it has no demo count.
pub(super) const DRIVE_TYPE: &CStr = c"Drive";

/// The players picked from the Player list, in the order picked.
pub(super) static PICKED_PLAYERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The Demos tab's Back, Forward and Up buttons (`ui/Demos.res`).
pub(super) const DEMO_NAV_BUTTONS: [&str; 3] = ["DemoBack", "DemoForward", "DemoUp"];

pub(super) const DAYS_FILTER: &str = "DaysFilter";
/// The Player filter: a name or SteamID, "they recorded it", and the note on
/// how many demos it can see, shown in place of the hint while it is in use.
pub(super) const PLAYER_FILTER: &str = "PlayerFilter";
pub(super) const PLAYER_RECORDED: &str = "PlayerRecorded";
/// The Clear button beside the Player box: greyed out while no player is
/// picked, since it would do nothing (#800).
pub(super) const PLAYER_CLEAR: &str = "PlayerClear";
pub(super) const PLAYER_NOTE: &str = "PlayerNote";
pub(super) const DEMOS_HINT: &str = "Hint";

/// What the start of a demo says about it: the map (header offset 16, 260
/// bytes), and whether an HLTV proxy recorded it. The proxy's connect message
/// ends "Spawn count N (HLTV)", about 1,060 bytes in: true of all 113 HLTV
/// demos among 984 surveyed, and of none of the POV ones. (`HLTV Proxy`, what
/// Studio's `is_hltv_demo` looks for in the first 512 bytes, is in none.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DemoInfo {
    pub(super) map: String,
    pub(super) hltv: bool,
    /// Seconds since the Unix epoch the file was last written.
    pub(super) modified: u64,
}

pub(super) fn demo_info(header: &[u8], modified: u64) -> Option<DemoInfo> {
    if header.len() < 16 + 260 || !header.starts_with(b"HLDEMO") {
        return None;
    }
    let map = &header[16..16 + 260];
    let map = &map[..map.iter().position(|&b| b == 0).unwrap_or(map.len())];
    let map = String::from_utf8_lossy(map);
    let map = map
        .trim_start_matches("maps/")
        .trim_end_matches(".bsp")
        .to_string();
    let hltv = header.windows(6).any(|w| w == b"(HLTV)");
    Some(DemoInfo {
        map,
        hltv,
        modified,
    })
}

/// `secs` (Unix time, UTC) as a calendar date and time:
/// (year, month, day, hour, minute).
pub(super) fn civil(secs: u64) -> (u64, u64, u64, u64, u64) {
    // Howard Hinnant's days-to-civil.
    let days = secs / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day, secs % 86_400 / 3600, secs % 3600 / 60)
}

/// The Date column's text: sorts as text in date order.
pub(super) fn date_text((year, month, day, hour, minute): (u64, u64, u64, u64, u64)) -> String {
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Whether the Type dropdown's text lets HLTV demos (`hltv`) or POV demos
/// (`!hltv`) through: "All", or anything else, lets both.
pub(super) fn shows_type(chosen: &str, hltv: bool) -> bool {
    match chosen.trim() {
        t if t.eq_ignore_ascii_case("HLTV") => hltv,
        t if t.eq_ignore_ascii_case("POV") => !hltv,
        _ => true,
    }
}

/// Everything the Demos tab filters by at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DemoFilters {
    pub(super) search: String,
    pub(super) map: String,
    pub(super) hltv: bool,
    pub(super) pov: bool,
    /// Newer than this many days, or none.
    pub(super) days: Option<u64>,
}

/// What a folder row shows in the Demos tab's Type column, where a demo says
/// POV or HLTV.
pub(super) const FOLDER_TYPE: &CStr = c"Folder";

/// What goes in front of a Demos tab row's name, and only sorts it: the list
/// sorts the name column by its text, so a leading space puts the up row,
/// the folders and the drives before the demos, and the up row's `..`
/// before any folder's name. The pictures are in the icon column
/// (`list_icons`, #779); before it, these were an arrow, a square and a
/// triangle drawn from a fallback font.
const FOLDER_MARK: &str = " ";
const DEMO_MARK: &str = "";
const UP_MARK: &str = " ";

/// What the Demos tab's Demo File column shows for a row's `path` from
/// `dod/`: a mark, then a demo's file name, a folder's name and a slash, or
/// `.. (up one folder)` for the up row, not the whole path (#409). The path
/// itself stays on the row, under [`PATH_KEY`], for loading.
///
/// A folder's demo count goes in brackets after its name, `count` (the Map
/// column is for maps only).
pub(super) fn display_name(path: &str, count: Option<&str>) -> String {
    let path = path.trim().trim_matches('"');
    let count = count.map_or_else(String::new, |c| format!(" ({c})"));
    if let Some(folder) = path.strip_suffix('/') {
        let last = folder.rsplit('/').next().unwrap_or(folder);
        if last == ".." {
            format!("{UP_MARK}.. up one folder{count}")
        } else {
            format!("{FOLDER_MARK}{last}/{count}")
        }
    } else {
        format!("{DEMO_MARK}{}", path.rsplit('/').next().unwrap_or(path))
    }
}

/// The Demos tab's line above the list, naming the folder the list is in: as
/// a path under the install folder's parent (`Half-Life - PRE-Anniversary for
/// Movies/dod/temp demos/`) when it is under it, otherwise in full -- that
/// parent itself included, where the path under it would be empty. A long
/// path keeps its end, to fit `max` characters.
pub(super) fn folder_line(dod: &std::path::Path, folder: &str, max: usize) -> String {
    if crate::demo_list_folders::is_studio_place(folder) {
        return "Currently in Studio's analysed demos".to_string();
    }
    if crate::demo_list_folders::is_drives_place(folder) {
        return "Currently in This PC (every drive)".to_string();
    }
    let here = crate::folder_counts::normalize(&dod.join(folder));
    let shown = dod
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(|common| here.strip_prefix(common).ok())
        .filter(|under| !under.as_os_str().is_empty())
        .map_or_else(|| here.to_path_buf(), std::path::Path::to_path_buf);
    let mut shown = shown.to_string_lossy().replace('\\', "/");
    if !shown.is_empty() && !shown.ends_with('/') {
        shown.push('/');
    }
    format!("Currently in {}", keep_the_end(&shown, max))
}

/// How wide the Go to list opens for its longest label of `longest`
/// characters: six pixels a character and room for the scroll bar, no
/// wider than the tab's `room` past the box, never narrower than the box.
pub(super) fn place_menu_width(longest: usize, box_wide: i32, room: i32) -> i32 {
    let text = i32::try_from(longest)
        .unwrap_or(i32::MAX)
        .saturating_mul(6)
        .saturating_add(32);
    text.min(room).max(box_wide)
}

/// The Demos tab's line above the list (`ui/Demos.res`).
pub(super) const FOLDER_LINE: &str = "FolderPath";

/// How much of a folder path fits the line above the list, `wide` pixels
/// wide. A label clips on the right at its width, which would cut off the
/// folder's own name; a long path loses folders from the left instead. Six
/// pixels a character, about the tab font's average, less "Currently in ".
pub(super) fn folder_line_chars(wide: i32) -> usize {
    (usize::try_from(wide / 6).unwrap_or(0))
        .saturating_sub(13)
        .max(20)
}

/// The folder the Demos tab's list shows: `dod/` (`""`) while folders are
/// off, when the game lists only `dod/`'s demos whatever folder Go to or Up
/// last went to.
pub(super) fn listed_folder(folders: bool, current: String) -> String {
    if folders { current } else { String::new() }
}

/// Whether the Demos tab's Back, Forward, Up and Go to can be used, in that
/// order, given whether each of the first three has somewhere to go. With
/// folders off none of them can: the list is `dod/`'s demos, wherever they
/// would go.
pub(super) fn nav_enabled(folders: bool, can_go: [bool; 3]) -> [bool; 4] {
    let [back, forward, up] = can_go;
    [back, forward, up, true].map(|on| folders && on)
}

/// `path` (folders, each ending in `/`) cut to at most about `max` characters
/// by dropping whole folders from the left behind `.../`. The last folder is
/// always kept, however long.
fn keep_the_end(path: &str, max: usize) -> String {
    if path.chars().count() <= max {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split_inclusive('/').collect();
    let mut kept = String::new();
    for part in parts.iter().rev() {
        let would = part.chars().count() + kept.chars().count() + 4;
        if !kept.is_empty() && would > max {
            break;
        }
        kept.insert_str(0, part);
    }
    format!(".../{kept}")
}

/// Whether a Demos tab row is a folder (#409 lists them as `name/`, quoted
/// when the name has a space) rather than a demo.
pub(super) fn is_folder_row(row: &str) -> bool {
    row.trim().trim_matches('"').ends_with('/')
}

/// A folder row's count, in brackets after its name, before it is in.
const COUNTING: &str = "counting...";

/// A folder row's demo count, in brackets after its name; `+` when the count stopped
/// short of the whole folder.
fn demo_count_text(count: crate::folder_counts::FolderCount) -> String {
    let more = if count.complete { "" } else { "+" };
    match count.demos {
        1 if count.complete => "1 demo".to_string(),
        n => format!("{n}{more} demos"),
    }
}

/// A folder row's count from its counts `direct` (in it) and `all` (at any
/// depth): `all` with `subfolders`, else `direct`. A folder with no demo
/// directly in it but some deeper says both, "0 here, 12 inside": Hide empty
/// folders keeps it (empty means none at any depth, or those demos couldn't
/// be reached), and "0 demos" alone would read as empty.
pub(super) fn folder_count_text(
    (direct, all): (
        crate::folder_counts::FolderCount,
        crate::folder_counts::FolderCount,
    ),
    subfolders: bool,
) -> String {
    if subfolders {
        demo_count_text(all)
    } else if direct.demos == 0 && direct.complete && all.demos > 0 {
        let more = if all.complete { "" } else { "+" };
        format!("0 here, {}{more} inside", all.demos)
    } else {
        demo_count_text(direct)
    }
}

/// What a folder row shows in brackets: its count, "counting..." until that
/// is in, or nothing while `ds_ui_demo_list_show_counts` is off (#787).
pub(super) fn shown_count(hidden: bool, count: Option<String>) -> Option<String> {
    (!hidden).then(|| count.unwrap_or_else(|| COUNTING.to_string()))
}

/// The four folder settings as one value, so a change to any lists again.
pub(super) fn folder_settings() -> u8 {
    use crate::demo_list_folders as f;
    f::enabled() as u8
        | (f::HIDE_EMPTY.on() as u8) << 1
        | (f::COUNT_SUBFOLDERS.on() as u8) << 2
        | (f::SHOW_COUNTS.on() as u8) << 3
}

/// Whether a folder setting's Settings-tab box shows: Show folders' demo
/// counts only while folders are listed, and the two boxes that need the
/// counts (Hide empty folders, Counts include subfolders) only while those are on too
/// (#787). `None` for every other box, which always shows.
pub(super) fn folder_box_shown(cvar: &str, folders: bool, counts: bool) -> Option<bool> {
    use crate::demo_list_folders as f;
    if cvar == f::SHOW_COUNTS.name {
        Some(folders)
    } else if cvar == f::HIDE_EMPTY.name || cvar == f::COUNT_SUBFOLDERS.name {
        Some(folders && counts)
    } else {
        None
    }
}

/// [`folder_settings`] before anything was seen: the tab has only just
/// filled its list.
pub(super) const SETTINGS_UNSEEN: u8 = 0xff;

/// Whether the folder settings (#409) changed since the Demos tab's list was
/// last filled.
pub(super) fn folders_changed(last: u8, now: u8) -> bool {
    last != SETTINGS_UNSEEN && last != now
}

/// Whether the Player box's Clear button has anything to clear: it is greyed
/// out while no player is picked (#800).
pub(super) fn clear_players_enabled(picked: &[String]) -> bool {
    !picked.is_empty()
}

/// The Demos tab's hint: how to load, and whether folders are listed, as a
/// state rather than a command that reads like one.
pub(super) fn demos_hint(folders: bool) -> &'static CStr {
    // No longer than the layout's first text, which fits its 416-wide label.
    if folders {
        c"Double-click a demo, or a folder to open it. Folders: on"
    } else {
        c"Double-click a demo to play it. Folders: off (ds_ui_demo_list_folders 1)"
    }
}

/// The name a row shows: its last part, without quotes or the folder's
/// `/`. A row is a path (`temp demos/x.dem`, `"D:/demos/x.dem"`), and the
/// search matches only what the list shows, so the folders above it (the
/// one the list is in) don't match every row.
fn row_name(row: &str) -> &str {
    let path = row.trim().trim_matches('"');
    let path = path.strip_suffix('/').unwrap_or(path);
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether a row passes every filter. `info` is `None` for a row whose
/// header couldn't be read (or a folder): only the search applies to it.
pub(super) fn passes(row: &str, info: Option<&DemoInfo>, f: &DemoFilters, now: u64) -> bool {
    let name = row_name(row);
    let haystack = match info {
        Some(i) => format!("{name} {}", i.map),
        None => name.to_string(),
    };
    if !matches_filter(&haystack, &f.search) {
        return false;
    }
    let Some(info) = info else { return true };
    if !f.map.trim().is_empty() && !matches_filter(&info.map, &f.map) {
        return false;
    }
    if (info.hltv && !f.hltv) || (!info.hltv && !f.pov) {
        return false;
    }
    if let Some(days) = f.days
        && now.saturating_sub(info.modified) > days * 86_400
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_search_matches_every_word_anywhere() {
        let row = "monday-wsod25_r07_m1_h1_hltv.dem";
        assert!(matches_filter(row, ""));
        assert!(matches_filter(row, "MONDAY"));
        assert!(matches_filter(row, "hltv monday"));
        assert!(matches_filter(row, "  r07  "));
        assert!(!matches_filter(row, "monday anzio"));
    }

    #[test]
    fn a_demo_header_gives_its_map_and_whether_hltv_recorded_it() {
        let mut header = vec![0u8; 544];
        header[..8].copy_from_slice(b"HLDEMO\0\0");
        header[16..16 + 13].copy_from_slice(b"dod_anzio\0xyz");
        let pov = demo_info(&header, 7).unwrap();
        assert_eq!(
            pov,
            DemoInfo {
                map: "dod_anzio".to_string(),
                hltv: false,
                modified: 7
            }
        );
        header.extend_from_slice(b"Spawn count 19 (HLTV)\n");
        assert!(demo_info(&header, 7).unwrap().hltv);
        assert_eq!(demo_info(b"not a demo", 0), None);
    }

    #[test]
    fn a_file_time_reads_as_a_date_that_sorts_as_text() {
        assert_eq!(date_text(civil(0)), "1970-01-01 00:00");
        // 2026-09-29 21:52:30 UTC; 2024-02-29, a leap day.
        assert_eq!(date_text(civil(1_790_718_750)), "2026-09-29 21:52");
        assert_eq!(date_text(civil(1_709_164_800)), "2024-02-29 00:00");
    }

    #[test]
    fn the_type_dropdown_picks_one_kind_or_both() {
        assert!(shows_type("All", true) && shows_type("All", false));
        assert!(shows_type("", true) && shows_type("", false));
        assert!(shows_type("HLTV", true) && !shows_type("HLTV", false));
        assert!(!shows_type("POV", true) && shows_type("pov", false));
    }

    #[test]
    fn the_demos_list_refills_only_when_folder_listing_flips() {
        // Not seen yet: the tab just filled its list on opening.
        assert!(!folders_changed(SETTINGS_UNSEEN, 1));
        assert!(!folders_changed(SETTINGS_UNSEEN, 0));
        assert!(folders_changed(1, 3), "hide empty turned on");
        assert!(folders_changed(0, 1));
        assert!(folders_changed(1, 0));
        assert!(!folders_changed(1, 1));
        assert!(!folders_changed(0, 0));
    }

    #[test]
    fn a_long_folder_path_keeps_its_end() {
        assert_eq!(keep_the_end("a/b/", 70), "a/b/");
        let deep = "D:/Games/Library/Mine/Steam/steamapps/common/Half-Life - PRE/dod/temp demos/";
        let short = keep_the_end(deep, 40);
        assert!(
            short.starts_with(".../") && short.ends_with("dod/temp demos/"),
            "{short}"
        );
        assert!(short.chars().count() <= 40, "{short}");
        // A single long last folder is kept whole.
        let one = format!("x/{}/", "y".repeat(80));
        assert!(keep_the_end(&one, 40).ends_with(&format!("{}/", "y".repeat(80))));
    }

    #[test]
    fn the_line_above_the_list_names_the_folder() {
        let dod = std::path::Path::new("C:/Steam/common/Half-Life - PRE/dod");
        assert_eq!(
            folder_line(dod, "", 90),
            "Currently in Half-Life - PRE/dod/"
        );
        assert_eq!(
            folder_line(dod, "temp demos/", 90),
            "Currently in Half-Life - PRE/dod/temp demos/"
        );
        assert_eq!(folder_line(dod, "../", 90), "Currently in Half-Life - PRE/");
        // The installs' own folder: nothing under it to show, so in full.
        assert_eq!(
            folder_line(dod, "../../", 90),
            "Currently in C:/Steam/common/"
        );
        assert_eq!(
            folder_line(dod, "temp demos/../", 90),
            "Currently in Half-Life - PRE/dod/"
        );
        // Above the installs: the whole path.
        assert_eq!(folder_line(dod, "../../../", 90), "Currently in C:/Steam/");
        // A narrow line keeps the folder's own name.
        assert_eq!(
            folder_line(dod, "temp demos/", 20),
            "Currently in .../dod/temp demos/"
        );
    }

    #[test]
    fn with_folders_off_the_navigation_greys_out_and_the_line_says_dod() {
        assert_eq!(
            nav_enabled(true, [true, false, true]),
            [true, false, true, true]
        );
        assert_eq!(nav_enabled(false, [true, true, true]), [false; 4]);
        assert_eq!(listed_folder(true, "D:/demos/".into()), "D:/demos/");
        assert_eq!(listed_folder(false, "D:/demos/".into()), "");
    }

    #[test]
    fn the_go_to_list_opens_wide_enough() {
        // A long recent folder: wider than the 150 px box.
        assert_eq!(place_menu_width(50, 150, 432), 332);
        // Never past the tab, never narrower than the box.
        assert_eq!(place_menu_width(200, 150, 432), 432);
        assert_eq!(place_menu_width(5, 150, 432), 150);
        assert_eq!(place_menu_width(50, 150, 0), 150);
    }

    #[test]
    fn the_line_fits_its_width() {
        // The drawn 276 px line: the cache place's line fits whole.
        let chars = folder_line_chars(276);
        assert!("Studio's analysed demos".len() <= chars, "{chars}");
        assert!(folder_line_chars(800) > chars);
        assert_eq!(folder_line_chars(0), 20);
    }

    #[test]
    fn rows_show_names_not_paths() {
        let demo = |n: &str| format!("{DEMO_MARK}{n}");
        let folder = |n: &str| format!("{FOLDER_MARK}{n}");
        let up = format!("{UP_MARK}.. up one folder");
        assert_eq!(
            display_name("temp demos/m3_h1.dem", None),
            demo("m3_h1.dem")
        );
        assert_eq!(
            display_name("\"temp demos/my clip.dem\"", None),
            demo("my clip.dem")
        );
        assert_eq!(display_name("a.dem", None), demo("a.dem"));
        assert_eq!(display_name("temp demos/", None), folder("temp demos/"));
        assert_eq!(display_name("../../steamapps/", None), folder("steamapps/"));
        assert_eq!(display_name("../", None), up);
        assert_eq!(display_name("temp demos/../", None), up);
        assert_eq!(display_name("../../../", None), up);
        // A folder's count in brackets after its name; a demo never gets one.
        assert_eq!(
            display_name("temp demos/", Some("73 demos")),
            folder("temp demos/ (73 demos)")
        );
        assert_eq!(
            display_name("../", Some("572 demos")),
            format!("{up} (572 demos)")
        );
        assert_eq!(display_name("a.dem", Some("1 demo")), demo("a.dem"));
    }

    #[test]
    fn folder_rows_are_told_from_demos() {
        for folder in [
            "../",
            "temp demos/",
            "\"temp demos/\"",
            "test/../",
            " sub/ ",
        ] {
            assert!(is_folder_row(folder), "{folder}");
        }
        for demo in ["a.dem", "test/a.dem", "\"temp demos/a b.dem\"", ""] {
            assert!(!is_folder_row(demo), "{demo}");
        }
    }

    #[test]
    fn a_folder_count_reads_as_text() {
        use crate::folder_counts::FolderCount;
        let c = |demos, complete| FolderCount { demos, complete };
        assert_eq!(demo_count_text(c(0, true)), "0 demos");
        assert_eq!(demo_count_text(c(1, true)), "1 demo");
        assert_eq!(demo_count_text(c(12, true)), "12 demos");
        assert_eq!(demo_count_text(c(1, false)), "1+ demos");
        assert_eq!(demo_count_text(c(500, false)), "500+ demos");
    }

    #[test]
    fn a_folder_with_demos_only_deeper_says_where_they_are() {
        use crate::folder_counts::FolderCount;
        let c = |demos, complete| FolderCount { demos, complete };
        // Include subfolders on: every demo at any depth.
        assert_eq!(
            folder_count_text((c(0, true), c(12, true)), true),
            "12 demos"
        );
        // Off: the demos in it, unless there are none there but some deeper.
        assert_eq!(
            folder_count_text((c(3, true), c(12, true)), false),
            "3 demos"
        );
        assert_eq!(
            folder_count_text((c(0, true), c(12, true)), false),
            "0 here, 12 inside"
        );
        assert_eq!(
            folder_count_text((c(0, true), c(500, false)), false),
            "0 here, 500+ inside"
        );
        assert_eq!(
            folder_count_text((c(0, true), c(0, true)), false),
            "0 demos"
        );
    }

    #[test]
    fn hidden_counts_leave_a_folder_row_its_name_alone() {
        let counted = Some("73 demos".to_string());
        assert_eq!(
            shown_count(false, counted.clone()).as_deref(),
            Some("73 demos")
        );
        assert_eq!(shown_count(false, None).as_deref(), Some(COUNTING));
        assert_eq!(shown_count(true, counted.clone()), None);
        assert_eq!(shown_count(true, None), None);
        assert_eq!(
            display_name("temp demos/", shown_count(true, counted).as_deref()),
            format!("{FOLDER_MARK}temp demos/")
        );
    }

    #[test]
    fn the_folder_sub_options_show_only_with_folders_and_counts() {
        let counts = "ds_ui_demo_list_show_counts";
        let hide_empty = "ds_ui_demo_list_hide_empty";
        let subfolders = "ds_ui_demo_list_count_subfolders";
        assert_eq!(folder_box_shown(counts, true, false), Some(true));
        assert_eq!(folder_box_shown(counts, false, true), Some(false));
        for cvar in [hide_empty, subfolders] {
            assert_eq!(folder_box_shown(cvar, true, true), Some(true));
            assert_eq!(folder_box_shown(cvar, true, false), Some(false), "{cvar}");
            assert_eq!(folder_box_shown(cvar, false, true), Some(false), "{cvar}");
        }
        assert_eq!(
            folder_box_shown("ds_ui_demo_list_folders", false, false),
            None
        );
        assert_eq!(folder_box_shown("hud_draw", false, false), None);
    }

    #[test]
    fn clear_is_enabled_only_while_a_player_is_picked() {
        assert!(!clear_players_enabled(&[]));
        assert!(clear_players_enabled(&["STEAM_0:1:6155141".to_string()]));
    }

    #[test]
    fn the_demos_hint_says_whether_folders_are_on() {
        let on = demos_hint(true).to_str().unwrap();
        let off = demos_hint(false).to_str().unwrap();
        assert!(on.contains("Folders: on"));
        assert!(off.contains("Folders: off") && off.contains("ds_ui_demo_list_folders 1"));
        // The layout's own text fits its label; neither may run longer.
        let first = "Double-click a demo, or pick one and Load. Folders: ds_ui_demo_list_folders 1";
        assert!(on.len() <= first.len() && off.len() <= first.len());
    }

    #[test]
    fn every_filter_has_to_pass() {
        let anzio = DemoInfo {
            map: "dod_anzio".to_string(),
            hltv: true,
            modified: 1_000_000,
        };
        let all = DemoFilters {
            search: String::new(),
            map: String::new(),
            hltv: true,
            pov: true,
            days: None,
        };
        let now = 1_000_000 + 3 * 86_400;
        assert!(passes("x.dem", Some(&anzio), &all, now));
        // The search matches the name shown, not the folders above it.
        let typed = |search: &str| DemoFilters {
            search: search.to_string(),
            ..all.clone()
        };
        assert!(!passes(
            "\"H:/Programs/Clips/\"",
            None,
            &typed("h:/programs"),
            now
        ));
        assert!(passes("\"H:/Programs/Clips/\"", None, &typed("clip"), now));
        assert!(!passes(
            "temp demos/x.dem",
            Some(&anzio),
            &typed("temp"),
            now
        ));
        assert_eq!(row_name("\"D:/demos/x.dem\""), "x.dem");
        assert_eq!(row_name("temp demos/"), "temp demos");
        // The search also sees the map.
        assert!(passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                search: "anzio".into(),
                ..all.clone()
            },
            now
        ));
        assert!(!passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                map: "flash".into(),
                ..all.clone()
            },
            now
        ));
        assert!(!passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                hltv: false,
                ..all.clone()
            },
            now
        ));
        assert!(passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                pov: false,
                ..all.clone()
            },
            now
        ));
        assert!(!passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                days: Some(2),
                ..all.clone()
            },
            now
        ));
        assert!(passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                days: Some(3),
                ..all.clone()
            },
            now
        ));
        // A folder (no header) only answers to the search.
        assert!(passes(
            "maps/",
            None,
            &DemoFilters {
                map: "flash".into(),
                ..all.clone()
            },
            now
        ));
    }
}
