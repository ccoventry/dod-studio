//! The Killstreaks tab (#565): the playing demo's streaks, found by
//! [`crate::streaks`], in a list borrowed the way the Demos tab's is. A
//! second hidden Load Demo window (`STREAK_LIST`) lends its list and Load
//! button ([`LOANS`]); its demo rows are cleared, its columns become Player,
//! Kills, Weapons and Time, its Load button is Go, and its `OnCommand` is
//! ours: Go, or a double-click, seeks to just before the streak.

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::streaks::{self, Status, Streak};

static DIALOG: AtomicUsize = AtomicUsize::new(0);
static ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
/// What the status line and the list show now, so each is redrawn only when
/// it changes.
static SHOWN_STATUS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static SHOWN_ROWS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// The row key Go reads: the bar time to seek to.
const SEEK_KEY: &CStr = c"seek";
/// The columns past the player's name (which is the window's own `demoname`
/// column, retitled): key, heading, width.
const COLUMNS: [(&CStr, &CStr, i32); 3] = [
    (c"kills", c"Kills", 50),
    (c"weapons", c"Weapons", 240),
    (c"time", c"Time", 60),
];
const PLAYER_COLUMN_WIDE: i32 = 150;
const STATUS_LABEL: &str = "StreakStatus";
const PROGRESS: &str = "StreakProgress";

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
            vgui.place(heading, (x, y, PLAYER_COLUMN_WIDE, tall));
            set_label(vgui.object(heading), "Player");
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
            fill(list, base, build, "none", &[]);
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
                let text = format!("Finding the killstreaks in {file}... {percent}%");
                show(vgui, page, build, (&text, &text), Some(percent));
                fill(list, base, build, &format!("{generation}:loading"), &[]);
            }
            Status::Ready(streaks) => {
                let text = if streaks.is_empty() {
                    format!("No streaks of {}+ kills in {file}.", streaks::MIN_KILLS)
                } else {
                    format!(
                        "{} streaks of {}+ kills in {file}. Go jumps to {} s before the first kill.",
                        streaks.len(),
                        streaks::MIN_KILLS,
                        streaks::LEAD_IN_SECS
                    )
                };
                show(vgui, page, build, (&text, &text), None);
                fill(list, base, build, &format!("{generation}:ready"), &streaks);
            }
            Status::Failed(why) => {
                let text = format!("Could not find the killstreaks in {file}: {why}");
                show(vgui, page, build, (&text, &text), None);
                fill(list, base, build, &format!("{generation}:failed"), &[]);
            }
        }
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
unsafe fn fill(list: *mut c_void, base: usize, build: &Build, key: &str, streaks: &[Streak]) {
    unsafe {
        let mut shown = SHOWN_ROWS.lock().unwrap_or_else(|e| e.into_inner());
        if *shown == key {
            return;
        }
        *shown = key.to_string();
        let delete_all: ListVoidFn = slot(list, LIST_SLOT_DELETE_ALL_ITEMS);
        delete_all(list);
        let new: OperatorNewFn = std::mem::transmute(base + build.keyvalues_new);
        let ctor: KeyValuesCtor = std::mem::transmute(base + build.keyvalues_ctor);
        let add_item: AddItemFn = slot(list, LIST_SLOT_ADD_ITEM);
        for streak in streaks {
            let cells = [
                (COLUMNS[0].0, streaks::kills_text(streak.kills)),
                (COLUMNS[1].0, streak.weapons.clone()),
                (COLUMNS[2].0, streaks::time_text(streak.first_kill)),
                (SEEK_KEY, format!("{:.2}", streak.seek_secs())),
            ];
            let Ok(player) = CString::new(streak.player.replace('\0', "")) else {
                continue;
            };
            let row = new(KEYVALUES_SIZE);
            if row.is_null() {
                continue;
            }
            ctor(row, c"data".as_ptr(), ROW_KEY.as_ptr(), player.as_ptr());
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
/// seeks to the selected streak.
unsafe extern "thiscall" fn on_command(this: *mut c_void, raw: *const c_char) {
    if text(raw).eq_ignore_ascii_case("load") {
        match unsafe { selected_value(this, SEEK_KEY) } {
            Some(secs) => {
                let line = format!("{} {secs}\n", crate::demo_seek::SEEK_TO_NAME);
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
