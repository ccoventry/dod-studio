//! The Highlights tab (#565): the playing demo's streaks, found by
//! [`crate::streaks`], in a list borrowed the way the Demos tab's is. A
//! second hidden Load Demo window (`STREAK_LIST`) lends its list and Load
//! button ([`LOANS`]); its demo rows are cleared, its columns become Row #,
//! Player, Kills, Time, Dur. and Details, its Load button is Go, and its `OnCommand` is
//! ours: Go, or a double-click, seeks to just before the streak.
//!
//! Every life with a kill is listed; a Min kills box narrows the list. A POV
//! demo lists only the recording player's (the footage follows no one else).
//! An HLTV demo lists everyone's, a Player box narrows them, and Go also puts
//! the camera on the streak's player (`dodstudio_spec_target`).

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::streaks::{self, Found, Status, Streak};

static DIALOG: AtomicUsize = AtomicUsize::new(0);
static ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
/// What the status line and the list show now, so each is redrawn only when
/// it changes.
static SHOWN_STATUS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static SHOWN_ROWS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// The Player and Min kills boxes' text the list was last narrowed by;
/// `None` after a refill.
static FILTERED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The row keys Go reads: the bar time to seek to, and in an HLTV demo the
/// player's number to put the camera on.
const SEEK_KEY: &CStr = c"seek";
const TARGET_KEY: &CStr = c"target";
/// The columns past the row number (which is the window's own `demoname`
/// column, retitled), as the Capture page's Highlights table names them:
/// key, heading, width. The Player column is hidden for a POV demo.
const COLUMNS: [(&CStr, &CStr, i32); 5] = [
    (c"player", c"Player", 140),
    (c"kills", c"Kills", 40),
    (c"time", c"Time", 50),
    (c"dur", c"Dur.", 40),
    (c"details", c"Details", 210),
];
const PLAYER_COLUMN: i32 = 1;
const ROW_COLUMN_WIDE: i32 = 44;
const STATUS_LABEL: &str = "StreakStatus";
const PROGRESS: &str = "StreakProgress";
/// The Player box, and its label: shown for an HLTV demo only.
const PLAYER_BOX: &str = "StreakPlayer";
const PLAYER_LABEL: &str = "StreakPlayerLabel";
/// The Min kills box, and its label: shown once the streaks are found.
const MIN_KILLS_BOX: &str = "StreakMinKills";
const MIN_KILLS_LABEL: &str = "StreakMinKillsLabel";

type KeyValuesCtor = unsafe extern "thiscall" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *const c_char,
) -> *mut c_void;
type AddItemFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, u32, u32, u32) -> i32;
type SetProgressFn = unsafe extern "thiscall" fn(*mut c_void, f32);

/// Builds the hidden window and turns its list into the streak list.
pub(super) unsafe fn build(
    vgui: &Vgui,
    base: usize,
    build: &Build,
    frame: *mut c_void,
) -> Result<(), String> {
    unsafe {
        let dialog = allocate(base, build, build.frame_size + 4)?;
        let dialog_ctor: SheetCtor = std::mem::transmute(base + build.file_dialog_ctor);
        let name = CString::new(STREAK_LIST).map_err(|e| e.to_string())?;
        dialog_ctor(dialog, frame, name.as_ptr());
        let dialog_vp = vpanel_of(dialog);
        vgui.set_visible(dialog_vp, false);
        ON_COMMAND.store(
            own_on_command(dialog, on_command as *const () as usize),
            Ordering::Release,
        );
        DIALOG.store(dialog as usize, Ordering::Release);
        SHOWN_STATUS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        SHOWN_ROWS.lock().unwrap_or_else(|e| e.into_inner()).clear();

        let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
        if list.is_null() {
            return Ok(());
        }
        // Columns change only while the list is empty (see add_demo_columns).
        let delete_all: ListVoidFn = slot(list, LIST_SLOT_DELETE_ALL_ITEMS);
        delete_all(list);
        let add: AddColumnFn = slot(list, LIST_SLOT_ADD_COLUMN_HEADER);
        for (index, (key, heading, width)) in COLUMNS.iter().enumerate() {
            add(
                list,
                index as i32 + 1,
                key.as_ptr(),
                heading.as_ptr(),
                *width,
                0,
            );
        }
        let sortable: ListIntBoolFn = slot(list, LIST_SLOT_SET_COLUMN_SORTABLE);
        for index in 0..=COLUMNS.len() as i32 {
            sortable(list, index, 1);
        }
        if let Some(heading) = vgui.child_named(vpanel_of(list), "demoname") {
            let (x, y, _, tall) = vgui.rect(heading);
            vgui.place(heading, (x, y, ROW_COLUMN_WIDE, tall));
            set_label(vgui.object(heading), "Row #");
        }
        if let Some(button) = vgui.child_named(dialog_vp, "LoadButton") {
            set_label(vgui.object(button), "Go");
        }
        Ok(())
    }
}

