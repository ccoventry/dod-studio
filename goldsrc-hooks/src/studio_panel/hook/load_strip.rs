//! The loading strip (#797): one line along the top of whichever tab is
//! showing, saying how far the demo player has read the demo and how far the
//! highlights analysis has got, while either still runs. The tabs' controls
//! sit below it while it shows ([`inset`]); it goes once both are done.
//! While a demo found damaged plays, it also carries that demo's line
//! (`demo_damage::tell::strip_line`, #1373), and stays.
//!
//! It is `DodStudio.res`'s `LoadStrip` label, moved onto the page showing:
//! a page paints over whatever of the window lies under it, so a strip left
//! on the window would be hidden by the page it overlaps.
//!
//! The highlights analysis starts as soon as a demo plays ([`request_highlights`]),
//! window open or not; the Highlights tab only shows its result. The demo
//! load needs nothing: it runs at the same rate with the window closed.

use std::ffi::CString;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use super::*;

/// The demo file the highlights were last asked for, so they are asked
/// once per demo rather than every frame; by path, so a same-name replay
/// from another folder is asked again (#1474).
static REQUESTED: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
/// The strip, and the text on it (`None` while hidden): a rebuilt window's
/// new strip is written again.
static SHOWN: std::sync::Mutex<(Vpanel, Option<String>)> = std::sync::Mutex::new((0, None));
/// The highlights' last percent, [`NONE`] when none runs: kept for the frames
/// the analysis' worker holds its result's lock.
static HIGHLIGHTS: AtomicU32 = AtomicU32::new(NONE);
const NONE: u32 = u32::MAX;
/// How far the pages' controls sit down: [`LOAD_STRIP_TALL`] while the strip
/// shows, else 0.
static INSET: AtomicI32 = AtomicI32::new(0);

/// Starts finding the playing demo's highlights. Runs every frame, window or
/// not; asks once per demo ([`crate::streaks::request`] ignores a repeat).
pub(super) fn request_highlights() {
    let Some(name) = crate::demo_reload::current_demo() else {
        return;
    };
    let path = crate::demo_locate::playing(&name);
    let mut requested = REQUESTED.lock().unwrap_or_else(|e| e.into_inner());
    if requested.as_ref() == Some(&path) {
        return;
    }
    crate::streaks::request(&path);
    *requested = Some(path);
}

/// How far the tabs' controls move down for the strip.
pub(super) fn inset() -> i32 {
    INSET.load(Ordering::Acquire)
}

/// Shows, moves or hides the strip. Runs every frame while the window shows,
/// before the pages are fitted.
pub(super) unsafe fn update(vgui: &Vgui, frame_vp: Vpanel) {
    unsafe {
        let demo = load_progress::demo_percent();
        let highlights = match crate::streaks::loading_percent() {
            Some(now) => {
                HIGHLIGHTS.store(now.unwrap_or(NONE), Ordering::Release);
                now
            }
            None => Some(HIGHLIGHTS.load(Ordering::Acquire)).filter(|&p| p != NONE),
        };
        let text = load_strip_text(demo, highlights, crate::demo_damage::tell::strip_line());
        let page = PAGE_OBJECTS
            .iter()
            .map(|p| vpanel_of(p.load(Ordering::Acquire) as *mut c_void))
            .find(|&vp| vp != 0 && vgui.visible(vp));
        let Some(strip) = find(vgui, frame_vp, page) else {
            // A layout edited before the strip existed: no room to make.
            INSET.store(0, Ordering::Release);
            return;
        };
        INSET.store(
            if text.is_some() { LOAD_STRIP_TALL } else { 0 },
            Ordering::Release,
        );
        if let Some(page) = page {
            if vgui.parent_of(strip) != page {
                vgui.set_parent(strip, page);
            }
            let (_, _, wide, _) = vgui.rect(page);
            vgui.place(strip, load_strip_bounds(wide));
        }
        if vgui.visible(strip) != text.is_some() {
            vgui.set_visible(strip, text.is_some());
        }
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        if shown.0 == strip && shown.1 == text {
            return;
        }
        if shown.1.is_some() != text.is_some() {
            crate::debug::report(&format!(
                "studio_panel: loading strip: {}",
                text.as_deref().unwrap_or("gone, both loads done")
            ));
        }
        if let Some(text) = &text {
            let object = vgui.object(strip);
            if let (false, Ok(c_text)) = (object.is_null(), CString::new(text.as_str())) {
                let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
                set_text(object, c_text.as_ptr());
            }
        }
        *shown = (strip, text);
    }
}

/// The strip: on the page showing (where it usually is), still on the window
/// (before its first move), or on a page shown before.
unsafe fn find(vgui: &Vgui, frame_vp: Vpanel, showing: Option<Vpanel>) -> Option<Vpanel> {
    unsafe {
        if let Some(vp) = showing.and_then(|page| vgui.child_named(page, LOAD_STRIP)) {
            return Some(vp);
        }
        if let Some(vp) = vgui.child_named(frame_vp, LOAD_STRIP) {
            return Some(vp);
        }
        PAGE_OBJECTS
            .iter()
            .map(|p| vpanel_of(p.load(Ordering::Acquire) as *mut c_void))
            .filter(|&vp| vp != 0 && Some(vp) != showing)
            .find_map(|page| vgui.child_named(page, LOAD_STRIP))
    }
}
