//! The Demos tab (#409, #779): our own hidden Load Demo window's list,
//! filtered, given its columns and short names, with the Go to box, the
//! folder line and the navigation buttons; the pure parts are in
//! `studio_panel::demos_tab`.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// Bumped each time the demo list is refilled and stamped, so what is
/// worked out from its rows is worked out again.
pub(super) static DEMO_LIST_STAMPS: AtomicUsize = AtomicUsize::new(0);

pub(super) static DEMO_LIST_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
/// Our own Load Demo window, or 0.
pub(super) static DEMO_DIALOG: AtomicUsize = AtomicUsize::new(0);

/// The selected row's path in our Load Demo window's list ([`row_path_text`]'s
/// keys, in the same order).
unsafe fn selected_demo(dialog: *mut c_void) -> Option<String> {
    unsafe { selected_value(dialog, PATH_KEY).or_else(|| selected_value(dialog, ROW_KEY)) }
}

/// A row's path from `dod/`: the hidden [`PATH_KEY`] once the tab has
/// given it a display name, otherwise the list's own [`ROW_KEY`].
pub(super) unsafe fn row_path_text(row: *mut c_void) -> String {
    unsafe {
        let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
        for key in [PATH_KEY, ROW_KEY] {
            let raw = get_string(row, key.as_ptr(), c"".as_ptr());
            if !raw.is_null() {
                let value = text(raw);
                if !value.is_empty() {
                    return value;
                }
            }
        }
        String::new()
    }
}

/// Puts the selected row's path back in [`ROW_KEY`], for the window's
/// own handler (#409's folder opening reads it there).
unsafe fn restore_selected_path(dialog: *mut c_void) {
    unsafe {
        let Some(path) = selected_value(dialog, PATH_KEY) else {
            return;
        };
        let Ok((_, build)) = gameui() else { return };
        let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
        if list.is_null() {
            return;
        }
        let get_selected: ListIntFn = slot(list, LIST_SLOT_GET_SELECTED_ITEM);
        let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
        let row = get_item(list, get_selected(list, 0) as i32);
        if let (false, Ok(path)) = (row.is_null(), std::ffi::CString::new(path)) {
            let set_string: SetStringFn = slot(row, KEYVALUES_SLOT_SET_STRING);
            set_string(row, ROW_KEY.as_ptr(), path.as_ptr());
        }
    }
}

/// The selected row's `key` in a hidden Load Demo window's list.
pub(super) unsafe fn selected_value(dialog: *mut c_void, key: &CStr) -> Option<String> {
    unsafe {
        let (_, build) = gameui().ok()?;
        let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
        if list.is_null() {
            return None;
        }
        let get_selected: ListIntFn = slot(list, LIST_SLOT_GET_SELECTED_ITEM);
        let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
        let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
        let id = get_selected(list, 0) as i32;
        if is_valid(list, id) & 0xff == 0 {
            return None;
        }
        let row = get_item(list, id);
        if row.is_null() {
            return None;
        }
        let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
        let raw = get_string(row, key.as_ptr(), c"".as_ptr());
        (!raw.is_null())
            .then(|| text(raw))
            .filter(|t| !t.is_empty())
    }
}

