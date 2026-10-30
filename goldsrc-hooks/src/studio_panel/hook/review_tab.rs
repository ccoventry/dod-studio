//! The Review tab (#623): which highlight `ds_demo_review` is on, and the
//! boxes its answer's kill range and note are read from.
//!
//! The labels follow [`crate::review::tab_view`] every frame the tab shows.
//! The boxes are filled once per highlight (the review's generation), so what
//! the user types stays until the review moves on. With no review running,
//! From the start and From here ask DoD Studio for a review of the demo
//! playing (#1142), greyed while that can't happen, and the second line says
//! why or how the request is going ([`crate::review::IdleFacts`]).

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

const HEADING: &str = "ReviewHeading";
const DETAIL: &str = "ReviewDetail";
const KILL_COUNT: &str = "KillCount";
const FROM: &str = "ReviewFrom";
const TO: &str = "ReviewTo";
const NOTE: &str = "ReviewNote";

/// The labels' text as last set.
static SHOWN: std::sync::Mutex<Option<[String; 3]>> = std::sync::Mutex::new(None);
/// The review generation the boxes were last filled for.
static FILLED: AtomicU32 = AtomicU32::new(u32::MAX);

fn page() -> Vpanel {
    // Safety: the page object this module built, or null before that.
    unsafe { vpanel_of(PAGE_OBJECTS[REVIEW_PAGE].load(Ordering::Acquire) as *mut c_void) }
}

/// Sets the text of the control `name` on `page` through its `slot_index`
/// (a label's or a text box's `SetText`); nothing when the layout has none.
pub(super) unsafe fn set(vgui: &Vgui, page: Vpanel, name: &str, slot_index: usize, text: &str) {
    unsafe {
        let Some(object) = vgui
            .child_named(page, name)
            .map(|vp| vgui.object(vp))
            .filter(|o| !o.is_null())
        else {
            return;
        };
        if let Ok(text) = CString::new(text) {
            let set_text: SetTextFn = slot(object, slot_index);
            set_text(object, text.as_ptr());
        }
    }
}

/// Runs every frame while the window shows.
pub(super) unsafe fn update(vgui: &Vgui) {
    unsafe {
        let page = page();
        if page == 0 || !vgui.visible(page) {
            return;
        }
        let view = crate::review::tab_view();
        // Only looked at with no review running: it may look up the pipe.
        let idle = view.is_none().then(crate::review::IdleFacts::now);
        let labels = match (&view, &idle) {
            (Some(v), _) => [
                v.heading.clone(),
                v.detail.clone(),
                format!("of {}", v.kills),
            ],
            (None, facts) => [
                crate::review::IDLE_HEADING.to_string(),
                facts.as_ref().map(|f| f.detail()).unwrap_or_default(),
                String::new(),
            ],
        };
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        if shown.as_ref() != Some(&labels) {
            for (name, text) in [HEADING, DETAIL, KILL_COUNT].iter().zip(&labels) {
                set(vgui, page, name, LABEL_SLOT_SET_TEXT, text);
            }
            *shown = Some(labels);
        }
        drop(shown);
        enable_review_only(vgui, page, view.is_some());
        let can_ask = idle.as_ref().is_some_and(|f| f.can_ask());
        enable(vgui, page, &crate::review::START_BUTTONS, can_ask);
        if let Some(v) = view
            && FILLED.swap(v.generation, Ordering::AcqRel) != v.generation
        {
            set(
                vgui,
                page,
                FROM,
                TEXT_ENTRY_SLOT_SET_TEXT,
                &v.from.to_string(),
            );
            set(vgui, page, TO, TEXT_ENTRY_SLOT_SET_TEXT, &v.to.to_string());
            set(vgui, page, NOTE, TEXT_ENTRY_SLOT_SET_TEXT, &v.note);
        }
    }
}

type IsEnabledFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;

/// Greys out the answers, moves, Stop and boxes while no review runs, and
/// enables them while one does (`review_lock::REVIEW_ONLY`). Only a control
/// whose state differs is touched, so a frame with nothing to change costs
/// ten `IsEnabled` calls.
unsafe fn enable_review_only(vgui: &Vgui, page: Vpanel, on: bool) {
    unsafe { enable(vgui, page, crate::review_lock::REVIEW_ONLY, on) }
}

/// Enables or greys each named control, touching only those whose state
/// differs.
unsafe fn enable(vgui: &Vgui, page: Vpanel, names: &[&str], on: bool) {
    unsafe {
        for name in names {
            let Some(object) = vgui
                .child_named(page, name)
                .map(|vp| vgui.object(vp))
                .filter(|o| !o.is_null())
            else {
                continue;
            };
            let is_enabled: IsEnabledFn = slot(object, PANEL_SLOT_IS_ENABLED);
            if (is_enabled(object) & 0xff != 0) != on {
                let set: ListIntVoidFn = slot(object, PANEL_SLOT_SET_ENABLED);
                set(object, i32::from(on));
            }
        }
    }
}

/// A text box's text, however long.
pub(super) unsafe fn long_text(vgui: &Vgui, page: Vpanel, name: &str) -> String {
    unsafe {
        let Some(entry) = vgui
            .child_named(page, name)
            .map(|vp| vgui.object(vp))
            .filter(|o| !o.is_null())
        else {
            return String::new();
        };
        let get_text: GetTextFn = slot(entry, TEXT_ENTRY_SLOT_GET_TEXT);
        let mut buf = vec![0u8; 1024];
        get_text(entry, buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
        CStr::from_bytes_until_nul(&buf)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The From, To and Note boxes, when they were filled for the highlight the
/// review is on -- otherwise they still hold the last one's.
pub(in crate::studio_panel) fn inputs() -> Option<(String, String, String)> {
    if FILLED.load(Ordering::Acquire) != crate::review::generation() {
        return None;
    }
    let page = page();
    if page == 0 {
        return None;
    }
    let vgui = Vgui::get().ok()?;
    unsafe {
        Some((
            long_text(&vgui, page, FROM),
            long_text(&vgui, page, TO),
            long_text(&vgui, page, NOTE),
        ))
    }
}
