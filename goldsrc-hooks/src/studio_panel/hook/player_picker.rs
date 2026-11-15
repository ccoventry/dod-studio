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
//! Picking a name adds it to the picked players, shown on the line beside
//! the box, and empties the box for the next; the Player filter reads only
//! the picked players, so what is typed searches the list, not the demos.
//! A picked player's row comes first in the list, marked `[x] `, and
//! clicking it takes them off again.
//!
//! One entry per player, not per name (#579): everyone with a SteamID is
//! listed once, by the name they used in the most demos, with their other
//! names after it -- `dyelife (also: dyeL!fe[dd])`. Picking that entry
//! filters by the SteamID, which [`crate::demo_rosters::has_player`] matches
//! whatever name was used. A player with no SteamID (a demo without `*sid`,
//! LAN) stays one entry per name.
//!
//! An entry shows at most [`MAX_ALSO`] other names, so the open list is
//! widened to fit its longest entry (as far as the tab has room), and the
//! window's help line spells out every name of the entry under the mouse
//! (#781).

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Mutex;

use analysis::cache::DemoPlayers;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use super::*;

/// The most names the list shows at once.
const MAX_SHOWN: usize = 40;
/// What a picked player's row in the list starts with: clicking it takes
/// them off the picked line again. Plain ASCII, which every font has.
const REMOVE: &str = "[x] ";

/// The most other names an entry shows after its main one.
const MAX_ALSO: usize = 3;

/// One entry in the Player list.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    /// What the list shows.
    label: String,
    /// What the picked line shows: the main name.
    name: String,
    /// Every other name, most used first: what the help line spells out.
    others: Vec<String>,
    /// What picking it adds to the filter: the SteamID64, or the name for a
    /// player with none.
    term: String,
    /// Every name and the SteamID, lowercased, for what is typed.
    search: String,
}

/// Every player in the listed demos, with what it was built from (the
/// players files' generation and the list's row count).
type Names = Option<((u64, usize), Vec<Choice>)>;
static NAMES: Mutex<Names> = Mutex::new(None);
/// The text the list was last built for.
static BUILT_FOR: Mutex<Option<String>> = Mutex::new(None);
/// Whether the list was open last frame: a name clicked in it closes it.
static WAS_OPEN: AtomicBool = AtomicBool::new(false);
/// What the picked-players line says now.
static CHOSEN_SHOWN: Mutex<Option<String>> = Mutex::new(None);
/// The text of the list row under the mouse while the list is open.
static HOVERED: Mutex<Option<String>> = Mutex::new(None);

/// How tall the open list's rows were when last measured (0: not yet).
static ROW_TALL: AtomicI32 = AtomicI32::new(0);

/// Roughly how wide a character of the window's font is, in pixels: what
/// the picked-players line is cut to fit by.
const CHAR_WIDE: i32 = 7;

/// How tall a list row is where [`CHAR_WIDE`] and `place_menu_width`'s six
/// pixels a character hold: the pre-Anniversary GameUI, whose font doesn't
/// scale. The 25th Anniversary GameUI scales its font and rows with the
/// screen (44 px rows at 2560x1440, where the list cut its entries short and
/// the help line ran off the window), so the open list's widths are scaled
/// by its rows' height (#781).
const UNSCALED_ROW_TALL: i32 = 22;

/// The picked players as one line of at most `room` characters: as many
/// names as fit, then how many more there are.
fn fit_names(picked: &[String], room: usize) -> String {
    if picked.is_empty() {
        return "none: pick names from the Player list".to_string();
    }
    let all = picked.join(", ");
    if all.chars().count() <= room {
        return all;
    }
    let mut line = String::new();
    for (shown, name) in picked.iter().enumerate() {
        let more = format!(" +{} more", picked.len() - shown);
        let next = if line.is_empty() {
            name.clone()
        } else {
            format!("{line}, {name}")
        };
        let rest = picked.len() - shown - 1;
        let tail = if rest > 0 {
            format!(" +{rest} more").chars().count()
        } else {
            0
        };
        if next.chars().count() + tail > room {
            return if line.is_empty() {
                format!("{} picked", picked.len())
            } else {
                format!("{line}{more}")
            };
        }
        line = next;
    }
    line
}