/// Our Load Demo window's `OnCommand`. Load (and a double-click, which
/// sends the same command) on a demo runs `viewdemo` here: the stock
/// window would post it to its parent for the VCR bar to run, and our
/// window is no VCR bar. A folder row (#409) and everything else go to
/// the window's own handler.
pub(super) unsafe extern "thiscall" fn demo_list_on_command(this: *mut c_void, raw: *const c_char) {
    let command = text(raw);
    if command.eq_ignore_ascii_case("load") {
        match unsafe { selected_demo(this) } {
            // A double-click loads even with Load greyed out (#826).
            Some(row)
                if !row.trim_end_matches('"').ends_with('/') && crate::review_lock::locked() =>
            {
                crate::commands::console_print(&format!(
                    "{NAME}: {}\n",
                    crate::review_lock::LOCKED_NOTE
                ));
                return;
            }
            Some(row) if !row.trim_end_matches('"').ends_with('/') => {
                let line = viewdemo_line(&row);
                let ran = std::ffi::CString::new(line.clone())
                    .is_ok_and(|l| crate::engine::client_cmd(&l));
                unsafe {
                    crate::debug::report(&format!(
                        "studio_panel: Demos tab {} {}",
                        if ran { "ran" } else { "could not run" },
                        line.trim()
                    ))
                };
                return;
            }
            // A folder: #409's handler opens it, reading the path from
            // the column the tab gave a display name.
            Some(_) => unsafe { restore_selected_path(this) },
            // Nothing picked: Enter anywhere on the tab lands here too
            // (the window's default button), so no console line (#777);
            // the hint under the list already says what to do.
            None => return,
        }
    }
    let original = DEMO_LIST_ON_COMMAND.load(Ordering::Acquire);
    if original != 0 {
        // Safety: the window's own OnCommand, from its vftable.
        let original: OnCommandFn = unsafe { std::mem::transmute(original) };
        unsafe { original(this, raw) };
    }
}

/// What the search box held when the list was last filtered, and
/// whether the list has been refilled since (every row shows again).
static FILTERED_FOR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// The folder and the filters as typed when the list was last scrolled
/// to the top: a new folder or search starts at the top, a background
/// refilter (rosters, a recount) leaves the scroll alone.
static SCROLLED_FOR: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// [`folder_settings`] as the Demos tab's list was last filled with them.
static LISTED_FOLDERS: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(SETTINGS_UNSEEN);

/// The Demos page whose hint last got [`demos_hint`]'s text, so a rebuilt
/// window gets it too.
static HINTED_PAGE: AtomicUsize = AtomicUsize::new(0);

/// The text last put on the line above the list, and on which page.
static HEADED: std::sync::Mutex<(usize, String)> = std::sync::Mutex::new((0, String::new()));

/// Each demo's header facts, by its path, kept while the file is unchanged.
static DEMO_INFO: std::sync::Mutex<Vec<(String, u64, Option<DemoInfo>)>> =
    std::sync::Mutex::new(Vec::new());

/// How much of a demo `demo_info` reads.
const DEMO_START: usize = 4096;

/// The demo file a row names (a path from `dod/`), or `None` for a
/// folder row.
pub(super) fn row_path(row: &str) -> Option<std::path::PathBuf> {
    let name = row.trim().trim_matches('"');
    if name.ends_with('/') || name.is_empty() {
        return None;
    }
    Some(crate::demo_list_folders::full_path(&dod_dir(), name))
}

/// The header facts of the demo a row names (a path from `dod/`).
fn info_for(row: &str) -> Option<DemoInfo> {
    let name = row.trim().trim_matches('"');
    let path = row_path(row)?;
    let meta = std::fs::metadata(&path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let mut cache = DEMO_INFO.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, m, info)) = cache.iter().find(|(p, _, _)| p == name)
        && *m == modified
    {
        return info.clone();
    }
    use std::io::Read;
    let mut header = Vec::with_capacity(DEMO_START);
    let info = std::fs::File::open(&path)
        .and_then(|f| f.take(DEMO_START as u64).read_to_end(&mut header))
        .ok()
        .and_then(|_| demo_info(&header, modified));
    cache.retain(|(p, _, _)| p != name);
    cache.push((name.to_string(), modified, info.clone()));
    info
}