/// Keeps the tab showing the playing demo's streaks. Runs every frame; asks
/// for an analysis only while the tab is showing.
pub(super) unsafe fn update(vgui: &Vgui) {
    unsafe {
        let page = vpanel_of(PAGE_OBJECTS[STREAKS_PAGE].load(Ordering::Acquire) as *mut c_void);
        if page == 0 || !vgui.visible(page) {
            return;
        }
        let Ok((base, build)) = gameui() else { return };
        let dialog = DIALOG.load(Ordering::Acquire) as *mut c_void;
        if dialog.is_null() {
            return;
        }
        let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
        if list.is_null() {
            return;
        }
        let Some(name) = crate::demo_reload::current_demo() else {
            show(
                vgui,
                page,
                build,
                (
                    "none",
                    "No demo yet: play one with viewdemo, then come back here.",
                ),
                None,
            );
            fill(list, base, build, "none", &[], false);
            show_filters(vgui, page, None);
            return;
        };
        let Some(game_dir) = res_dir().parent().map(std::path::Path::to_path_buf) else {
            return;
        };
        let path = streaks::demo_path(&game_dir, &name);
        streaks::request(&path);
        let Some((generation, status, percent)) = streaks::status() else {
            return;
        };
        let file = path
            .file_name()
            .map_or(name.clone(), |f| f.to_string_lossy().into_owned());
        match status {
            Status::Loading => {
                let text = format!("Finding the highlights in {file}... {percent}%");
                show(vgui, page, build, (&text, &text), Some(percent));
                fill(
                    list,
                    base,
                    build,
                    &format!("{generation}:loading"),
                    &[],
                    false,
                );
                show_filters(vgui, page, None);
            }
            Status::Ready(found) => {
                let text = ready_text(&found, &file);
                show(vgui, page, build, (&text, &text), None);
                fill(
                    list,
                    base,
                    build,
                    &format!("{generation}:ready"),
                    &found.streaks,
                    found.hltv,
                );
                show_filters(vgui, page, Some(found.hltv));
                narrow(vgui, page, list, found.hltv);
            }
            Status::Failed(why) => {
                let text = format!("Could not find the highlights in {file}: {why}");
                show(vgui, page, build, (&text, &text), None);
                fill(
                    list,
                    base,
                    build,
                    &format!("{generation}:failed"),
                    &[],
                    false,
                );
                show_filters(vgui, page, None);
            }
        }
    }
}

/// The status line once the streaks are found.
fn ready_text(found: &Found, file: &str) -> String {
    let n = found.streaks.len();
    let lead = streaks::LEAD_IN_SECS;
    match (&found.recorder, n) {
        (Some(recorder), 0) => format!("No kills by {recorder} in {file}."),
        (Some(recorder), _) => format!(
            "{n} highlights by {recorder} in {file}. Go jumps to {lead} s before the first kill."
        ),
        (None, 0) => format!("No kills in {file}."),
        (None, _) if found.hltv => {
            format!("{n} highlights in {file}. Go jumps to {lead} s before and watches the player.")
        }
        (None, _) => {
            format!("{n} highlights in {file}. Go jumps to {lead} s before the first kill.")
        }
    }
}

