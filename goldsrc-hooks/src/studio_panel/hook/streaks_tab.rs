//! The Highlights tab (#565): the playing demo's streaks, found by
//! [`crate::streaks`], in a list borrowed the way the Demos tab's is. A
//! second hidden Load Demo window (`STREAK_LIST`) lends its list and Load
//! button ([`LOANS`]); its demo rows are cleared, its columns become Row #,
//! Player, Kills, Time, Dur. and Details, its Load button is Go, and its `OnCommand` is
//! ours: Go, or a double-click, seeks to just before the streak.
//!
//! Every life with a kill is listed. A POV demo lists only the recording
//! player's (the footage follows no one else), named in a greyed-out Player
//! box. An HLTV demo lists everyone's, the Player box narrows them, and Go
//! also puts the camera on the streak's player (`dodstudio_spec_target`).
//! The hint line under the list says when there is nothing to list, and why;
//! how far the analysis has got is the loading strip's and the progress bar's.

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::streaks::{self, Found, Status, Streak};

static DIALOG: AtomicUsize = AtomicUsize::new(0);

/// The hidden Load Demo window lending the list, or 0 before the tab is built.
pub(super) fn dialog() -> usize {
    DIALOG.load(Ordering::Acquire)
}
static ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
/// What the hint line and the list show now, so each is redrawn only when
/// it changes.
static SHOWN_STATUS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static SHOWN_ROWS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// The list the Player box was last set for, so it is set once per demo and
/// then left to what is typed.
static BOXED_FOR: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

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
/// The line under the list, beside Go (`Highlights.res`).
const HINT_LABEL: &str = "Hint";
const PROGRESS: &str = "StreakProgress";
/// The Player box, and its label: shown once the streaks are found.
const PLAYER_BOX: &str = "StreakPlayer";
const PLAYER_LABEL: &str = "StreakPlayerLabel";

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
        BOXED_FOR.lock().unwrap_or_else(|e| e.into_inner()).clear();

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

/// Keeps the tab showing the playing demo's streaks. Runs every frame while
/// the tab shows; the analysis itself starts as soon as the demo plays
/// (`load_strip::request_highlights`, #797), so asking here again is a no-op.
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
            fill(list, base, build, "none", &[], false, "");
            show_filters(vgui, page, None);
            return;
        };
        let game_dir = dod_dir();
        let path = streaks::demo_path(&game_dir, &name);
        streaks::request(&path);
        let Some((generation, status, percent)) = streaks::status() else {
            return;
        };
        match status {
            Status::Loading => {
                show(
                    vgui,
                    page,
                    build,
                    (&format!("loading {percent}"), GO_HINT),
                    Some(percent),
                );
                fill(
                    list,
                    base,
                    build,
                    &format!("{generation}:loading"),
                    &[],
                    false,
                    "",
                );
                show_filters(vgui, page, None);
            }
            Status::Ready(found) => {
                let text = ready_hint(&found);
                show(vgui, page, build, (&text, &text), None);
                let demo_key = format!("{generation}:ready");
                {
                    let mut boxed = BOXED_FOR.lock().unwrap_or_else(|e| e.into_inner());
                    if *boxed != demo_key {
                        fill_player_box(vgui, page, &found);
                        *boxed = demo_key.clone();
                    }
                }
                // A POV demo's box names its one player: every row shows.
                let player = if found.hltv {
                    box_text(vgui, page, PLAYER_BOX).trim().to_string()
                } else {
                    String::new()
                };
                fill(
                    list,
                    base,
                    build,
                    &format!("{demo_key}|{player}"),
                    &found.streaks,
                    found.hltv,
                    &player,
                );
                show_filters(vgui, page, Some(found.hltv));
            }
            Status::Failed(why) => {
                let text = format!("Could not find the highlights: {why}");
                show(vgui, page, build, (&text, &text), None);
                fill(
                    list,
                    base,
                    build,
                    &format!("{generation}:failed"),
                    &[],
                    false,
                    "",
                );
                show_filters(vgui, page, None);
            }
        }
    }
}

/// The hint under the list while the streaks are found or loading: what Go
/// does.
const GO_HINT: &str = "Double-click a streak, or pick one and Go.";

