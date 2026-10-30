//! The kill range and note form's window (#1139): a small GameUI `Frame` of
//! its own, laid out by `ReviewForm.res`, shown over the playing demo with
//! the menu closed. What it shows and the rules for what is typed live in
//! `crate::review_form`; this file builds, shows, reads and hides it.
//!
//! ## Over the demo, with the menu closed
//!
//! A GameUI window hangs from GameUI's `TaskBar`, which GameUI hides with
//! the menu (`TaskBar < BasePanel < BaseGameUIPanel < StaticPanel`). The
//! form is built under `TaskBar`, as our main window is, then moved to the
//! root of that chain (`StaticPanel`, which stays shown) with
//! `IPanel::SetParent`, so it shows while the demo plays. Measured on both
//! builds by #1139's probe: the window draws over the playing demo, keyboard
//! focus lands in a box asked for it (`IPanel::RequestFocus`), keys typed go
//! into the box, and Y or N typed there never reach the answer keys (vgui2
//! keeps the keys a box has focus for; `HUD_Key_Event` never sees them).
//! Hidden, the keys are the game's again. A click on its buttons worked on
//! the pre-Anniversary build only (the Anniversary build's mouse is raw
//! input), so everything works from the keyboard: Tab moves between the
//! boxes (`tabPosition` in the layout), Enter saves, Esc cancels.
//!
//! ## Keys and buttons
//!
//! A box passes the keys it doesn't use (Enter, Esc) up to the window, whose
//! `OnKeyCodeTyped` (slot [`PANEL_SLOT_ON_KEY_CODE_TYPED`]) is ours in a
//! copied vftable; its Save and Cancel buttons send `save` and `cancel` to
//! its `OnCommand`, ours too. A kill range outside the highlight keeps the
//! form open with what to type on its last line.
//!
//! ## When it closes by itself
//!
//! When the review moves to another highlight or ends, and when the menu
//! comes up (the form isn't part of it, so it would sit on top): each
//! cancels, as Esc does. The engine brings the menu up on the very Esc the
//! form cancels with, so a menu that comes up just after the form's Esc, or
//! while it shows, is put away again: Esc on the form means the form.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::review_form::FormView;

/// The window's panel name, which its `.res` entry is keyed by.
const FORM_NAME: &CStr = c"DodStudioReviewForm";
const TITLE: &str = "FormTitle";
const FROM: &str = "FormFrom";
const TO: &str = "FormTo";
const KILL_COUNT: &str = "FormKillCount";
const NOTE: &str = "FormNote";
const HINT: &str = "FormHint";
const SAVE: &str = "save";
const CANCEL: &str = "cancel";
/// vgui2's key codes for Esc (`KEY_ESCAPE`); Enter's are the window's.
const KEY_ESCAPE: i32 = 70;
/// `IPanel::SetMouseInputEnabled`, beside the keyboard's slot 31.
const IPANEL_SET_MOUSE_INPUT_ENABLED: usize = 32;
/// How far down the screen the form's top sits: under the review's box on
/// the left, clear of the chat at the bottom.
const TOP_PERCENT: i32 = 55;

/// The form we built, or 0.
static FORM: AtomicUsize = AtomicUsize::new(0);
/// The class's own `OnKeyCodeTyped` and `OnCommand`, for what isn't ours.
static OWN_ON_KEY: AtomicUsize = AtomicUsize::new(0);
static OWN_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
/// The highlight the form shows, while it shows. Read every frame it shows,
/// so an `RwLock` (CLAUDE.md: no blocking mutex per frame).
static SHOWING: std::sync::RwLock<Option<FormView>> = std::sync::RwLock::new(None);

fn trace(line: &str) {
    unsafe { crate::debug::report(&format!("review_form: {line}")) };
}

fn showing() -> std::sync::RwLockReadGuard<'static, Option<FormView>> {
    SHOWING.read().unwrap_or_else(|e| e.into_inner())
}

fn set_showing(view: Option<FormView>) {
    *SHOWING.write().unwrap_or_else(|e| e.into_inner()) = view;
}

/// Runs every frame: opens the form when the review asked for it, and
/// closes it when what it shows has gone.
pub(super) unsafe fn update(vgui: &Vgui) {
    unsafe {
        if crate::review_form::take_open_request() {
            match open(vgui) {
                Ok(note) => trace(&note),
                Err(why) => {
                    crate::commands::console_print(&format!(
                        "{}: the kill range and note form: {why}\n",
                        crate::review::NAME
                    ));
                    trace(&format!("not opened: {why}"));
                }
            }
        }
        if crate::review_form::is_shown() {
            match form(vgui) {
                Some((_, vp)) => {
                    if let Some(why) = reason_to_close(vgui, vp) {
                        vgui.set_visible(vp, false);
                        close(why);
                    }
                }
                None => close("its window has gone"),
            }
        }
        put_away_esc_menu(vgui);
    }
}

