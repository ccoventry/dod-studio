//! The Playback tab's loading progress (#465): while `viewdemo` is still
//! reading the demo, a label and a progress bar say how far it has got.
//!
//! The demo player reads the whole file into its world before it is done
//! ("Demo file completely loaded."), and plays and seeks only as far as it
//! has read. How far: the buffered span (`GetEndTime - GetStartTime`, through
//! [`crate::demo_seek::buffered_while_loading`]) over the playback length in
//! the file's own directory ([`crate::demo_file::length`]), less the stretch
//! the LOADING segment adds in front (#810).

use std::ffi::{CString, c_void};
use std::sync::atomic::Ordering;

use super::*;

const LABEL: &str = "LoadLabel";
const BAR: &str = "LoadProgress";

/// The demo whose length is known, and that length; and the last percent
/// shown (`None` while hidden).
static LENGTH: std::sync::Mutex<Option<(String, Option<crate::demo_file::DemoLength>)>> =
    std::sync::Mutex::new(None);
static SHOWN: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

type SetProgressFn = unsafe extern "thiscall" fn(*mut c_void, f32);

/// Runs every frame while the window shows.
pub(super) unsafe fn update(vgui: &Vgui) {
    unsafe {
        let page = vpanel_of(PAGE_OBJECTS[PLAYBACK_PAGE].load(Ordering::Acquire) as *mut c_void);
        if page == 0 || !vgui.visible(page) {
            return;
        }
        let percent = demo_percent();
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        if *shown == percent {
            return;
        }
        if shown.is_some() && percent.is_none() {
            crate::debug::report("studio_panel: Playback tab: the demo is loaded");
        } else if let Some(p) = percent
            && shown.is_none_or(|was| p / 10 != was / 10)
        {
            crate::debug::report(&format!(
                "studio_panel: Playback tab: loading the demo, {p}%"
            ));
        }
        *shown = percent;
        for name in [LABEL, BAR] {
            if let Some(vp) = vgui.child_named(page, name) {
                vgui.set_visible(vp, percent.is_some());
            }
        }
        let Some(percent) = percent else { return };
        if let Some(label) = vgui.child_named(page, LABEL) {
            let object = vgui.object(label);
            if let (false, Ok(text)) = (
                object.is_null(),
                CString::new(format!("Loading the demo: {percent}%")),
            ) {
                let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
                set_text(object, text.as_ptr());
            }
        }
        let Ok((base, build)) = gameui() else { return };
        if let Some(bar) = vgui.child_named(page, BAR) {
            let object = vgui.object(bar);
            if !object.is_null() && *(object as *const usize) == base + build.progress_bar_vftable {
                let set_progress: SetProgressFn = slot(object, PROGRESS_BAR_SLOT_SET_PROGRESS);
                set_progress(object, percent as f32 / 100.0);
            }
        }
    }
}

/// How much of the playing demo the demo player has read, while it is still
/// reading. Shared with the loading strip (#797) and the stock bar's
/// progress (`demo_bar_progress`), so all three say the same, and worked out
/// whatever tab is showing.
pub(super) fn demo_percent() -> Option<u32> {
    crate::demo_seek::buffered_while_loading().and_then(|buffered| {
        let length = length_of_current_demo()?;
        Some(crate::demo_file::load_percent(
            buffered,
            length,
            loaded_span_of_current_demo(),
        ))
    })
}

/// The demo whose loaded span is being worked out, and the span once known
/// (#866). A background thread walks the file; until it answers, the
/// directory's estimate stands. `try_lock` only: this runs every frame.
static SPAN: std::sync::Mutex<Option<(String, Option<f32>)>> = std::sync::Mutex::new(None);

fn loaded_span_of_current_demo() -> Option<f32> {
    let name = crate::demo_reload::current_demo()?;
    let mut known = SPAN.try_lock().ok()?;
    match known.as_ref() {
        Some((demo, span)) if *demo == name => return *span,
        _ => {}
    }
    *known = Some((name.clone(), None));
    drop(known);
    let path = crate::streaks::demo_path(&dod_dir(), &name);
    std::thread::spawn(move || {
        let span = crate::demo_file::loaded_span(&path);
        let mut known = SPAN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((demo, slot)) = known.as_mut()
            && *demo == name
        {
            *slot = span;
        }
    });
    None
}

/// The playing demo's playback length, read once per demo.
fn length_of_current_demo() -> Option<crate::demo_file::DemoLength> {
    let name = crate::demo_reload::current_demo()?;
    let mut known = LENGTH.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((demo, length)) = known.as_ref()
        && *demo == name
    {
        return *length;
    }
    let game_dir = dod_dir();
    let length = crate::demo_file::length(&crate::streaks::demo_path(&game_dir, &name));
    *known = Some((name, length));
    length
}
