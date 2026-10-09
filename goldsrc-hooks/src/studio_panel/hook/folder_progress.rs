//! The Demos tab's counting bar (#409): while `folder_counts` counts the
//! listed folders' demos off the game thread, a progress bar stands where
//! the hint is and says how far it has got. When the count is done the tab
//! lists again (`filter_demo_list`), and the bar gives the hint back.

use std::ffi::c_void;
use std::sync::atomic::Ordering;

use super::*;

const BAR: &str = "FolderProgress";

/// The fraction last shown, in thousandths (`None` while hidden), so the bar
/// is only touched when it moves.
static SHOWN: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

type SetProgressFn = unsafe extern "thiscall" fn(*mut c_void, f32);

/// Runs every frame while the window shows.
pub(super) unsafe fn update(vgui: &Vgui) {
    unsafe {
        let page = vpanel_of(PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void);
        if page == 0 || !vgui.visible(page) {
            return;
        }
        let fraction = crate::folder_counts::progress()
            .map(|(done, total)| (done * 1000 / total.max(1)).min(1000) as u32);
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        if *shown == fraction {
            return;
        }
        let was_showing = shown.is_some();
        *shown = fraction;
        let Some(bar) = vgui.child_named(page, BAR) else {
            return;
        };
        vgui.set_visible(bar, fraction.is_some());
        // The hint makes room while the bar shows, and comes back after,
        // unless the Player note has its place.
        if let Some(hint) = vgui.child_named(page, DEMOS_HINT) {
            let note_up = vgui
                .child_named(page, PLAYER_NOTE)
                .is_some_and(|note| vgui.visible(note));
            if fraction.is_some() {
                vgui.set_visible(hint, false);
            } else if was_showing {
                vgui.set_visible(hint, !note_up);
            }
        }
        let Some(fraction) = fraction else { return };
        let Ok((base, build)) = gameui() else { return };
        let object = vgui.object(bar);
        if !object.is_null() && *(object as *const usize) == base + build.progress_bar_vftable {
            let set_progress: SetProgressFn = slot(object, PROGRESS_BAR_SLOT_SET_PROGRESS);
            set_progress(object, fraction as f32 / 1000.0);
        }
    }
}