/// Hides the menu the form's Esc brought up: the engine opens the menu on
/// the same Esc the form takes to cancel (seen live on PRE). The review
/// doesn't open its Review tab for it either (`review::watch_menu`).
unsafe fn put_away_esc_menu(vgui: &Vgui) {
    unsafe {
        if crate::review_form::menu_is_from_esc() && menu_up(vgui) {
            close_for_playback();
            trace("put away the menu the form's Esc brought up");
        }
    }
}

/// Why the shown form must close now, if it must.
unsafe fn reason_to_close(vgui: &Vgui, vp: Vpanel) -> Option<&'static str> {
    unsafe {
        if !vgui.visible(vp) {
            return Some("hidden");
        }
        let shown_key = showing().as_ref().map(|v| v.key.clone());
        let review_key = crate::review::form_view().map(|v| v.key);
        if review_key.is_none() {
            return Some("the review ended");
        }
        if shown_key != review_key {
            return Some("the review moved to another highlight");
        }
        if menu_up(vgui) {
            // Esc, whichever of the form and the engine saw it first.
            crate::review_form::note_esc();
            return Some("the menu came up");
        }
        None
    }
}

/// Whether GameUI's menu shows: its `TaskBar` and everything above it.
unsafe fn menu_up(vgui: &Vgui) -> bool {
    unsafe { base_panel(vgui).is_ok_and(|taskbar| vgui.shown(vpanel_of(taskbar))) }
}

/// Notes the form closed, without saving.
fn close(why: &str) {
    crate::review_form::set_shown(false);
    set_showing(None);
    trace(&format!("closed: {why}"));
}

/// The form, if it still exists as the object we built.
unsafe fn form(vgui: &Vgui) -> Option<(*mut c_void, Vpanel)> {
    let object = FORM.load(Ordering::Acquire) as *mut c_void;
    if object.is_null() {
        return None;
    }
    unsafe {
        let vp = vpanel_of(object);
        (vp != 0 && vgui.object(vp) == object).then_some((object, vp))
    }
}

/// The top of the panel chain GameUI hangs from (`StaticPanel`), which stays
/// shown with the menu closed.
unsafe fn root(vgui: &Vgui) -> Result<Vpanel, String> {
    unsafe {
        let mut panel = vpanel_of(base_panel(vgui)?);
        for _ in 0..32 {
            let parent = vgui.parent_of(panel);
            if parent == 0 {
                return Ok(panel);
            }
            panel = parent;
        }
        Err("GameUI's panels have no root".to_string())
    }
}

/// Builds the form under GameUI's `TaskBar`, as the main window is built,
/// with our own key and command handlers.
unsafe fn build(vgui: &Vgui) -> Result<*mut c_void, String> {
    let (base, build) = gameui()?;
    unsafe {
        let parent = base_panel(vgui)?;
        let frame = allocate(base, build, build.frame_size)?;
        if build.frame_ctor_fourth_arg {
            let ctor: FrameCtor4 = std::mem::transmute(base + build.frame_ctor);
            ctor(frame, parent, FORM_NAME.as_ptr(), 1, 0);
        } else {
            let ctor: FrameCtor3 = std::mem::transmute(base + build.frame_ctor);
            ctor(frame, parent, FORM_NAME.as_ptr(), 1);
        }
        let own = own_slots(
            frame,
            &[
                (FRAME_SLOT_ON_COMMAND, form_on_command as *const () as usize),
                (
                    PANEL_SLOT_ON_KEY_CODE_TYPED,
                    form_on_key as *const () as usize,
                ),
            ],
        );
        OWN_ON_COMMAND.store(own[0], Ordering::Release);
        OWN_ON_KEY.store(own[1], Ordering::Release);
        let load: LoadSettingsFn = std::mem::transmute(base + build.load_control_settings);
        load(frame, FORM_RES.0.as_ptr(), std::ptr::null());
        vgui.set_visible(vpanel_of(frame), false);
        Ok(frame)
    }
}

/// The form, built the first time.
unsafe fn ensure_form(vgui: &Vgui) -> Result<(*mut c_void, Vpanel), String> {
    unsafe {
        if let Some(found) = form(vgui) {
            return Ok(found);
        }
        ensure_res(false)?;
        let object = build(vgui)?;
        FORM.store(object as usize, Ordering::Release);
        form(vgui).ok_or_else(|| "the new form has no panel".to_string())
    }
}

/// Opens the form on the highlight the review is on, with its kill range and
/// note in the boxes and the keyboard in the first box.
unsafe fn open(vgui: &Vgui) -> Result<String, String> {
    let view = crate::review::form_view().ok_or("no review running")?;
    unsafe {
        let (_, vp) = ensure_form(vgui)?;
        fill(vgui, vp, &view);
        vgui.set_parent(vp, root(vgui)?);
        place(vgui, vp);
        vgui.set_visible(vp, true);
        let front: PanelFn = slot(vgui.panel, IPANEL_MOVE_TO_FRONT);
        front(vgui.panel, vp);
        let keyboard: PanelSetBoolFn = slot(vgui.panel, IPANEL_SET_KEYBOARD_INPUT_ENABLED);
        keyboard(vgui.panel, vp, 1);
        let mouse: PanelSetBoolFn = slot(vgui.panel, IPANEL_SET_MOUSE_INPUT_ENABLED);
        mouse(vgui.panel, vp, 1);
        if let Some(from) = vgui.child_named(vp, FROM) {
            let focus: SetParentFn = slot(vgui.panel, IPANEL_REQUEST_FOCUS);
            focus(vgui.panel, from, 0);
        }
        // Keys down now are released into the form, never to the game's
        // key hook: forget them there.
        crate::review_keys::forget_held();
        crate::review_form::set_shown(true);
        let note = format!(
            "opened for {} (kills {}-{} of {}); {}",
            view.title,
            view.from,
            view.to,
            view.kills,
            vgui.describe(vp)
        );
        set_showing(Some(view));
        Ok(note)
    }
}