/// Every name of one entry as one line of at most `room` characters: as
/// many of the other names as fit, then how many more there are.
fn names_help(choice: &Choice, room: usize) -> String {
    if choice.others.is_empty() {
        return format!("{}: no other names in these demos", choice.name);
    }
    let head = format!("{}, also: ", choice.name);
    let mut line = head.clone();
    for (i, other) in choice.others.iter().enumerate() {
        let next = if i == 0 {
            format!("{line}{other}")
        } else {
            format!("{line}, {other}")
        };
        let rest = choice.others.len() - i - 1;
        let tail = if rest > 0 {
            format!(" +{rest} more").chars().count()
        } else {
            0
        };
        if next.chars().count() + tail > room {
            let left = choice.others.len() - i;
            return if i == 0 {
                format!("{head}{left} other names")
            } else {
                format!("{line} +{left} more")
            };
        }
        line = next;
    }
    line
}

/// The entry a list row's text belongs to: its label, or a picked player's
/// `[x] ` row by their main name. A player with no SteamID shares a main
/// name with the SteamID player whose name they used, so a `[x] ` row looks
/// among the `picked` terms first.
fn entry_for<'a>(choices: &'a [Choice], picked: &[String], row: &str) -> Option<&'a Choice> {
    let row = row.trim();
    match row.strip_prefix(REMOVE.trim_end()) {
        Some(name) => {
            let named = |c: &&Choice| c.name.eq_ignore_ascii_case(name.trim());
            choices
                .iter()
                .filter(named)
                .find(|c| picked.iter().any(|p| p.eq_ignore_ascii_case(&c.term)))
                .or_else(|| choices.iter().find(named))
        }
        None => choices.iter().find(|c| c.label.eq_ignore_ascii_case(row)),
    }
}

/// While the list is open with the mouse over an entry: every name of that
/// player, cut to fit a line `wide` pixels wide. For the window's help line.
pub(super) fn hovered_help(wide: i32) -> Option<String> {
    let row = HOVERED.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let picked = PICKED_PLAYERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let (_, all) = names.as_ref()?;
    let choice = entry_for(all, &picked, &row)?;
    let room = help_room(wide, ROW_TALL.load(Ordering::Acquire));
    Some(names_help(choice, room))
}

/// How many characters fit the help line `wide` pixels wide, when the open
/// list's rows are `row_tall` tall.
fn help_room(wide: i32, row_tall: i32) -> usize {
    ((wide - 8) / scaled(CHAR_WIDE, row_tall).max(1)).max(20) as usize
}

/// `px`, a width counted for the unscaled font, at a font whose list rows
/// are `row_tall` tall (0: not measured yet, left as it is).
fn scaled(px: i32, row_tall: i32) -> i32 {
    if row_tall <= 0 {
        return px;
    }
    px.saturating_mul(row_tall) / UNSCALED_ROW_TALL
}

/// How wide the open list is for its longest label of `longest` characters
/// at rows `row_tall` tall: wide enough for the label and the scroll bar, no
/// wider than the tab's `room` past the box, never narrower than the box.
fn list_width(longest: usize, box_wide: i32, room: i32, row_tall: i32) -> i32 {
    scaled(place_menu_width(longest, 0, i32::MAX), row_tall)
        .min(room)
        .max(box_wide)
}

/// How tall the open list's rows are: the first visible `MenuItem`'s height.
unsafe fn row_tall(vgui: &Vgui, menu: *mut c_void, item_vftable: usize) -> Option<i32> {
    unsafe {
        vgui.child_list(vpanel_of(menu))
            .into_iter()
            .filter(|&c| vgui.visible(c))
            .filter(|&c| {
                let item = vgui.object(c);
                !item.is_null() && *(item as *const usize) == item_vftable
            })
            .map(|c| vgui.rect(c).3)
            .find(|&tall| tall > 0)
    }
}