/// Shows only the demos matching the Demos tab's filters. Runs every
/// frame; does work only when a filter (or the list) changed.
pub(super) unsafe fn filter_demo_list(vgui: &Vgui) {
    unsafe {
        let page = vpanel_of(PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void);
        if page == 0 || !vgui.visible(page) {
            return;
        }
        // The list is filled when the tab opens. Changing a folder
        // setting (#409) while it shows lists it again, so the folders
        // appear, go or recount without reopening the window.
        let folders = folder_settings();
        let last = LISTED_FOLDERS.swap(folders, Ordering::AcqRel);
        if folders_changed(last, folders) || crate::folder_counts::take_finished() {
            refill_demo_list();
        }
        // The hint says whether folders are on, not just the command.
        let new_page = HINTED_PAGE.swap(page as usize, Ordering::AcqRel) != page as usize;
        // The line above the list says which folder the list is in:
        // `dod/` while folders are off, the game's own listing.
        // The line stretches with the tab, so how much path fits does too.
        let line = vgui.child_named(page, FOLDER_LINE);
        let line_text = match line {
            Some(line) => folder_line(
                &dod_dir(),
                &listed_folder(folders & 1 == 1, crate::demo_list_folders::current_folder()),
                folder_line_chars(vgui.rect(line).2),
            ),
            None => String::new(),
        };
        let mut headed = HEADED.lock().unwrap_or_else(|e| e.into_inner());
        if *headed != (page as usize, line_text.clone()) {
            if let Some(line) = line
                && let Ok(text) = std::ffi::CString::new(line_text.clone())
            {
                let object = vgui.object(line);
                if !object.is_null() {
                    let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
                    set_text(object, text.as_ptr());
                }
            }
            *headed = (page as usize, line_text);
        }
        drop(headed);
        if (last != folders || new_page)
            && let Some(hint) = vgui.child_named(page, DEMOS_HINT)
        {
            let object = vgui.object(hint);
            if !object.is_null() {
                let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
                set_text(object, demos_hint(folders & 1 == 1).as_ptr());
            }
        }
        // The Go to dropdown: a pick goes to that place, and the list
        // fills again, which puts the heading back (#779).
        let picked = box_text(vgui, page, DEMO_PLACE);
        if folders & 1 == 1 && !picked.is_empty() && picked.as_bytes() != PLACE_HEADING.to_bytes() {
            let folder = PLACES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|p| p.label == picked)
                .map(|p| p.folder.clone());
            if let Some(folder) = folder {
                crate::demo_list_folders::go_to(&folder);
            }
            refill_demo_list();
        }
        widen_place_menu(vgui, page);
        show_nav_enabled(vgui, page, folders & 1 == 1, new_page);
        show_clear_enabled(vgui, page, new_page);
        let filters = DemoFilters {
            search: box_text(vgui, page, DEMO_FILTER),
            map: box_text(vgui, page, MAP_FILTER),
            hltv: shows_type(&box_text(vgui, page, DEMO_TYPE), true),
            pov: shows_type(&box_text(vgui, page, DEMO_TYPE), false),
            days: box_text(vgui, page, DAYS_FILTER).trim().parse::<u64>().ok(),
        };
        // The picked players only: what is typed in the Player box
        // searches its list, not the demos.
        let terms = PICKED_PLAYERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let player = terms.join(" | ");
        let all = box_text(vgui, page, PLAYER_MATCH)
            .trim()
            .eq_ignore_ascii_case("All");
        let recorded = box_ticked_or(vgui, page, PLAYER_RECORDED, false);
        if !terms.is_empty() {
            crate::demo_rosters::ensure_filled();
        }
        // Rosters written in the background filter the list again.
        let filter = format!(
            "{filters:?} {player:?} {all} {recorded} {}",
            crate::demo_rosters::generation()
        );
        let dialog = DEMO_DIALOG.load(Ordering::Acquire) as *mut c_void;
        let Ok((_, build)) = gameui() else { return };
        if dialog.is_null() {
            return;
        }
        let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
        if list.is_null() {
            return;
        }
        let mut last = FILTERED_FOR.lock().unwrap_or_else(|e| e.into_inner());
        // A refilled list shows every row again: stamp it and filter afresh.
        if stamp_demo_rows(list) {
            resort_demo_list(list, build);
            *last = None;
        }
        if last.as_deref() == Some(filter.as_str()) {
            return;
        }
        let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
        let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
        let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
        let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
        let set_visible: ListSetVisibleFn = slot(list, LIST_SLOT_SET_ITEM_VISIBLE);
        let get_string_of = |row: *mut c_void| -> String { row_path_text(row) };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // Header reads only when a filter needs them.
        let need_info = !filters.map.trim().is_empty()
            || !filters.hltv
            || !filters.pov
            || filters.days.is_some()
            || !filters.search.trim().is_empty();
        let (mut demos, mut with_players, mut matched) = (0, 0, 0);
        let mut id = first(list);
        let mut guard = 0;
        while is_valid(list, id) & 0xff != 0 && guard < 100_000 {
            guard += 1;
            let row = get_item(list, id);
            if !row.is_null() {
                let name = get_string_of(row);
                let info = if need_info { info_for(&name) } else { None };
                let mut shown = passes(&name, info.as_ref(), &filters, now);
                // A folder row stays; a demo needs its players file to match.
                if !player.is_empty()
                    && let Some(path) = row_path(&name)
                {
                    demos += 1;
                    let players = crate::demo_rosters::players_for(&path);
                    with_players += players.is_some() as usize;
                    shown &= players.is_some_and(|r| {
                        crate::demo_rosters::has_players(&r, &terms, all, recorded)
                    });
                    matched += shown as usize;
                }
                set_visible(list, id, shown as u32);
            }
            id = next(list, id);
        }
        // A new folder or search starts at the top, and puts the scroll
        // bar back in the range that a shorter list shrank.
        let typed = format!(
            "{:?} {filters:?} {player:?} {all} {recorded}",
            crate::demo_list_folders::current_folder()
        );
        let mut scrolled = SCROLLED_FOR.lock().unwrap_or_else(|e| e.into_inner());
        if *scrolled != typed {
            crate::list_panel::scroll_to_top(list);
            *scrolled = typed;
        }
        drop(scrolled);
        show_player_note(
            vgui,
            page,
            (!player.is_empty()).then_some((with_players, demos)),
        );
        if !player.is_empty() {
            crate::debug::report(&format!(
                "studio_panel: Demos tab Player {player:?}{}: {matched} shown, {with_players} of {demos} demos analysed",
                if recorded { " (recorded it)" } else { "" }
            ));
        }
        *last = Some(filter);
    }
}