/// The filter boxes: none while nothing is listed (`None`), Min kills once
/// the streaks are found, and Player too for an HLTV demo (`Some(true)`).
unsafe fn show_filters(vgui: &Vgui, page: Vpanel, hltv: Option<bool>) {
    unsafe {
        let shown = [
            (PLAYER_BOX, hltv == Some(true)),
            (PLAYER_LABEL, hltv == Some(true)),
            (MIN_KILLS_BOX, hltv.is_some()),
            (MIN_KILLS_LABEL, hltv.is_some()),
        ];
        for (name, on) in shown {
            if let Some(vp) = vgui.child_named(page, name)
                && vgui.visible(vp) != on
            {
                vgui.set_visible(vp, on);
            }
        }
    }
}

/// Shows only the rows whose player matches the Player box (every word, case
/// ignored) with at least the Min kills box's kills, when either box changed
/// or the list was refilled.
unsafe fn narrow(vgui: &Vgui, page: Vpanel, list: *mut c_void, hltv: bool) {
    unsafe {
        let player = if hltv {
            box_text(vgui, page, PLAYER_BOX).trim().to_string()
        } else {
            String::new()
        };
        let min_kills: usize = box_text(vgui, page, MIN_KILLS_BOX)
            .trim()
            .parse()
            .unwrap_or(0);
        let wanted = format!("{player}|{min_kills}");
        let mut filtered = FILTERED.lock().unwrap_or_else(|e| e.into_inner());
        if filtered.as_deref() == Some(wanted.as_str()) {
            return;
        }
        let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
        let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
        let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
        let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
        let set_visible: ListSetVisibleFn = slot(list, LIST_SLOT_SET_ITEM_VISIBLE);
        let mut id = first(list);
        let mut guard = 0;
        while is_valid(list, id) & 0xff != 0 && guard < 100_000 {
            guard += 1;
            let row = get_item(list, id);
            if !row.is_null() {
                let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
                let get = |key: &CStr| {
                    let raw = get_string(row, key.as_ptr(), c"".as_ptr());
                    if raw.is_null() {
                        String::new()
                    } else {
                        text(raw)
                    }
                };
                let kills: usize = get(COLUMNS[1].0).trim().parse().unwrap_or(0);
                let shown = matches_filter(&get(COLUMNS[0].0), &player) && kills >= min_kills;
                set_visible(list, id, shown as u32);
            }
            id = next(list, id);
        }
        *filtered = Some(wanted);
    }
}

/// Sets the status line (when `key` changed) and the progress bar: shown
/// with `percent`, hidden without.
unsafe fn show(
    vgui: &Vgui,
    page: Vpanel,
    build: &Build,
    (key, text): (&str, &str),
    percent: Option<u32>,
) {
    unsafe {
        let mut shown = SHOWN_STATUS.lock().unwrap_or_else(|e| e.into_inner());
        if *shown == key {
            return;
        }
        if let Some(label) = vgui.child_named(page, STATUS_LABEL) {
            set_label(vgui.object(label), text);
        }
        if let Some(bar) = vgui.child_named(page, PROGRESS) {
            vgui.set_visible(bar, percent.is_some());
            let object = vgui.object(bar);
            let Ok((base, _)) = gameui() else { return };
            if let Some(percent) = percent
                && !object.is_null()
                && *(object as *const usize) == base + build.progress_bar_vftable
            {
                let set_progress: SetProgressFn = slot(object, PROGRESS_BAR_SLOT_SET_PROGRESS);
                set_progress(object, percent as f32 / 100.0);
            }
        }
        *shown = key.to_string();
    }
}

