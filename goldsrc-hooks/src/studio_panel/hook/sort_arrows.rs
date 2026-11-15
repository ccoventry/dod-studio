//! The sort arrow in a list's headings (#611): the Demos and Highlights tabs'
//! lists say which column they are sorted by, and which way, as "Date ▼".
//!
//! GameUI's `ListPanel` sorts on a heading click but draws no marker. Its
//! state, found in both builds' `GameUI.dll` through `OnSetSortColumn`
//! (vftable slot 190, the "SetSortColumn" message's handler; PRE `+0x63460`,
//! Anniversary `+0x6d2c0`), is Source's: the sort column, the secondary
//! column after it, then an ascending flag for each, as plain bytes. A click
//! on the sorted column flips its flag; a click on another makes the old one
//! the secondary. [`Build::list_sort_column`] and
//! [`Build::list_sort_ascending`] hold where they sit.
//!
//! Each frame the two are read; when they change, every heading is set to its
//! own text, plus the arrow on the sorted one. The headings' own text is read
//! once per list, before any arrow is added, through `GetColumnHeaderText`.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use super::*;

/// `ListPanel::SetColumnHeaderText(int column, const wchar_t *text)`: wide,
/// so the arrow needs no conversion on the way.
const LIST_SLOT_SET_COLUMN_HEADER_TEXT_WIDE: usize = 139;
/// `ListPanel::GetColumnHeaderText(int column, char *out, int size)`: false
/// past the last column.
const LIST_SLOT_GET_COLUMN_HEADER_TEXT: usize = 144;
/// More columns than either list has.
const MAX_COLUMNS: i32 = 16;
const HEADING_BYTES: usize = 128;

const UP: char = '\u{25B2}';
const DOWN: char = '\u{25BC}';

type GetHeaderTextFn = unsafe extern "thiscall" fn(*mut c_void, i32, *mut c_char, i32) -> u8;
type SetHeaderTextWideFn = unsafe extern "thiscall" fn(*mut c_void, i32, *const u16);

/// What one list's headings show.
struct Shown {
    list: usize,
    /// Each heading's own text, by column.
    headings: Vec<String>,
    /// The column and direction the arrow is on.
    sort: Option<(i32, bool)>,
}

static SHOWN: Mutex<[Shown; 2]> = Mutex::new([
    Shown {
        list: 0,
        headings: Vec::new(),
        sort: None,
    },
    Shown {
        list: 0,
        headings: Vec::new(),
        sort: None,
    },
]);

/// A heading as it should read: its own text, and the arrow when the list is
/// sorted by it.
fn heading(text: &str, sorted: Option<bool>) -> String {
    match sorted {
        None => text.to_string(),
        Some(true) => format!("{text} {UP}"),
        Some(false) => format!("{text} {DOWN}"),
    }
}

/// Runs every frame while the window shows.
pub(super) unsafe fn update() {
    let Ok((_, build)) = gameui() else { return };
    let dialogs = [
        DEMO_DIALOG.load(Ordering::Acquire),
        super::streaks_tab::dialog(),
    ];
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    for (dialog, shown) in dialogs.into_iter().zip(shown.iter_mut()) {
        if dialog == 0 {
            continue;
        }
        unsafe {
            let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
            if !list.is_null() {
                update_list(build, list, shown);
            }
        }
    }
}

unsafe fn update_list(build: &Build, list: *mut c_void, shown: &mut Shown) {
    unsafe {
        if shown.list != list as usize {
            shown.list = list as usize;
            shown.headings = read_headings(list);
            shown.sort = None;
        }
        let column = *((list as *const u8).add(build.list_sort_column) as *const i32);
        let ascending = *((list as *const u8).add(build.list_sort_ascending)) != 0;
        let sort = (column >= 0).then_some((column, ascending));
        if shown.sort == sort {
            return;
        }
        shown.sort = sort;
        let set: SetHeaderTextWideFn = slot(list, LIST_SLOT_SET_COLUMN_HEADER_TEXT_WIDE);
        for (index, text) in shown.headings.iter().enumerate() {
            let index = index as i32;
            let sorted = sort.filter(|(c, _)| *c == index).map(|(_, up)| up);
            let wide: Vec<u16> = heading(text, sorted)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            set(list, index, wide.as_ptr());
        }
    }
}

/// Every heading's text, in column order.
unsafe fn read_headings(list: *mut c_void) -> Vec<String> {
    let mut headings = Vec::new();
    unsafe {
        let get: GetHeaderTextFn = slot(list, LIST_SLOT_GET_COLUMN_HEADER_TEXT);
        for index in 0..MAX_COLUMNS {
            let mut buffer = vec![0u8; HEADING_BYTES];
            if get(
                list,
                index,
                buffer.as_mut_ptr().cast(),
                HEADING_BYTES as i32,
            ) == 0
            {
                break;
            }
            let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
            headings.push(String::from_utf8_lossy(&buffer[..end]).into_owned());
        }
    }
    headings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sorted_heading_gets_the_arrow_for_its_direction() {
        assert_eq!(heading("Date", Some(true)), "Date \u{25B2}");
        assert_eq!(heading("Date", Some(false)), "Date \u{25BC}");
        assert_eq!(heading("Map", None), "Map");
    }

    /// The fields sit where each build's `OnSetSortColumn` reads them: the
    /// column, the secondary 4 bytes on, then the two flags.
    #[test]
    fn each_build_keeps_the_flag_8_bytes_after_the_column() {
        for build in &BUILDS {
            assert_eq!(
                build.list_sort_ascending,
                build.list_sort_column + 8,
                "{}",
                build.name
            );
        }
        assert_eq!(BUILDS[0].list_sort_column, 0xfc);
        assert_eq!(BUILDS[1].list_sort_column, 0x100);
    }
}