/// The text of the list row under the mouse, if it is one of the menu's
/// items (a `MenuItem`, not its scroll bar).
unsafe fn row_under_mouse(vgui: &Vgui, menu: *mut c_void, item_vftable: usize) -> Option<String> {
    unsafe {
        let (mx, my) = cursor()?;
        let row = vgui
            .child_list(vpanel_of(menu))
            .into_iter()
            .filter(|&c| vgui.visible(c))
            .find(|&c| {
                let (ax, ay) = vgui.abs_pos(c);
                let (_, _, w, h) = vgui.rect(c);
                mx >= ax && mx < ax + w && my >= ay && my < ay + h
            })?;
        let item = vgui.object(row);
        if item.is_null() || *(item as *const usize) != item_vftable {
            return None;
        }
        let get_text: GetTextFn = slot(item, LABEL_SLOT_GET_TEXT);
        let mut buf = [0u8; 512];
        get_text(item, buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
        CStr::from_bytes_until_nul(&buf)
            .ok()
            .map(|s| s.to_string_lossy().into_owned())
    }
}

/// Widens the open list past the 170 px box to fit its longest entry, as far
/// as the tab has room right of the box. Every frame while open, since the
/// box narrows its menu to itself each time it opens (as the Go to list).
/// The text's width scales with the rows' height, since the 25th
/// Anniversary GameUI scales its font with the screen.
unsafe fn widen(
    vgui: &Vgui,
    page: Vpanel,
    combo: *mut c_void,
    menu: *mut c_void,
    item_vftable: usize,
    all: &[Choice],
) {
    unsafe {
        let longest = all
            .iter()
            .map(|c| c.label.chars().count())
            .max()
            .unwrap_or(0);
        if let Some(tall) = row_tall(vgui, menu, item_vftable) {
            ROW_TALL.store(tall, Ordering::Release);
        }
        let tall = ROW_TALL.load(Ordering::Acquire);
        let (x, _, w, _) = vgui.rect(vpanel_of(combo));
        let (_, _, page_w, _) = vgui.rect(page);
        let set_width: ListIntVoidFn = slot(menu, MENU_SLOT_SET_FIXED_WIDTH);
        set_width(menu, list_width(longest, w, page_w - x - 8, tall));
    }
}

/// What the picked line calls a picked term: its player's main name.
fn shown_name(choices: &[Choice], term: &str) -> String {
    choices
        .iter()
        .find(|c| c.term.eq_ignore_ascii_case(term))
        .map_or_else(|| term.to_string(), |c| c.name.clone())
}

/// The picked-players line beside the box, cut to fit its width.
unsafe fn show_picked(vgui: &Vgui, page: Vpanel, choices: &[Choice]) {
    unsafe {
        let picked: Vec<String> = PICKED_PLAYERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|t| shown_name(choices, t))
            .collect();
        let Some(vp) = vgui.child_named(page, PLAYER_CHOSEN) else {
            return;
        };
        let (_, _, wide, _) = vgui.rect(vp);
        let room = ((wide - 8) / CHAR_WIDE).max(8) as usize;
        let line = fit_names(&picked, room);
        let mut shown = CHOSEN_SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        if shown.as_deref() == Some(line.as_str()) {
            return;
        }
        let label = vgui.object(vp);
        if label.is_null() {
            return;
        }
        if let Ok(c) = CString::new(line.replace('\0', "")) {
            let set_text: SetTextFn = slot(label, LABEL_SLOT_SET_TEXT);
            set_text(label, c.as_ptr());
            *shown = Some(line);
        }
    }
}