/// While the Player filter is in use, says how many demos it could look
/// in, in place of the hint; `None` puts the hint back.
unsafe fn show_player_note(vgui: &Vgui, page: Vpanel, counts: Option<(usize, usize)>) {
    unsafe {
        if let Some(hint) = vgui.child_named(page, DEMOS_HINT) {
            vgui.set_visible(hint, counts.is_none());
        }
        let Some(note) = vgui.child_named(page, PLAYER_NOTE) else {
            return;
        };
        vgui.set_visible(note, counts.is_some());
        if let Some((analysed, demos)) = counts {
            let text = format!(
                "Player looks in the {analysed} of {demos} demos analysed so far (Studio, or the Highlights tab)."
            );
            let object = vgui.object(note);
            if let (false, Ok(text)) = (object.is_null(), std::ffi::CString::new(text)) {
                let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
                set_text(object, text.as_ptr());
            }
        }
    }
}

/// Gives our Load Demo window's list [`DEMO_COLUMNS`] after the
/// demo's name, every column sortable by a click on its heading. The name
/// column stays the window's own (as wide as its `.res` list, so it is
/// narrowed through its heading, which is a panel named after the column):
/// `RemoveColumn` marks the column's sort-history byte 0xff instead of
/// removing it, and `DeleteAllItems` then indexes column 0xff and crashes.
pub(super) unsafe fn add_demo_columns(vgui: &Vgui, dialog: *mut c_void) {
    unsafe {
        let Ok((_, build)) = gameui() else { return };
        let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
        if list.is_null() {
            return;
        }
        let sortable: ListIntBoolFn = slot(list, LIST_SLOT_SET_COLUMN_SORTABLE);
        let delete_all: ListVoidFn = slot(list, LIST_SLOT_DELETE_ALL_ITEMS);
        // A row remembers its place in each column's sort order by column
        // position, so columns change only while the list is empty (the
        // window filled it as it was built); `refill_demo_list` refills it.
        delete_all(list);
        // The icon column first (#779): an image column, fixed width,
        // drawn from our own image list; a row picks its icon with the
        // `int` under the column's key.
        add_column(
            list,
            0,
            list_icons::ICON_KEY,
            c"",
            list_icons::ICON_COLUMN_WIDE,
            list_icons::COLUMN_FIXEDSIZE | list_icons::COLUMN_IMAGE,
        );
        list_icons::install(vgui.surface, list);
        for (index, (key, heading, width)) in DEMO_COLUMNS.iter().enumerate() {
            add_column(list, index as i32 + 2, key, heading, *width, 0);
        }
        if let Some(heading) = vgui.child_named(vpanel_of(list), "demoname") {
            let (x, y, _, tall) = vgui.rect(heading);
            vgui.place(heading, (x, y, NAME_COLUMN_WIDE, tall));
        }
        renumber_draggers(list, build);
        // Every column but the icons' sorts by a click on its heading.
        for index in 1..=DEMO_COLUMNS.len() as i32 + 1 {
            sortable(list, index, 1);
        }
    }
}