/// Lists `streaks`, when `key` (which list it is) changed.
unsafe fn fill(
    list: *mut c_void,
    base: usize,
    build: &Build,
    key: &str,
    streaks: &[Streak],
    hltv: bool,
) {
    unsafe {
        let mut shown = SHOWN_ROWS.lock().unwrap_or_else(|e| e.into_inner());
        if *shown == key {
            return;
        }
        *shown = key.to_string();
        // Every row shows again: narrow it afresh.
        *FILTERED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let delete_all: ListVoidFn = slot(list, LIST_SLOT_DELETE_ALL_ITEMS);
        delete_all(list);
        // One player in a POV demo: no Player column.
        let column_visible: ListIntBoolFn = slot(list, LIST_SLOT_SET_COLUMN_VISIBLE);
        column_visible(list, PLAYER_COLUMN, hltv as u32);
        let new: OperatorNewFn = std::mem::transmute(base + build.keyvalues_new);
        let ctor: KeyValuesCtor = std::mem::transmute(base + build.keyvalues_ctor);
        let add_item: AddItemFn = slot(list, LIST_SLOT_ADD_ITEM);
        for (index, streak) in streaks.iter().enumerate() {
            let target = match (hltv, streak.player_number) {
                (true, Some(number)) => number.to_string(),
                _ => String::new(),
            };
            let cells = [
                (COLUMNS[0].0, streak.player.replace('\0', "")),
                (COLUMNS[1].0, streaks::kills_text(streak.kills)),
                (COLUMNS[2].0, streaks::time_text(streak.first_kill)),
                (COLUMNS[3].0, streak.duration_text()),
                (COLUMNS[4].0, streak.details.clone()),
                (SEEK_KEY, format!("{:.2}", streak.seek_secs())),
                (TARGET_KEY, target),
            ];
            let Ok(row_number) = CString::new(streaks::row_text(index + 1)) else {
                continue;
            };
            let row = new(KEYVALUES_SIZE);
            if row.is_null() {
                continue;
            }
            ctor(row, c"data".as_ptr(), ROW_KEY.as_ptr(), row_number.as_ptr());
            let set_string: SetStringFn = slot(row, KEYVALUES_SLOT_SET_STRING);
            for (cell, value) in cells {
                if let Ok(value) = CString::new(value) {
                    set_string(row, cell.as_ptr(), value.as_ptr());
                }
            }
            // The list keeps its own copy; `row` stays ours, and is left
            // (a few dozen bytes a row, once per demo) rather than freed
            // through a slot nothing here has verified.
            add_item(list, row, 0, 0, 0);
        }
        if !streaks.is_empty() {
            // What the list holds now, counted back through it.
            let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
            let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
            let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
            let (mut id, mut rows) = (first(list), 0);
            while is_valid(list, id) & 0xff != 0 && rows < 100_000 {
                rows += 1;
                id = next(list, id);
            }
            crate::debug::report(&format!(
                "studio_panel: Highlights tab lists {rows} of {} streaks",
                streaks.len()
            ));
        }
    }
}

unsafe fn set_label(object: *mut c_void, text: &str) {
    if object.is_null() {
        return;
    }
    if let Ok(text) = CString::new(text) {
        unsafe {
            let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
            set_text(object, text.as_ptr());
        }
    }
}

/// The hidden window's `OnCommand`: Load (the Go button, or a double-click)
/// seeks to the selected streak, and in an HLTV demo puts the camera on its
/// player.
unsafe extern "thiscall" fn on_command(this: *mut c_void, raw: *const c_char) {
    if text(raw).eq_ignore_ascii_case("load") {
        match unsafe { selected_value(this, SEEK_KEY) } {
            Some(secs) => {
                let target = unsafe { selected_value(this, TARGET_KEY) };
                let line = go_line(&secs, target.as_deref());
                if !CString::new(line).is_ok_and(|l| crate::engine::client_cmd(&l)) {
                    crate::commands::console_print(&format!("{NAME}: could not run the seek\n"));
                }
            }
            None => crate::commands::console_print(&format!(
                "{NAME}: pick a streak in the list first\n"
            )),
        }
        return;
    }
    let original = ON_COMMAND.load(Ordering::Acquire);
    if original != 0 {
        // Safety: the window's own OnCommand, from its vftable.
        let original: OnCommandFn = unsafe { std::mem::transmute(original) };
        unsafe { original(this, raw) };
    }
}

/// What Go runs: the seek, then the camera onto the streak's player when the
/// row names one. The camera command rides on the seek and runs once it has
/// landed: a long seek steps there over a few hundred frames, and the player
/// may only exist once it has caught up (#596).
fn go_line(secs: &str, target: Option<&str>) -> String {
    match target.filter(|t| !t.is_empty()) {
        Some(number) => format!(
            "{} {secs} {} {number}\n",
            crate::demo_seek::SEEK_TO_NAME,
            crate::spectator_follow::TARGET_NAME
        ),
        None => format!("{} {secs}\n", crate::demo_seek::SEEK_TO_NAME),
    }
}