/// The hint under the list once the streaks are found: what Go does, or that
/// there is nothing to go to.
fn ready_hint(found: &Found) -> String {
    match (&found.recorder, found.streaks.is_empty()) {
        (Some(recorder), true) => format!("No kills by {recorder} in this demo."),
        (None, true) => "No kills in this demo.".to_string(),
        (_, false) if found.hltv => {
            "Double-click a streak, or pick one and Go: the camera follows its player.".to_string()
        }
        (_, false) => GO_HINT.to_string(),
    }
}

/// The Player box for a newly listed demo: empty and editable in an HLTV
/// demo, the recording player's name and greyed out in a POV demo, which
/// lists no one else.
unsafe fn fill_player_box(vgui: &Vgui, page: Vpanel, found: &Found) {
    unsafe {
        let Some(entry) = vgui
            .child_named(page, PLAYER_BOX)
            .map(|vp| vgui.object(vp))
            .filter(|o| !o.is_null())
        else {
            return;
        };
        let name = match (&found.recorder, found.hltv) {
            (Some(recorder), false) => recorder.replace('\0', ""),
            _ => String::new(),
        };
        if let Ok(name) = CString::new(name) {
            let set_text: SetTextFn = slot(entry, TEXT_ENTRY_SLOT_SET_TEXT);
            set_text(entry, name.as_ptr());
        }
        let set_enabled: ListIntVoidFn = slot(entry, PANEL_SLOT_SET_ENABLED);
        set_enabled(entry, i32::from(found.hltv));
    }
}

/// The Player box and its label: hidden while nothing is listed (`None`),
/// shown once the streaks are found.
unsafe fn show_filters(vgui: &Vgui, page: Vpanel, hltv: Option<bool>) {
    unsafe {
        for name in [PLAYER_BOX, PLAYER_LABEL] {
            if let Some(vp) = vgui.child_named(page, name)
                && vgui.visible(vp) != hltv.is_some()
            {
                vgui.set_visible(vp, hltv.is_some());
            }
        }
    }
}

/// Sets the hint line (when `key` changed) and the progress bar: shown with
/// `percent`, hidden without.
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
        if let Some(label) = vgui.child_named(page, HINT_LABEL) {
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

/// Lists the `streaks` whose player matches `player` (every word, case
/// ignored; "" matches all), when `key` (which list, and which filter)
/// changed. Each keeps its Row # from the whole list. Typing in the Player
/// box lists the rows again rather than hiding the others: a row
/// `SetItemVisible` shows again goes to the end of the list, out of order.
unsafe fn fill(
    list: *mut c_void,
    base: usize,
    build: &Build,
    key: &str,
    streaks: &[Streak],
    hltv: bool,
    player: &str,
) {
    unsafe {
        let mut shown = SHOWN_ROWS.lock().unwrap_or_else(|e| e.into_inner());
        if *shown == key {
            return;
        }
        *shown = key.to_string();
        let delete_all: ListVoidFn = slot(list, LIST_SLOT_DELETE_ALL_ITEMS);
        delete_all(list);
        // One player in a POV demo: no Player column.
        let column_visible: ListIntBoolFn = slot(list, LIST_SLOT_SET_COLUMN_VISIBLE);
        column_visible(list, PLAYER_COLUMN, hltv as u32);
        let new: OperatorNewFn = std::mem::transmute(base + build.keyvalues_new);
        let ctor: KeyValuesCtor = std::mem::transmute(base + build.keyvalues_ctor);
        let add_item: AddItemFn = slot(list, LIST_SLOT_ADD_ITEM);
        for (index, streak) in streaks.iter().enumerate() {
            if !matches_filter(&streak.player, player) {
                continue;
            }
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
            // (a few hundred bytes a row, per demo and per change to the
            // Player box) rather than freed
            // through a slot nothing here has verified.
            add_item(list, row, 0, 0, 0);
        }
        // A list scrolled down for the last demo showed nothing for one with
        // fewer highlights until the next scroll (#796). Nothing is sorted
        // unless a heading was clicked: the rows are in highlight order.
        crate::list_panel::after_refill(list, build.list_sort_column, None);
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
        // Nothing picked: Enter anywhere on the tab lands here too (the
        // window's default button), so no console line then (#777); the
        // hint under the list already says what to do.
        if let Some(secs) = unsafe { selected_value(this, SEEK_KEY) } {
            let target = unsafe { selected_value(this, TARGET_KEY) };
            let line = go_line(&secs, target.as_deref());
            if !CString::new(line).is_ok_and(|l| crate::engine::client_cmd(&l)) {
                crate::commands::console_print(&format!("{NAME}: could not run the seek\n"));
            }
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