/// Points every column's drag handle at the column's position now, as
/// `ListPanel::OnColumnResized` (and a double-click's
/// `ResizeColumnToContents`) looks the column up by position: without it
/// Demo File's handle, added at 0 before the icon column was put there,
/// resized the fixed icon column instead (#801).
unsafe fn renumber_draggers(list: *mut c_void, build: &Build) {
    unsafe {
        let base = list as *const u8;
        let records = *(base.add(build.list_columns) as *const *const u8);
        let allocated = *(base.add(build.list_columns + 4) as *const i32);
        let order = *(base.add(build.list_column_order) as *const *const u8);
        let count = *(base.add(build.list_column_order + 0xc) as *const i32);
        // A list never holds more columns than its one-byte record index
        // can name.
        if records.is_null() || order.is_null() || !(0..=0x100).contains(&count) {
            return;
        }
        let order = std::slice::from_raw_parts(order, count as usize);
        for (record, position) in dragger_positions(order, allocated.max(0) as usize) {
            let at = records.add(record * COLUMN_RECORD_SIZE + COLUMN_DRAGGER);
            let dragger = *(at as *const *mut u8);
            if !dragger.is_null() {
                *(dragger.add(build.dragger_column) as *mut i32) = position;
            }
        }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SystemTimeToTzSpecificLocalTime(
        zone: *const c_void,
        utc: *const [u16; 8],
        local: *mut [u16; 8],
    ) -> i32;
}

/// A file time as the Date column shows it, in local time (with the
/// daylight saving of that date, as Explorer shows it).
fn local_date(secs: u64) -> String {
    let (year, month, day, hour, minute) = civil(secs);
    let utc = [
        year as u16,
        month as u16,
        0,
        day as u16,
        hour as u16,
        minute as u16,
        0,
        0,
    ];
    let mut local = [0u16; 8];
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } == 0 {
        return date_text((year, month, day, hour, minute));
    }
    let [year, month, _, day, hour, minute, ..] = local.map(u64::from);
    date_text((year, month, day, hour, minute))
}

/// Sorts the Demos tab's rows again by what they show now. The list
/// sorted them as the window added them, by their whole paths, and
/// [`stamp_demo_rows`] renaming a row doesn't move it, so a refill after
/// a folder setting changed left the folders below the demos while the
/// heading still said ascending (`list_panel`, #796). With no column
/// chosen: by Demo File (column 1, after the icons), ascending.
unsafe fn resort_demo_list(list: *mut c_void, build: &Build) {
    unsafe { crate::list_panel::resort(list, build.list_sort_column, Some(1)) }
}

