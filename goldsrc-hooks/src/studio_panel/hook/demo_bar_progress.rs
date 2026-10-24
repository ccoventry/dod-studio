//! The stock VCR bar's loading progress (#465): the `StudioLoadPercent`
//! label and `StudioLoadProgress` bar that DoD Studio's launch adds to the
//! bar's layout, filled in while `viewdemo` is still reading the demo. The
//! percent is the Playback tab's (`load_progress`); this is the same thing
//! on the stock bar, for when the bar is in use (`ds_ui_viewdemo_in_panel
//! 0`). See `crate::demo_bar_progress`.

use std::ffi::{CString, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::*;
use crate::demo_bar_progress::{PERCENT_LABEL, PROGRESS_BAR, SETTING, percent_text, worth_logging};

/// The percent last shown, [`HIDDEN`] while hidden. An atomic, not a
/// mutex: this runs every frame.
static SHOWN: AtomicU32 = AtomicU32::new(HIDDEN);
const HIDDEN: u32 = u32::MAX;
/// Whether a bar without the controls has been reported, once a session.
static MISSING_REPORTED: AtomicBool = AtomicBool::new(false);

type SetProgressFn = unsafe extern "thiscall" fn(*mut c_void, f32);

/// Runs every frame, with or without our window: a few reads until a
/// demo is loading and the stock bar is the one in use.
pub(super) fn update() {
    let shown = match SHOWN.load(Ordering::Relaxed) {
        HIDDEN => None,
        p => Some(p),
    };
    let store = |p: Option<u32>| SHOWN.store(p.unwrap_or(HIDDEN), Ordering::Relaxed);
    let percent = if SETTING.on() && !viewdemo_in_panel() {
        super::load_progress::demo_percent()
    } else {
        None
    };
    if shown == percent {
        return;
    }
    let Ok(vgui) = Vgui::get() else { return };
    // Safety: vgui2's own interfaces, on the engine's thread, as the rest of
    // the window's code uses them.
    unsafe {
        let Some(bar) = vgui.bar() else {
            store(None);
            return;
        };
        let (label, progress) = (
            vgui.child_named(bar, PERCENT_LABEL),
            vgui.child_named(bar, PROGRESS_BAR),
        );
        if label.is_none() && progress.is_none() {
            if percent.is_some() && !MISSING_REPORTED.swap(true, Ordering::AcqRel) {
                crate::debug::report(&format!(
                    "demo_bar_progress: the demo bar has no {PERCENT_LABEL}: the game was \
                     started without DoD Studio's dod_addon\\resource\\DemoPlayerDialog.res \
                     or without -addons, so it shows no loading progress"
                ));
            }
            store(None);
            return;
        }
        if worth_logging(shown, percent) {
            crate::debug::report(&match percent {
                Some(p) => format!("demo_bar_progress: loading the demo, {p}%"),
                None => "demo_bar_progress: the demo is loaded".to_string(),
            });
        }
        store(percent);
        for vp in [label, progress].into_iter().flatten() {
            vgui.set_visible(vp, percent.is_some());
        }
        let Some(percent) = percent else { return };
        if let Some(label) = label {
            let object = vgui.object(label);
            if let (false, Ok(text)) = (object.is_null(), CString::new(percent_text(percent))) {
                let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
                set_text(object, text.as_ptr());
            }
        }
        let Ok((base, build)) = gameui() else { return };
        if let Some(progress) = progress {
            let object = vgui.object(progress);
            if !object.is_null() && *(object as *const usize) == base + build.progress_bar_vftable {
                let set_progress: SetProgressFn = slot(object, PROGRESS_BAR_SLOT_SET_PROGRESS);
                set_progress(object, percent as f32 / 100.0);
            }
        }
    }
}
