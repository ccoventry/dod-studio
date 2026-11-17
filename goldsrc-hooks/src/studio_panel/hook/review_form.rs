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
//! Hidden, the keys are the game's again. Real clicks reach its boxes and
//! buttons on both builds once the boxes fit their font (below; #1311's
//! test), and everything works from the keyboard too: Tab moves between the
//! boxes (`tabPosition` in the layout), Enter saves, Esc cancels.
//!
//! ## Boxes taller than their font
//!
//! A text box shows `tall / (font tall + 1)` lines, and a click on one that
//! shows none reads past its line-break list: on the Anniversary build, whose
//! font grows with the screen (24 px at 720 lines, as tall as the layout
//! file's boxes), that put up GameUI's "Assertion failed" dialogs (#1311).
//! The font is only known once the shown form's first layout has given the
//! boxes theirs, so from then on each frame [`fit_then_take_clicks`] grows
//! any box shorter than its font + `BOX_SPARE_PX`, with its row, moves the
//! rows below down and lengthens the form, and only then lets the form take
//! clicks. A box a user's layout leaves out is left out of the fit; one that
//! isn't a plain `TextEntry` keeps the form keyboard-only. Either is said
//! once in the hook log.
//!
//! ## Keys and buttons
//!
//! A box passes every key press up to the window (`TextEntry` doesn't override
//! `OnKeyCodePressed`, so `Panel`'s forwards it to the parent), whose
//! `OnKeyCodePressed` (slot [`PANEL_SLOT_ON_KEY_CODE_PRESSED`]) is ours in a
//! copied vftable; its Save and Cancel buttons send `save` and `cancel` to
//! its `OnCommand`, ours too. A kill range outside the highlight keeps the
//! form open with what to type on its last line.
//!
//! ## When it closes by itself
//!
//! When the review moves to another highlight or ends, when a demo load
//! ends the clip it is for, and when the menu comes up (the form isn't part
//! of it, so it would sit on top): each cancels, as Esc does. The form keeps
//! no draft, so a demo load says in the console that what was typed wasn't
//! saved. The engine brings the menu up on the very Esc the
//! form cancels with, so a menu that comes up just after the form's Esc, or
//! while it shows, is put away again: Esc on the form means the form.

use crate::lock::{read_or_recover, write_or_recover};
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use crate::deathmsg::font::hook::{FontTallFn, SURFACE_GET_FONT_TALL};
use crate::review_form::{BoxCheck, BoxFont, FormView, LaidOutControl};

/// The window's panel name, which its `.res` entry is keyed by.
const FORM_NAME: &CStr = c"DodStudioReviewForm";
const TITLE: &str = "FormTitle";
const FROM: &str = "FormFrom";
const TO: &str = "FormTo";
const KILL_COUNT: &str = "FormKillCount";
const NOTE: &str = "FormNote";
/// The text boxes, sized to their font ([`fit_then_take_clicks`]).
const BOXES: [&str; 3] = [FROM, TO, NOTE];
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
/// Whether the shown form's boxes were fitted to their fonts since it opened
/// ([`fit_then_take_clicks`]), so it isn't tried again.
static FITTED: AtomicBool = AtomicBool::new(false);
/// The class's own `OnKeyCodePressed` and `OnCommand`, for what isn't ours.
static OWN_ON_KEY_PRESSED: AtomicUsize = AtomicUsize::new(0);
static OWN_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
/// The highlight the form shows, while it shows. Read every frame it shows,
/// so an `RwLock` (CLAUDE.md: no blocking mutex per frame).
static SHOWING: std::sync::RwLock<Option<FormView>> = std::sync::RwLock::new(None);

fn trace(line: &str) {
    unsafe { crate::debug::report(&format!("review_form: {line}")) };
}

fn showing() -> std::sync::RwLockReadGuard<'static, Option<FormView>> {
    read_or_recover(&SHOWING)
}

fn set_showing(view: Option<FormView>) {
    *write_or_recover(&SHOWING) = view;
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
                    } else if !FITTED.load(Ordering::Acquire) {
                        fit_then_take_clicks(vgui, vp);
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
        if crate::review_form::demo_load_ended(demo_playing()) {
            crate::commands::console_print(&format!(
                "{}: closed the kill range and note form without saving: a demo load ended its highlight's clip\n",
                crate::review::NAME
            ));
            return Some("a demo load ended its highlight's clip");
        }
        if menu_up(vgui) {
            // Esc, whichever of the form and the engine saw it first.
            crate::review_form::note_esc();
            return Some("the menu came up");
        }
        None
    }
}