/// Fills in every row's Map, Type, Player and Date, and its short name, when
/// the list was (re)filled or the players files changed since the last
/// time. Returns whether it did.
unsafe fn stamp_demo_rows(list: *mut c_void) -> bool {
    unsafe {
        let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
        let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
        let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
        let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
        let apply_changes: ListIntVoidFn = slot(list, LIST_SLOT_APPLY_ITEM_CHANGES);
        let get = |row: *mut c_void, key: &CStr| -> String {
            let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
            let raw = get_string(row, key.as_ptr(), c"".as_ptr());
            if raw.is_null() {
                String::new()
            } else {
                text(raw)
            }
        };
        let id = first(list);
        if is_valid(list, id) & 0xff == 0 {
            return false;
        }
        let row = get_item(list, id);
        // Stamped with the players files' generation, so names that
        // arrive later (an analysis, the background fill) are shown.
        crate::demo_rosters::ensure_filled();
        let stamp = std::ffi::CString::new(crate::demo_rosters::generation().to_string())
            .unwrap_or_default();
        if row.is_null() || get(row, STAMP_KEY) == stamp.to_string_lossy() {
            return false;
        }
        let mut id = id;
        let mut guard = 0;
        while is_valid(list, id) & 0xff != 0 && guard < 100_000 {
            guard += 1;
            let row = get_item(list, id);
            if !row.is_null() {
                let set_string: SetStringFn = slot(row, KEYVALUES_SLOT_SET_STRING);
                // The path moves to the hidden key the first time, and
                // the first column gets the short name (end of the loop).
                let path = row_path_text(row);
                if get(row, PATH_KEY).is_empty()
                    && let Ok(c_path) = std::ffi::CString::new(path.clone())
                {
                    set_string(row, PATH_KEY.as_ptr(), c_path.as_ptr());
                }
                if let Some(info) = info_for(&path)
                    && let Ok(map) = std::ffi::CString::new(info.map.clone())
                    && let Ok(date) = std::ffi::CString::new(local_date(info.modified))
                {
                    set_string(row, DEMO_COLUMNS[0].0.as_ptr(), map.as_ptr());
                    let kind = if info.hltv { c"HLTV" } else { c"POV" };
                    set_string(row, DEMO_COLUMNS[1].0.as_ptr(), kind.as_ptr());
                    set_string(row, DEMO_COLUMNS[3].0.as_ptr(), date.as_ptr());
                }
                // A folder row (#409) says so in the Type column, so it
                // can't be mistaken for a demo, and its demo count goes
                // in brackets after its name (`display_name`), leaving
                // the Map column to maps.
                let name = path.clone();
                let mut folder_count = None;
                // The icon column (#779): the row's int picks the image.
                let is_up = name
                    .trim()
                    .trim_matches('"')
                    .strip_suffix('/')
                    .is_some_and(|folder| folder.rsplit('/').next() == Some(".."));
                let icon = list_icons::icon_for(
                    is_folder_row(&name),
                    is_up,
                    crate::demo_list_folders::is_drive_root(&name),
                    info_for(&path).map(|info| info.hltv),
                );
                let set_int: SetIntFn = slot(row, list_icons::KEYVALUES_SLOT_SET_INT);
                set_int(row, list_icons::ICON_KEY.as_ptr(), icon as i32);
                if is_folder_row(&name) {
                    set_string(row, DEMO_COLUMNS[1].0.as_ptr(), FOLDER_TYPE.as_ptr());
                    // Counted in the background: "counting..." until
                    // then, and the tab lists again when it is done. A
                    // drive row (#779) is never counted: it says Drive.
                    if crate::demo_list_folders::is_drive_root(&name) {
                        set_string(row, DEMO_COLUMNS[1].0.as_ptr(), DRIVE_TYPE.as_ptr());
                    } else {
                        let folder = crate::demo_list_folders::full_path(&dod_dir(), &name);
                        let count = crate::folder_counts::counts(&folder).map(|counts| {
                            folder_count_text(
                                counts,
                                crate::demo_list_folders::COUNT_SUBFOLDERS.on(),
                            )
                        });
                        folder_count =
                            shown_count(!crate::demo_list_folders::SHOW_COUNTS.on(), count);
                    }
                }
                if let Some(info) = info_for(&path) {
                    // A dash for HLTV (nobody recorded it); blank for a
                    // POV demo not analysed yet.
                    let player = if info.hltv {
                        "-".to_string()
                    } else {
                        row_path(&path)
                            .and_then(|path| crate::demo_rosters::players_for(&path))
                            .and_then(|d| d.players.into_iter().find(|p| p.recorder))
                            .map(|p| p.name.replace('\0', ""))
                            .unwrap_or_default()
                    };
                    if let Ok(player) = std::ffi::CString::new(player) {
                        set_string(row, DEMO_COLUMNS[2].0.as_ptr(), player.as_ptr());
                    }
                }
                if let Ok(shown) =
                    std::ffi::CString::new(display_name(&path, folder_count.as_deref()))
                {
                    set_string(row, ROW_KEY.as_ptr(), shown.as_ptr());
                }
                set_string(row, STAMP_KEY.as_ptr(), stamp.as_ptr());
                // Each column keeps its rows sorted as they were added;
                // re-sort this one into the new columns.
                apply_changes(list, id);
            }
            id = next(list, id);
        }
        DEMO_LIST_STAMPS.fetch_add(1, Ordering::AcqRel);
        true
    }
}