/// The Player list's entries for these demos' players, sorted by label.
fn choices(demos: &[DemoPlayers]) -> Vec<Choice> {
    // Per SteamID: each name, as first spelled, and how many demos used it.
    let mut by_id: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    let mut nameless: Vec<String> = Vec::new();
    for demo in demos {
        let mut counted: Vec<(String, String)> = Vec::new();
        for p in &demo.players {
            let name = p.name.replace('\0', "").trim().to_string();
            if name.is_empty() {
                continue;
            }
            let Some(id) = p.steam_id.as_ref().filter(|id| !id.is_empty()) else {
                nameless.push(name);
                continue;
            };
            let key = (id.clone(), name.to_ascii_lowercase());
            if counted.contains(&key) {
                continue;
            }
            counted.push(key);
            let names = by_id.entry(id.clone()).or_default();
            match names
                .iter_mut()
                .find(|(n, _)| n.eq_ignore_ascii_case(&name))
            {
                Some((_, count)) => *count += 1,
                None => names.push((name, 1)),
            }
        }
    }
    let mut out: Vec<Choice> = by_id
        .into_iter()
        .map(|(id, mut names)| {
            names.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()))
            });
            let name = names[0].0.clone();
            let others: Vec<&str> = names[1..].iter().map(|(n, _)| n.as_str()).collect();
            let label = match others.len() {
                0 => name.clone(),
                n if n <= MAX_ALSO => format!("{name} (also: {})", others.join(", ")),
                n => format!(
                    "{name} (also: {} +{} more)",
                    others[..MAX_ALSO].join(", "),
                    n - MAX_ALSO
                ),
            };
            let others: Vec<String> = others.iter().map(|n| n.to_string()).collect();
            let search = names
                .iter()
                .map(|(n, _)| n.as_str())
                .chain(std::iter::once(id.as_str()))
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            Choice {
                label,
                name,
                others,
                term: id,
                search,
            }
        })
        .collect();
    nameless.sort_by_key(|n| n.to_ascii_lowercase());
    nameless.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    for name in nameless {
        // A name someone with a SteamID also uses gets its own label, so
        // the two entries can be told apart (and picked apart).
        let taken = out.iter().any(|c| c.label.eq_ignore_ascii_case(&name));
        out.push(Choice {
            label: if taken {
                format!("{name} (no SteamID)")
            } else {
                name.clone()
            },
            search: name.to_ascii_lowercase(),
            term: name.clone(),
            others: Vec::new(),
            name,
        });
    }
    out.sort_by_key(|c| c.label.to_ascii_lowercase());
    out
}

/// The Player list's entries for the demos `list` holds.
unsafe fn names_in(list: *mut c_void) -> Vec<Choice> {
    unsafe {
        let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
        let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
        let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
        let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
        let mut demos: Vec<DemoPlayers> = Vec::new();
        let mut id = first(list);
        let mut guard = 0;
        while is_valid(list, id) & 0xff != 0 && guard < 100_000 {
            guard += 1;
            let row = get_item(list, id);
            if !row.is_null() {
                let name = row_path_text(row);
                if let Some(players) =
                    row_path(&name).and_then(|path| crate::demo_rosters::players_for(&path))
                {
                    demos.push(players);
                }
            }
            id = next(list, id);
        }
        choices(&demos)
    }
}

/// The entries to offer for `typed`: every word in one of the player's names
/// (or their SteamID), case ignored, and not one already picked.
fn matching<'a>(choices: &'a [Choice], typed: &str, picked: &[String]) -> Vec<&'a Choice> {
    let words: Vec<String> = typed
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    choices
        .iter()
        .filter(|c| !picked.iter().any(|p| p.eq_ignore_ascii_case(&c.term)))
        .filter(|c| words.iter().all(|w| c.search.contains(w)))
        .take(MAX_SHOWN)
        .collect()
}

