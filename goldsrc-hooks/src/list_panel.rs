//! What every vgui2 `ListPanel` needs after its rows were deleted and added
//! again, in one place (#796).
//!
//! The list keeps its scroll bar's value when its rows are deleted and
//! refilled: a shorter list shrinks the bar's range but the value stays past
//! the end, so nothing shows until the next scroll, and a long list after a
//! short one draws its bar too short until then. It also sorts rows in as
//! they are added, by what they said then; a row renamed afterwards
//! (`ApplyItemChanges`) is re-indexed but not moved, so the rows sit in the
//! old order while the heading's arrow says otherwise. The Demos tab, the
//! game's own Load Demo window and the Highlights tab each had (or lacked)
//! their own copy of the same two fixes; they call [`after_refill`] now.
//!
//! The slots are `ListPanel`'s on both engine builds (`client.dll`-side
//! GameUI is the same), checked by `tools/verify_studio_panel.py`.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

/// `Panel::InvalidateLayout(bool layoutNow, bool reloadScheme)`, what
/// `Menu::SetFixedWidth` calls. The list is laid out now, before the wheel
/// below, so its scroll bar has the new rows' range: against the old one (a
/// list that fitted, whose range ends below zero) the clamp left the bar
/// above its top, drawn short until the next scroll.
pub const PANEL_SLOT_INVALIDATE_LAYOUT: usize = 58;
/// `ListPanel::OnMouseWheeled(int delta)`: moves its scroll bar by `delta`
/// times three rows through `ScrollBar::SetValue`, which clamps to the
/// bar's range. A big wheel up puts a refilled list back at the top.
pub const LIST_SLOT_ON_MOUSE_WHEELED: usize = 99;
/// `ListPanel::OnSetSortColumn(int column)`, the "SetSortColumn" message's
/// handler: on the column already sorted it flips the direction and sorts
/// the list (`sort_arrows`, #611).
pub const LIST_SLOT_ON_SET_SORT_COLUMN: usize = 190;

/// The `OnSetSortColumn` calls that sort a list again by what its rows show
/// now, given its current sort column (`m_iSortColumn`, -1 for none) and
/// the column to sort by when none is chosen (`None` leaves such a list in
/// the order its rows were added). With a column chosen, flipping its
/// direction twice sorts twice and leaves the direction as it was.
pub fn resort_calls(sort_column: i32, default: Option<i32>) -> Vec<i32> {
    if sort_column < 0 {
        default.into_iter().collect()
    } else {
        vec![sort_column, sort_column]
    }
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::c_void;

    type ListIntFn = unsafe extern "thiscall" fn(*mut c_void, i32);
    type ListIntIntFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32);

    unsafe fn slot<F: Copy>(object: *mut c_void, index: usize) -> F {
        unsafe {
            let vftable = *(object as *const *const usize);
            std::mem::transmute_copy(&*vftable.add(index))
        }
    }

    /// Sorts `list` again by its current column and direction, or by
    /// `default` when none is chosen (see [`super::resort_calls`]).
    /// `sort_column_offset` is where the list keeps `m_iSortColumn`
    /// (`studio_panel`'s `Build::list_sort_column`).
    pub unsafe fn resort(list: *mut c_void, sort_column_offset: usize, default: Option<i32>) {
        unsafe {
            let column = *((list as *const u8).add(sort_column_offset) as *const i32);
            let on_set: ListIntFn = slot(list, super::LIST_SLOT_ON_SET_SORT_COLUMN);
            for column in super::resort_calls(column, default) {
                on_set(list, column);
            }
        }
    }

    /// Lays `list` out, so its scroll bar has the new rows' range, then
    /// scrolls it to the top.
    pub unsafe fn scroll_to_top(list: *mut c_void) {
        unsafe {
            let lay_out: ListIntIntFn = slot(list, super::PANEL_SLOT_INVALIDATE_LAYOUT);
            lay_out(list, 1, 0);
            let wheel: ListIntFn = slot(list, super::LIST_SLOT_ON_MOUSE_WHEELED);
            wheel(list, 100_000);
        }
    }
}

/// After `list` was emptied and filled again: sorts it by what its rows
/// show now ([`resort`]) and starts it at the top ([`scroll_to_top`]).
///
/// # Safety
/// `list` is a live GameUI `ListPanel`, and `sort_column_offset` its
/// build's `m_iSortColumn` offset.
#[cfg(target_arch = "x86")]
pub unsafe fn after_refill(
    list: *mut std::ffi::c_void,
    sort_column_offset: usize,
    default: Option<i32>,
) {
    unsafe {
        hook::resort(list, sort_column_offset, default);
        hook::scroll_to_top(list);
    }
}

/// Sorts `list` again by what its rows show now, without moving its scroll.
///
/// # Safety
/// As [`after_refill`].
#[cfg(target_arch = "x86")]
pub unsafe fn resort(list: *mut std::ffi::c_void, sort_column_offset: usize, default: Option<i32>) {
    unsafe { hook::resort(list, sort_column_offset, default) }
}

/// Lays `list` out and scrolls it to the top, leaving its order alone.
///
/// # Safety
/// `list` is a live GameUI `ListPanel`.
#[cfg(target_arch = "x86")]
pub unsafe fn scroll_to_top(list: *mut std::ffi::c_void) {
    unsafe { hook::scroll_to_top(list) }
}

#[cfg(test)]
mod tests {
    use super::resort_calls;

    #[test]
    fn a_chosen_column_is_flipped_twice_to_keep_its_direction() {
        assert_eq!(resort_calls(1, Some(0)), vec![1, 1]);
        assert_eq!(resort_calls(4, None), vec![4, 4]);
    }

    #[test]
    fn no_column_sorts_by_the_default_or_not_at_all() {
        assert_eq!(resort_calls(-1, Some(0)), vec![0]);
        assert_eq!(resort_calls(-1, Some(1)), vec![1]);
        assert!(resort_calls(-1, None).is_empty());
    }
}