/// What the Go to dropdown offered last, by label, for the pick (#779).
static PLACES: std::sync::Mutex<Vec<crate::demo_list_folders::Place>> =
    std::sync::Mutex::new(Vec::new());

/// Fills the Demos tab's Go to dropdown: the heading, then every place
/// (#779). Again at each list fill, so a new drive or one of Studio's
/// folders shows up, and after a pick, which puts the heading back.
pub(super) unsafe fn fill_place_box(vgui: &Vgui, base: usize, build: &Build) {
    let demos_page =
        unsafe { vpanel_of(PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void) };
    if demos_page == 0 {
        return;
    }
    // Only a ComboBox (by its vftable) is filled, as the Type box is.
    let found = unsafe {
        vgui.child_named(demos_page, DEMO_PLACE)
            .map(|vp| vgui.object(vp))
            .filter(|o| !o.is_null() && *(*o as *const usize) == base + build.combo_box_vftable)
    };
    let Some(o) = found else {
        return;
    };
    let places = crate::demo_list_folders::places();
    unsafe {
        let clear: ActivateFn = slot(o, COMBO_SLOT_DELETE_ALL_ITEMS);
        clear(o);
        let add_item: ComboAddItemFn = slot(o, COMBO_SLOT_ADD_ITEM);
        add_item(o, PLACE_HEADING.as_ptr(), std::ptr::null());
        for place in &places {
            if let Ok(label) = std::ffi::CString::new(place.label.clone()) {
                add_item(o, label.as_ptr(), std::ptr::null());
            }
        }
        let activate_row: ListIntVoidFn = slot(o, COMBO_SLOT_ACTIVATE_ITEM_BY_ROW);
        activate_row(o, 0);
    }
    *PLACES.lock().unwrap_or_else(|e| e.into_inner()) = places;
}

/// Back, Forward, Up and Go to as last set, a bit each in that order;
/// `0xff` for not yet. Set again only on a change or a new page.
static NAV_ENABLED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0xff);

