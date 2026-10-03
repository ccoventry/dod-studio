//! The Demos tab's Player box as a searchable dropdown (#565): an editable
//! `ComboBox` whose list holds every player in the listed demos, narrowed to
//! the names matching what is typed, and opened as you type.
//!
//! The game's `ComboBox` doesn't narrow its own list, so its items are
//! rebuilt whenever the text changes: `DeleteAllItems` (slot
//! [`COMBO_SLOT_DELETE_ALL_ITEMS`], which hands on to the drop-down menu's
//! own), then `AddItem` per name. The list opens through the box's own
//! `OnCommand("ButtonClicked")`, what its arrow button sends, which toggles
//! it; so it is only sent while the list is closed.
//!
//! Picking a name sets the box's text to it, which the Player filter reads
//! like anything typed; a text that is exactly a name doesn't reopen it.

use std::ffi::{CString, c_void};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use super::*;

/// The most names the list shows at once.
const MAX_SHOWN: usize = 40;

/// Every player name in the listed demos, with what it was built from (the
/// players files' generation and the list's row count).
type Names = Option<((u64, usize), Vec<String>)>;
static NAMES: Mutex<Names> = Mutex::new(None);
/// The text the list was last built for.
static BUILT_FOR: Mutex<Option<String>> = Mutex::new(None);

/// Distinct player names in the demos `list` holds, sorted, case ignored.
unsafe fn names_in(list: *mut c_void) -> Vec<String> {
    unsafe {
        let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
        let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
        let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
        let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
        let mut names: Vec<String> = Vec::new();
        let mut id = first(list);
        let mut guard = 0;
        while is_valid(list, id) & 0xff != 0 && guard < 100_000 {
            guard += 1;
            let row = get_item(list, id);
            if !row.is_null() {
                let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
                let raw = get_string(row, ROW_KEY.as_ptr(), c"".as_ptr());
                let name = if raw.is_null() {
                    String::new()
                } else {
                    text(raw)
                };
                if let Some(players) =
                    row_path(&name).and_then(|path| crate::demo_rosters::players_for(&path))
                {
                    names.extend(
                        players
                            .players
                            .into_iter()
                            .map(|p| p.name.replace('\0', "").trim().to_string())
                            .filter(|n| !n.is_empty()),
                    );
                }
            }
            id = next(list, id);
        }
        names.sort_by_key(|n| n.to_ascii_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        names
    }
}

/// The names to offer for `typed`: every word in the name, case ignored.
fn matching<'a>(names: &'a [String], typed: &str) -> Vec<&'a String> {
    let words: Vec<String> = typed
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    names
        .iter()
        .filter(|n| {
            let lower = n.to_ascii_lowercase();
            words.iter().all(|w| lower.contains(w))
        })
        .take(MAX_SHOWN)
        .collect()
}

/// Runs every frame while the window shows.
pub(super) unsafe fn update(vgui: &Vgui) {
    unsafe {
        let page = vpanel_of(PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void);
        if page == 0 || !vgui.visible(page) {
            return;
        }
        let Ok((base, build)) = gameui() else { return };
        let Some(combo) = vgui
            .child_named(page, PLAYER_FILTER)
            .map(|vp| vgui.object(vp))
            .filter(|o| !o.is_null() && *(*o as *const usize) == base + build.combo_box_vftable)
        else {
            return;
        };
        let dialog = DEMO_DIALOG.load(Ordering::Acquire) as *mut c_void;
        if dialog.is_null() {
            return;
        }
        let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
        if list.is_null() {
            return;
        }

        // The names, again when players files arrive or the list is refilled.
        let key = (
            crate::demo_rosters::generation(),
            DEMO_LIST_STAMPS.load(Ordering::Acquire),
        );
        let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = names.as_ref().is_none_or(|(k, _)| *k != key);
        if fresh {
            crate::demo_rosters::ensure_filled();
            *names = Some((key, names_in(list)));
        }
        let Some((_, all)) = names.as_ref() else {
            return;
        };

        let typed = box_text(vgui, page, PLAYER_FILTER);
        let mut built = BUILT_FOR.lock().unwrap_or_else(|e| e.into_inner());
        if !fresh && built.as_deref() == Some(typed.as_str()) {
            return;
        }
        let first_build = built.is_none();
        *built = Some(typed.clone());

        let delete_all: ListVoidFn = slot(combo, COMBO_SLOT_DELETE_ALL_ITEMS);
        let add_item: ComboAddItemFn = slot(combo, COMBO_SLOT_ADD_ITEM);
        delete_all(combo);
        let shown = matching(all, &typed);
        for name in &shown {
            if let Ok(c) = CString::new(name.as_str()) {
                add_item(combo, c.as_ptr(), std::ptr::null());
            }
        }

        // Open the list while typing: not for the first build, an empty box,
        // no match, or a name picked from the list.
        let picked = all.iter().any(|n| n.eq_ignore_ascii_case(typed.trim()));
        if first_build || fresh || typed.trim().is_empty() || shown.is_empty() || picked {
            return;
        }
        let menu = *((combo as *const u8).add(build.combo_menu) as *const *mut c_void);
        if menu.is_null() {
            return;
        }
        let on_command: OnCommandFn = slot(combo, FRAME_SLOT_ON_COMMAND);
        if vgui.visible(vpanel_of(menu)) {
            // Close and reopen, so the list is sized and placed for its
            // new length.
            on_command(combo, BUTTON_CLICKED.as_ptr());
        }
        on_command(combo, BUTTON_CLICKED.as_ptr());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_typed_word_narrows_the_names() {
        let names: Vec<String> = ["dyelife", "m00cat <3", "Candyman", "gorilla[bc]"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let got: Vec<&String> = matching(&names, "CAT");
        assert_eq!(got, vec!["m00cat <3"]);
        assert_eq!(matching(&names, "").len(), 4);
        assert_eq!(matching(&names, "y man").len(), 1);
        assert!(matching(&names, "nobody").is_empty());
    }
}
