//! Greys out the demo controls while a review runs (#826, `review_lock`):
//! the VCR bar's buttons and slider, the game's Load Demo window's Load, and
//! DoD Studio's Playback tab and the Demos and Highlights tabs' Load / Go,
//! wherever each lives this frame (a lent control moves between its own
//! window and our tab). The game's own Load Demo window is closed while
//! locked, since its list loads on a double-click whether or not it is
//! enabled.
//!
//! Every handle comes from this frame's walk of GameUI's popups and our live
//! window, never from an earlier frame (a kept VPANEL of a deleted panel
//! crashes vgui2). Each control greyed is remembered with its state before,
//! and the release, the frame after the review ends, puts exactly that back.

use std::ffi::c_void;

use super::*;
use crate::review_lock::{self, Held, Holder};

type IsEnabledFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;

/// What the lock greyed out, and whether it was on last frame. Only the
/// engine's main thread polls, so no lock is needed.
struct State {
    held: Vec<Held>,
    on: bool,
}

thread_local! {
    static STATE: std::cell::RefCell<State> = const {
        std::cell::RefCell::new(State { held: Vec::new(), on: false })
    };
}

/// A walk that finds more than this many controls to remember is a layout
/// gone wrong; the rest are greyed but not remembered, so never enabled.
const MAX_HELD: usize = 1024;

/// Every lockable control this frame: panel, name and object.
unsafe fn lockable(vgui: &Vgui) -> Vec<(Vpanel, String, *mut c_void)> {
    let mut holders: Vec<(Vpanel, Holder)> = Vec::new();
    unsafe {
        for (vp, name) in vgui.gameui_popups() {
            let holder = match name.as_str() {
                review_lock::VCR_BAR => Holder::VcrBar,
                review_lock::LOAD_DEMO => Holder::LoadDemo,
                DEMO_LIST | STREAK_LIST => Holder::LendingList,
                _ => continue,
            };
            holders.push((vp, holder));
        }
        if window(vgui).is_some() {
            for (page, holder) in [
                (PLAYBACK_PAGE, Holder::PlaybackTab),
                (DEMOS_PAGE, Holder::ListTab),
                (STREAKS_PAGE, Holder::ListTab),
            ] {
                let vp = vpanel_of(PAGE_OBJECTS[page].load(Ordering::Acquire) as *mut c_void);
                if vp != 0 {
                    holders.push((vp, holder));
                }
            }
        }
        let mut found = Vec::new();
        for (vp, holder) in holders {
            for child in vgui.child_list(vp) {
                let name = vgui.name(child);
                if !review_lock::locks(holder, &name) {
                    continue;
                }
                let object = vgui.object(child);
                if !object.is_null() {
                    found.push((child, name, object));
                }
            }
        }
        found
    }
}

unsafe fn is_enabled(object: *mut c_void) -> bool {
    unsafe {
        let is_enabled: IsEnabledFn = slot(object, PANEL_SLOT_IS_ENABLED);
        is_enabled(object) & 0xff != 0
    }
}

unsafe fn set_enabled(object: *mut c_void, on: bool) {
    unsafe {
        let set: ListIntVoidFn = slot(object, PANEL_SLOT_SET_ENABLED);
        set(object, i32::from(on));
    }
}

/// Closes the game's own Load Demo window when it is open, returning
/// whether it was.
unsafe fn close_load_demo(vgui: &Vgui) -> bool {
    unsafe {
        match vgui.popup(review_lock::LOAD_DEMO) {
            Some(vp) if vgui.visible(vp) => {
                vgui.set_visible(vp, false);
                true
            }
            _ => false,
        }
    }
}

/// Follows the review: greys the controls while it runs (again each frame,
/// in case GameUI enabled one), and puts them back the frame after. A few
/// atomic loads when no review has run.
pub(in crate::studio_panel) fn poll() {
    let locked = review_lock::locked();
    let idle = STATE.with(|cell| {
        cell.try_borrow()
            .map(|s| !s.on && s.held.is_empty())
            .unwrap_or(true)
    });
    if !locked && idle {
        return;
    }
    let Ok(vgui) = Vgui::get() else { return };
    STATE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return;
        };
        unsafe {
            let found = lockable(&vgui);
            if locked {
                let closed = close_load_demo(&vgui);
                let mut greyed = 0;
                for (vp, name, object) in &found {
                    let enabled = is_enabled(*object);
                    let (remember, grey) = review_lock::on_sight(&state.held, *vp, name, enabled);
                    if let Some(was_enabled) = remember
                        && state.held.len() < MAX_HELD
                    {
                        state.held.push(Held {
                            panel: *vp,
                            name: name.clone(),
                            was_enabled,
                        });
                    }
                    if grey {
                        set_enabled(*object, false);
                        greyed += 1;
                    }
                }
                if !state.on {
                    state.on = true;
                    crate::debug::report(&format!(
                        "lock_controls: a review is running: the VCR bar, Load Demo and DoD Studio's Playback, Demos and Highlights controls are locked ({greyed} greyed out{})",
                        if closed { ", Load Demo closed" } else { "" }
                    ));
                } else if closed {
                    crate::debug::report("lock_controls: closed the Load Demo window (locked during a review)");
                }
            } else {
                let mut restored = 0;
                for (vp, name, object) in &found {
                    if review_lock::restores(&state.held, *vp, name) && !is_enabled(*object) {
                        set_enabled(*object, true);
                        restored += 1;
                    }
                }
                crate::debug::report(&format!(
                    "lock_controls: the review ended: the demo controls work again ({restored} enabled again, {} remembered)",
                    state.held.len()
                ));
                state.held.clear();
                state.on = false;
            }
        }
    });
}