/// The highlight's title, range, kill count and note in the form.
unsafe fn fill(vgui: &Vgui, vp: Vpanel, view: &FormView) {
    unsafe {
        let texts = [
            (TITLE, LABEL_SLOT_SET_TEXT, view.title.clone()),
            (
                KILL_COUNT,
                LABEL_SLOT_SET_TEXT,
                crate::review_form::kill_count_text(view.kills),
            ),
            (
                HINT,
                LABEL_SLOT_SET_TEXT,
                crate::review_form::KEYS_HINT.to_string(),
            ),
            (FROM, TEXT_ENTRY_SLOT_SET_TEXT, view.from.to_string()),
            (TO, TEXT_ENTRY_SLOT_SET_TEXT, view.to.to_string()),
            (NOTE, TEXT_ENTRY_SLOT_SET_TEXT, view.note.clone()),
        ];
        for (name, slot_index, text) in texts {
            review_tab::set(vgui, vp, name, slot_index, &text);
        }
    }
}

/// Centred across the screen, [`TOP_PERCENT`] of the way down. A place the
/// user dragged it to comes back with `window_layout`'s remembered layouts.
unsafe fn place(vgui: &Vgui, vp: Vpanel) {
    unsafe {
        let Ok(root) = root(vgui) else { return };
        let (_, _, screen_wide, screen_tall) = vgui.rect(root);
        let (_, _, wide, tall) = vgui.rect(vp);
        let x = ((screen_wide - wide) / 2).max(0);
        let y = (screen_tall * TOP_PERCENT / 100)
            .min(screen_tall - tall)
            .max(0);
        vgui.place(vp, (x, y, wide, tall));
    }
}

/// Reads the boxes and saves them, or says on the hint line what to type.
unsafe fn submit() {
    let Ok(vgui) = Vgui::get() else { return };
    unsafe {
        let Some((_, vp)) = form(&vgui) else { return };
        let Some(view) = showing().clone() else {
            return;
        };
        let from = review_tab::long_text(&vgui, vp, FROM);
        let to = review_tab::long_text(&vgui, vp, TO);
        let note = review_tab::long_text(&vgui, vp, NOTE);
        match crate::review_form::typed_range(&from, &to, (view.from, view.to), view.kills) {
            Ok((from, to)) => {
                crate::review_form::save(crate::review_form::Saved {
                    key: view.key,
                    from,
                    to,
                    note,
                });
                vgui.set_visible(vp, false);
                close("saved");
            }
            Err(hint) => {
                review_tab::set(&vgui, vp, HINT, LABEL_SLOT_SET_TEXT, &hint);
                trace(&format!("kept open: {hint} (typed {from:?} to {to:?})"));
            }
        }
    }
}

/// Hides the form without saving.
unsafe fn cancel() {
    let Ok(vgui) = Vgui::get() else { return };
    unsafe {
        if let Some((_, vp)) = form(&vgui) {
            vgui.set_visible(vp, false);
        }
    }
    close("cancelled");
}

/// The form's `OnKeyCodeTyped`: a key its boxes passed up. Enter saves, Esc
/// cancels, anything else goes to the class's own.
unsafe extern "thiscall" fn form_on_key(this: *mut c_void, code: i32) {
    match code {
        KEY_ENTER | KEY_PAD_ENTER => unsafe { submit() },
        KEY_ESCAPE => {
            crate::review_form::note_esc();
            unsafe { cancel() }
        }
        _ => {
            let own = OWN_ON_KEY.load(Ordering::Acquire);
            if own != 0 {
                // Safety: the class's own OnKeyCodeTyped, from its vftable.
                let own: KeyFn = unsafe { std::mem::transmute(own) };
                unsafe { own(this, code) };
            }
        }
    }
}

/// The form's `OnCommand`: its Save and Cancel buttons.
unsafe extern "thiscall" fn form_on_command(this: *mut c_void, raw: *const c_char) {
    match text(raw).as_str() {
        SAVE => unsafe { submit() },
        CANCEL => unsafe { cancel() },
        _ => {
            let own = OWN_ON_COMMAND.load(Ordering::Acquire);
            if own != 0 {
                // Safety: the class's own OnCommand, from its vftable.
                let own: OnCommandFn = unsafe { std::mem::transmute(own) };
                unsafe { own(this, raw) };
            }
        }
    }
}