/// Runs every frame while the window shows.
pub(super) unsafe fn update(vgui: &Vgui) {
    unsafe {
        *HOVERED.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
        show_picked(vgui, page, all);
        let menu = *((combo as *const u8).add(build.combo_menu) as *const *mut c_void);
        if menu.is_null() {
            return;
        }
        let open = vgui.visible(vpanel_of(menu));
        let was_open = WAS_OPEN.swap(open, Ordering::AcqRel);
        if open {
            let item_vftable = base + build.menu_item_vftable;
            widen(vgui, page, combo, menu, item_vftable, all);
            *HOVERED.lock().unwrap_or_else(|e| e.into_inner()) =
                row_under_mouse(vgui, menu, item_vftable);
        }
        // A name clicked in the list: the list closed itself and the box holds
        // that name. It joins the picked players and the box empties for the
        // next. (A name typed out in full is still just a search.)
        if was_open
            && !open
            && let Some(name) = typed.trim().strip_prefix(REMOVE.trim_end())
        {
            PICKED_PLAYERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|p| !shown_name(all, p).eq_ignore_ascii_case(name.trim()));
            let set_text: SetTextFn = slot(combo, TEXT_ENTRY_SLOT_SET_TEXT);
            set_text(combo, c"".as_ptr());
            return;
        }
        if was_open
            && !open
            && let Some(choice) = all
                .iter()
                .find(|c| c.label.eq_ignore_ascii_case(typed.trim()))
        {
            let mut picked = PICKED_PLAYERS.lock().unwrap_or_else(|e| e.into_inner());
            if !picked.iter().any(|p| p.eq_ignore_ascii_case(&choice.term)) {
                picked.push(choice.term.clone());
            }
            drop(picked);
            let set_text: SetTextFn = slot(combo, TEXT_ENTRY_SLOT_SET_TEXT);
            set_text(combo, c"".as_ptr());
            return;
        }
        // Built again for new text, and when the picked players change: a
        // picked name leaves the list, and Clear puts them all back.
        let picked = PICKED_PLAYERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let build_key = format!("{typed}\u{1}{}", picked.join("\u{1}"));
        let mut built = BUILT_FOR.lock().unwrap_or_else(|e| e.into_inner());
        if !fresh && built.as_deref() == Some(build_key.as_str()) {
            return;
        }
        let first_build = built.is_none();
        *built = Some(build_key);

        let delete_all: ListVoidFn = slot(combo, COMBO_SLOT_DELETE_ALL_ITEMS);
        let add_item: ComboAddItemFn = slot(combo, COMBO_SLOT_ADD_ITEM);
        delete_all(combo);
        let shown = matching(all, &typed, &picked);
        // The picked players first, matching what is typed, each marked: a
        // click takes one back off.
        let words: Vec<String> = typed
            .split_whitespace()
            .map(|w| w.to_ascii_lowercase())
            .collect();
        let picked_rows: Vec<String> = picked
            .iter()
            .map(|t| shown_name(all, t))
            .filter(|n| {
                let lower = n.to_ascii_lowercase();
                words.iter().all(|w| lower.contains(w))
            })
            .collect();
        for name in &picked_rows {
            if let Ok(c) = CString::new(format!("{REMOVE}{name}")) {
                add_item(combo, c.as_ptr(), std::ptr::null());
            }
        }
        for choice in &shown {
            if let Ok(c) = CString::new(choice.label.as_str()) {
                add_item(combo, c.as_ptr(), std::ptr::null());
            }
        }

        if first_build || fresh {
            return;
        }
        // Nothing to offer (an empty box, no match): close it. Through the
        // panel, not the box's own close, which selects all its text so the
        // next key replaces it.
        if typed.trim().is_empty() || shown.is_empty() && picked_rows.is_empty() {
            if open {
                vgui.set_visible(vpanel_of(menu), false);
                WAS_OPEN.store(false, Ordering::Release);
            }
            return;
        }
        // Already open: the rebuilt items show in it as they are.
        if open {
            return;
        }
        // The list must not take the keyboard: an open menu takes keys as
        // its own type-ahead (the next letter jumps to a row), so typing
        // would stop going into the box. It still takes clicks. The same as
        // the Console tab's completion list.
        let keyboard: PanelSetBoolFn = slot(vgui.panel, IPANEL_SET_KEYBOARD_INPUT_ENABLED);
        keyboard(vgui.panel, vpanel_of(menu), 0);
        let on_command: OnCommandFn = slot(combo, FRAME_SLOT_ON_COMMAND);
        on_command(combo, BUTTON_CLICKED.as_ptr());
        let focus: SetParentFn = slot(vgui.panel, IPANEL_REQUEST_FOCUS);
        focus(vgui.panel, vpanel_of(combo), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picked_line_fits_its_room() {
        let names: Vec<String> = ["dyelife", "m00cat", "Candyman", "gorilla"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(fit_names(&names, 100), "dyelife, m00cat, Candyman, gorilla");
        let cut = fit_names(&names, 26);
        assert_eq!(cut, "dyelife, m00cat +2 more");
        assert!(cut.chars().count() <= 26);
        assert_eq!(fit_names(&names, 5), "4 picked");
        assert!(fit_names(&[], 30).starts_with("none"));
    }

    fn demo(players: &[(&str, Option<&str>)]) -> DemoPlayers {
        DemoPlayers {
            demo_type: "HLTV".to_string(),
            players: players
                .iter()
                .map(|(name, id)| analysis::cache::DemoPlayer {
                    id: id.map_or_else(|| format!("PLAYER_{name}"), str::to_string),
                    steam_id: id.map(str::to_string),
                    name: name.to_string(),
                    recorder: false,
                })
                .collect(),
        }
    }

    fn labels(choices: &[Choice]) -> Vec<&str> {
        choices.iter().map(|c| c.label.as_str()).collect()
    }

    #[test]
    fn every_typed_word_narrows_the_names() {
        let all = choices(&[demo(&[
            ("dyelife", None),
            ("m00cat <3", None),
            ("Candyman", None),
            ("gorilla[bc]", None),
        ])]);
        let got: Vec<&str> = matching(&all, "CAT", &[])
            .iter()
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(got, vec!["m00cat <3"]);
        assert_eq!(matching(&all, "", &[]).len(), 4);
        assert_eq!(matching(&all, "y man", &[]).len(), 1);
        assert!(matching(&all, "nobody", &[]).is_empty());
        // A picked name is no longer offered.
        let picked = vec!["DYELIFE".to_string()];
        assert_eq!(matching(&all, "", &picked).len(), 3);
        assert!(matching(&all, "dye", &picked).is_empty());
    }

    /// #579: one entry per SteamID, by the name most demos used, the others
    /// after it; picking it filters by the SteamID.
    #[test]
    fn a_steamid_is_one_entry_named_by_its_most_used_name() {
        let id = Some("76561197960265729");
        let all = choices(&[
            demo(&[("dyelife", id), ("m00cat", Some("76561197960265730"))]),
            demo(&[("dyelife", id)]),
            demo(&[("dyeL!fe[dd]", id), ("lanplayer", None)]),
        ]);
        assert_eq!(
            labels(&all),
            vec!["dyelife (also: dyeL!fe[dd])", "lanplayer", "m00cat"]
        );
        assert_eq!(all[0].term, "76561197960265729");
        assert_eq!(all[0].name, "dyelife");
        assert_eq!(all[1].term, "lanplayer");
        // An old name finds them too, and so does the SteamID.
        assert_eq!(matching(&all, "dd", &[]).len(), 1);
        assert_eq!(matching(&all, "265729", &[]).len(), 1);
        // The picked line names them, not their SteamID.
        assert_eq!(shown_name(&all, "76561197960265729"), "dyelife");
        assert_eq!(shown_name(&all, "someone typed"), "someone typed");
    }

    #[test]
    fn many_other_names_are_cut_short() {
        let id = Some("1");
        let all = choices(&[
            demo(&[("a", id)]),
            demo(&[("a", id)]),
            demo(&[("b", id), ("c", id), ("d", id), ("e", id)]),
        ]);
        assert_eq!(labels(&all), vec!["a (also: b, c, d +1 more)"]);
    }

    #[test]
    fn a_name_without_a_steamid_that_someone_else_uses_is_told_apart() {
        let all = choices(&[demo(&[("dyelife", Some("1"))]), demo(&[("dyelife", None)])]);
        assert_eq!(labels(&all), vec!["dyelife", "dyelife (no SteamID)"]);
        assert_eq!(all[1].term, "dyelife");
    }

    /// #781: the help line names every other name, as many as fit.
    #[test]
    fn the_help_line_spells_out_every_name_that_fits() {
        let id = Some("1");
        let all = choices(&[
            demo(&[("a", id)]),
            demo(&[("a", id)]),
            demo(&[("bb", id), ("cc", id), ("dd", id), ("ee", id)]),
            demo(&[("solo", Some("2"))]),
        ]);
        let a = entry_for(&all, &[], "a (also: bb, cc, dd +1 more)").unwrap();
        assert_eq!(names_help(a, 100), "a, also: bb, cc, dd, ee");
        let cut = names_help(a, 23);
        assert_eq!(cut, "a, also: bb, cc +2 more");
        assert!(cut.chars().count() <= 23, "{cut}");
        assert_eq!(names_help(a, 22), "a, also: bb +3 more");
        assert_eq!(names_help(a, 5), "a, also: 4 other names");
        let solo = entry_for(&all, &[], "solo").unwrap();
        assert_eq!(names_help(solo, 100), "solo: no other names in these demos");
    }

    /// #781: the 25th Anniversary GameUI's font scales with the screen (44 px
    /// rows at 2560x1440 against the pre-Anniversary 22), and the list and
    /// the help line's room scale with it.
    #[test]
    fn the_list_and_the_help_line_scale_with_the_font() {
        // Pre-Anniversary rows, or not measured yet: six pixels a character.
        assert_eq!(list_width(50, 170, 2000, 22), 332);
        assert_eq!(list_width(50, 170, 2000, 0), 332);
        // Anniversary at 2560x1440: twice as wide, still within the tab.
        assert_eq!(list_width(50, 170, 2000, 44), 664);
        assert_eq!(list_width(200, 170, 1000, 44), 1000);
        assert_eq!(list_width(5, 170, 1000, 44), 170);
        // The help line fits half as many characters at twice the font.
        assert_eq!(help_room(1408, 22), 200);
        assert_eq!(help_room(1408, 44), 100);
        assert_eq!(help_room(1408, 0), 200);
        assert_eq!(help_room(100, 44), 20);
    }

    #[test]
    fn a_picked_row_finds_its_entry_by_name() {
        let all = choices(&[demo(&[("dyelife", Some("1")), ("dyeL!fe[dd]", Some("1"))])]);
        assert_eq!(
            entry_for(&all, &[], "[x] dyeL!fe[dd]").map(|c| c.term.as_str()),
            Some("1")
        );
        assert_eq!(
            entry_for(&all, &[], "dyeL!fe[dd] (also: dyelife)").map(|c| c.term.as_str()),
            Some("1")
        );
        assert!(entry_for(&all, &[], "nobody").is_none());
    }

    /// A picked player with no SteamID whose name a SteamID player also used:
    /// their `[x]` row names them, not the SteamID player.
    #[test]
    fn a_picked_row_without_a_steamid_is_its_own_entry() {
        let id = Some("1");
        let all = choices(&[
            demo(&[("dyelife", id)]),
            demo(&[("dyelife", id), ("dl", id)]),
            demo(&[("dyelife", None)]),
        ]);
        let lan = entry_for(&all, &["dyelife".to_string()], "[x] dyelife").unwrap();
        assert_eq!(
            names_help(lan, 100),
            "dyelife: no other names in these demos"
        );
        let steam = entry_for(&all, &["1".to_string()], "[x] dyelife").unwrap();
        assert_eq!(names_help(steam, 100), "dyelife, also: dl");
    }
}