/// Whether a demo is in the player and read.
fn demo_playing() -> bool {
    crate::demo_seek::clock().is_some_and(|c| c.active && !c.loading)
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
                    PANEL_SLOT_ON_KEY_CODE_PRESSED,
                    form_on_key_pressed as *const () as usize,
                ),
            ],
        );
        OWN_ON_COMMAND.store(own[0], Ordering::Release);
        OWN_ON_KEY_PRESSED.store(own[1], Ordering::Release);
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
        let found = form(vgui).ok_or_else(|| "the new form has no panel".to_string())?;
        scale_layout(vgui, found.1);
        Ok(found)
    }
}

/// The controls `ReviewForm.res` lays out, by name: the ones to scale. The
/// window's own title bar and grips are the frame's to lay out.
fn laid_out_controls() -> impl Iterator<Item = &'static str> {
    FORM_RES
        .2
        .lines()
        .filter_map(|l| l.trim().strip_prefix("\"fieldName\""))
        .map(|v| v.trim().trim_matches('"'))
        .filter(|name| FORM_NAME.to_str() != Ok(*name))
}

/// Grows the new form and its controls with the font on a GameUI that
/// scales its font with the screen (`review_form::layout_scale`): the layout
/// file's sizes are the pre-Anniversary font's.
unsafe fn scale_layout(vgui: &Vgui, vp: Vpanel) {
    unsafe {
        let Ok((_, build)) = gameui() else { return };
        let Ok(root) = root(vgui) else { return };
        let (_, _, _, screen_tall) = vgui.rect(root);
        let scale = crate::review_form::layout_scale(screen_tall, build.font_scales_with_screen);
        if scale == (1, 1) {
            return;
        }
        let (x, y, wide, tall) = vgui.rect(vp);
        let (_, _, wide, tall) = crate::review_form::scaled_rect((0, 0, wide, tall), scale);
        vgui.place(vp, (x, y, wide, tall));
        let mut scaled = 0;
        for name in laid_out_controls() {
            if let Some(child) = vgui.child_named(vp, name) {
                let rect = crate::review_form::scaled_rect(vgui.rect(child), scale);
                vgui.place(child, rect);
                scaled += 1;
            }
        }
        trace(&format!(
            "scaled by {}/{} for a font that grows with the screen: {wide}x{tall}, {scaled} control(s)",
            scale.0, scale.1
        ));
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
        // No clicks until the boxes fit their fonts ([`fit_then_take_clicks`]): vgui2
        // routes no click to a panel, or its children, that takes no mouse
        // input, and a click on a box too short for its font trips GameUI's
        // asserts.
        FITTED.store(false, Ordering::Release);
        take_clicks(vgui, vp, false);
        if let Some(from) = vgui.child_named(vp, FROM) {
            let focus: SetParentFn = slot(vgui.panel, IPANEL_REQUEST_FOCUS);
            focus(vgui.panel, from, 0);
        }
        // Keys down now are released into the form, never to the game's
        // key hook: forget them there.
        crate::review_keys::forget_held();
        crate::review_form::note_demo_at_open(demo_playing());
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

/// Whether the form, and so its boxes and buttons, takes clicks.
unsafe fn take_clicks(vgui: &Vgui, vp: Vpanel, on: bool) {
    unsafe {
        let mouse: PanelSetBoolFn = slot(vgui.panel, IPANEL_SET_MOUSE_INPUT_ENABLED);
        mouse(vgui.panel, vp, on as u32);
    }
}

/// One control the layout file names, as the shown form has it.
struct FormControl {
    panel: Vpanel,
    laid_out: LaidOutControl,
}

/// Fits the boxes to their font once every box's font is known, says so in
/// the hook log, and then lets the form take clicks, unless a box couldn't be
/// read (`review_form::check_boxes`). Does nothing while a box still waits
/// for its font; after a fit, [`FITTED`] stops it running again.
unsafe fn fit_then_take_clicks(vgui: &Vgui, vp: Vpanel) {
    unsafe {
        let fonts: Vec<(&str, BoxFont)> = BOXES
            .iter()
            .map(|&name| (name, box_font(vgui, vp, name)))
            .collect();
        let BoxCheck::Ready {
            left_out,
            takes_clicks,
        } = crate::review_form::check_boxes(&fonts)
        else {
            return;
        };
        let controls = read_controls(vgui, vp, &fonts);
        let laid_out: Vec<LaidOutControl> = controls.iter().map(|c| c.laid_out).collect();
        let fit = crate::review_form::fit_boxes(&laid_out);
        if fit.form_grow > 0 {
            place_fitted(vgui, vp, &controls, &fit);
        }
        trace(&fit_note(&controls, &fit, vgui.rect(vp).3));
        if let Some(left_out) = left_out {
            let clicks = if takes_clicks {
                ""
            } else {
                "; the form stays keyboard-only"
            };
            trace(&format!("left out of the box fit: {left_out}{clicks}"));
        }
        take_clicks(vgui, vp, takes_clicks);
        FITTED.store(true, Ordering::Release);
    }
}

/// What the form's control `name` is, as a text box to fit: its font's
/// height once its first layout has given it a font.
unsafe fn box_font(vgui: &Vgui, vp: Vpanel, name: &str) -> BoxFont {
    unsafe {
        let Some(child) = vgui.child_named(vp, name) else {
            return BoxFont::Missing;
        };
        let Ok((base, build)) = gameui() else {
            return BoxFont::NotTextEntry;
        };
        let object = vgui.object(child);
        if object.is_null() || *(object as *const usize) != base + build.text_entry_vftable {
            return BoxFont::NotTextEntry;
        }
        let font = *((object as usize + build.text_entry_font) as *const u32);
        if font == 0 {
            return BoxFont::NotKnownYet;
        }
        let font_tall: FontTallFn = slot(vgui.surface, SURFACE_GET_FONT_TALL);
        match font_tall(vgui.surface, font) {
            tall if tall > 0 => BoxFont::Tall(tall),
            _ => BoxFont::NotKnownYet,
        }
    }
}

/// The form's laid-out controls as they are now, each box with its font
/// from `fonts`.
unsafe fn read_controls(vgui: &Vgui, vp: Vpanel, fonts: &[(&str, BoxFont)]) -> Vec<FormControl> {
    unsafe {
        laid_out_controls()
            .filter_map(|name| {
                let panel = vgui.child_named(vp, name)?;
                let font_tall = fonts
                    .iter()
                    .find(|&&(box_name, _)| box_name == name)
                    .and_then(|&(_, font)| font.tall());
                Some(FormControl {
                    panel,
                    laid_out: LaidOutControl {
                        rect: vgui.rect(panel),
                        font_tall,
                    },
                })
            })
            .collect()
    }
}

/// Moves the controls where `fit` placed them and lengthens the form.
unsafe fn place_fitted(
    vgui: &Vgui,
    vp: Vpanel,
    controls: &[FormControl],
    fit: &crate::review_form::Fit,
) {
    unsafe {
        for (control, &rect) in controls.iter().zip(&fit.placed) {
            vgui.place(control.panel, rect);
        }
        let (x, y, wide, tall) = vgui.rect(vp);
        vgui.place(vp, (x, y, wide, tall + fit.form_grow));
    }
}

/// The hook log's line for a fit: each box's height against its font's, and
/// the form's new height when it grew.
fn fit_note(controls: &[FormControl], fit: &crate::review_form::Fit, form_tall: i32) -> String {
    let boxes: Vec<String> = controls
        .iter()
        .zip(&fit.placed)
        .filter_map(|(control, &(_, _, _, tall))| {
            let font = control.laid_out.font_tall?;
            Some(format!("{tall} px for a {font} px font"))
        })
        .collect();
    match fit.form_grow {
        0 => format!("the boxes fit their font: {}", boxes.join(", ")),
        grow => format!(
            "made the boxes taller than their font, the form {grow} px taller ({form_tall} px): {}",
            boxes.join(", ")
        ),
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

/// The form's `OnKeyCodePressed`: a key its boxes passed up. Enter saves, Esc
/// cancels, anything else goes to the class's own.
unsafe extern "thiscall" fn form_on_key_pressed(this: *mut c_void, code: i32) {
    match code {
        KEY_ENTER | KEY_PAD_ENTER => unsafe { submit() },
        KEY_ESCAPE => {
            crate::review_form::note_esc();
            unsafe { cancel() }
        }
        _ => {
            let own = OWN_ON_KEY_PRESSED.load(Ordering::Acquire);
            if own != 0 {
                // Safety: the class's own OnKeyCodePressed, from its vftable.
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
