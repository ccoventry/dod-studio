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
//!
//! One entry per player, not per name (#579): everyone with a SteamID is
//! listed once, by the name they used in the most demos, with their other
//! names after it -- `dyelife (also: dyeL!fe[dd])`. Picking that entry
//! filters by the SteamID, which [`crate::demo_rosters::has_player`] matches
//! whatever name was used. A player with no SteamID (a demo without `*sid`,
//! LAN) stays one entry per name.

use std::collections::HashMap;
use std::ffi::{CString, c_void};
use std::sync::Mutex;

use analysis::cache::DemoPlayers;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Roughly how wide a character of the window's font is, in pixels: what
/// the picked-players line is cut to fit by.
const CHAR_WIDE: i32 = 7;

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
                    "{name} (also: {} +{})",
                    others[..MAX_ALSO].join(", "),
                    n - MAX_ALSO
                ),
            };
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
        assert_eq!(labels(&all), vec!["a (also: b, c, d +1)"]);
    }

    #[test]
    fn a_name_without_a_steamid_that_someone_else_uses_is_told_apart() {
        let all = choices(&[demo(&[("dyelife", Some("1"))]), demo(&[("dyelife", None)])]);
        assert_eq!(labels(&all), vec!["dyelife", "dyelife (no SteamID)"]);
        assert_eq!(all[1].term, "dyelife");
    }
}
