//! The Review tab (#623): which highlight `dodstudio_review` is on, and the
//! boxes its answer's kill range and note are read from.
//!
//! The labels follow [`crate::review::tab_view`] every frame the tab shows.
//! The boxes are filled once per highlight (the review's generation), so what
//! the user types stays until the review moves on.

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

unsafe fn set(vgui: &Vgui, page: Vpanel, name: &str, slot_index: usize, text: &str) {
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
        let labels = match &view {
            Some(v) => [
                v.heading.clone(),
                v.detail.clone(),
                format!("of {}", v.kills),
            ],
            None => [
                crate::review::IDLE_HEADING.to_string(),
                crate::review::IDLE_DETAIL.to_string(),
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

/// A text box's text, however long.
unsafe fn long_text(vgui: &Vgui, page: Vpanel, name: &str) -> String {
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