/// Greys out the Demos tab's Back, Forward and Up where each has nowhere
/// to go ([`nav_enabled`]), and all of them and Go to while folders are
/// off.
unsafe fn show_nav_enabled(vgui: &Vgui, page: Vpanel, folders: bool, new_page: bool) {
    use crate::demo_list_folders::{can_go_back, can_go_forward, can_go_up};
    let can = nav_enabled(folders, [can_go_back(), can_go_forward(), can_go_up()]);
    let want = can
        .iter()
        .enumerate()
        .fold(0u8, |bits, (i, &on)| bits | (u8::from(on) << i));
    if !new_page && NAV_ENABLED.load(Ordering::Acquire) == want {
        return;
    }
    let names = DEMO_NAV_BUTTONS.iter().copied().chain([DEMO_PLACE]);
    for (name, on) in names.zip(can) {
        unsafe {
            let Some(vp) = vgui.child_named(page, name) else {
                continue;
            };
            let button = vgui.object(vp);
            if button.is_null() {
                continue;
            }
            let set_enabled: ListIntVoidFn = slot(button, PANEL_SLOT_SET_ENABLED);
            set_enabled(button, i32::from(on));
        }
    }
    NAV_ENABLED.store(want, Ordering::Release);
}

/// The Clear button as last set: 0 or 1, `0xff` for not yet. Set again
/// only on a change or a new page.
static CLEAR_ENABLED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0xff);

/// Greys out the Player box's Clear button while no player is picked
/// (#800), the way [`show_nav_enabled`] does the navigation.
unsafe fn show_clear_enabled(vgui: &Vgui, page: Vpanel, new_page: bool) {
    let on = clear_players_enabled(&PICKED_PLAYERS.lock().unwrap_or_else(|e| e.into_inner()));
    let want = u8::from(on);
    if !new_page && CLEAR_ENABLED.load(Ordering::Acquire) == want {
        return;
    }
    unsafe {
        let Some(vp) = vgui.child_named(page, PLAYER_CLEAR) else {
            return;
        };
        let button = vgui.object(vp);
        if button.is_null() {
            return;
        }
        let set_enabled: ListIntVoidFn = slot(button, PANEL_SLOT_SET_ENABLED);
        set_enabled(button, i32::from(on));
    }
    CLEAR_ENABLED.store(want, Ordering::Release);
}

/// Widens the Go to list, while it is open, to fit its longest label
/// (as far as the tab has room). Every frame, since the box narrows it
/// again each time it opens.
unsafe fn widen_place_menu(vgui: &Vgui, page: Vpanel) {
    let Ok((base, build)) = gameui() else {
        return;
    };
    unsafe {
        let Some(vp) = vgui.child_named(page, DEMO_PLACE) else {
            return;
        };
        let combo = vgui.object(vp);
        if combo.is_null() || *(combo as *const usize) != base + build.combo_box_vftable {
            return;
        }
        let menu = *((combo as *const u8).add(build.combo_menu) as *const *mut c_void);
        if menu.is_null() || !vgui.visible(vpanel_of(menu)) {
            return;
        }
        let longest = PLACES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|p| p.label.chars().count())
            .max()
            .unwrap_or(0);
        let (x, _, w, _) = vgui.rect(vp);
        let (_, _, page_w, _) = vgui.rect(page);
        let set_width: ListIntVoidFn = slot(menu, MENU_SLOT_SET_FIXED_WIDTH);
        set_width(menu, place_menu_width(longest, w, page_w - x - 8));
    }
}

/// Back, Forward or Up on the Demos tab: `step` moves the lists'
/// folder, and a move lists again.
pub(super) unsafe fn navigate(step: fn() -> bool) -> Result<(), String> {
    if step() {
        unsafe { refill_demo_list() };
    }
    Ok(())
}

/// Lists the demos again in our Load Demo window (new recordings, or a
/// folder change), with the window's own fill.
pub(super) unsafe fn refill_demo_list() {
    let dialog = DEMO_DIALOG.load(Ordering::Acquire) as *mut c_void;
    if dialog.is_null() {
        return;
    }
    if let Ok((base, build)) = gameui() {
        unsafe {
            let fill: ActivateFn = std::mem::transmute(base + build.file_dialog_fill);
            crate::demo_list_folders::tab_fill(|| fill(dialog));
            if let Ok(vgui) = Vgui::get() {
                fill_place_box(&vgui, base, build);
            }
        }
        // Every row shows again: filter afresh.
        *FILTERED_FOR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
