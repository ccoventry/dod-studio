//! Two settings for GameUI's windows -- the console, the demo player's VCR
//! bar, the events list, the Load Demo window, Options and the rest
//! (issue #408):
//!
//! - `dodstudio_resizable_windows 1`: every one of them can be resized by
//!   its edges and corners, like the console.
//! - `dodstudio_remember_window_layout 1`: each one comes back where it was
//!   last left, at the size it was left, after the game restarts.
//!
//! ## Why neither is a `.res` setting
//!
//! A vgui2 `Frame` is resizable only if its code says so: GameUI reads no
//! "sizeable" key from a `.res` file, and most dialogs' constructors call
//! `Frame::SetSizeable(false)`. The console is resizable because its own code
//! never does.
//!
//! Positions already hold for a whole session: the VCR bar stays where it was
//! dragged, even across `viewdemo`s (tested live, pre-Anniversary, 2026-09-26).
//! What is lost is the next session. The console loads no `.res` at all, so the
//! build-mode editor's Save writes a file nothing reads, and the demo bar
//! doesn't keep its saved position either.
//!
//! ## How
//!
//! Every few frames this walks the engine surface's popups (every `Frame` is
//! one) through vgui2's own interfaces -- `VGUI_Surface026`'s
//! `GetPopupCount`/`GetPopup` and `VGUI_Panel007`'s position, size, name and
//! `GetPanel` -- and keeps only GameUI `Frame`s: the panel's vftable lies in
//! `GameUI.dll` and its `IsSizeable` slot is `Frame`'s own.
//!
//! - **Resizable:** `Frame::SetSizeable(true)` on each one that isn't already,
//!   remembering which it changed, so turning the setting off puts them back.
//!   A window's controls stretch with it only as far as their `.res`
//!   `autoResize`/`pinCorner` allow, and those can be set in build mode.
//! - **Remembered:** the first time a window is seen visible, it is moved to
//!   where it was saved under its module and panel name (and resized, when it
//!   is resizable). From then on its place is recorded, and written to
//!   `%APPDATA%\dod-studio\goldsrc_hooks_windows.txt` a few seconds after it
//!   stops changing. A saved spot partly off screen after a resolution change
//!   is pulled back on.
//!
//! Only `Frame::SetSizeable` and `Frame::IsSizeable` are per-build addresses;
//! everything else goes through the vgui2 interfaces. Each DLL is named by PE
//! timestamp and image size, and anything else is refused.
//! `tools/verify_window_layout.py` checks every slot and address against both
//! movie installs. Both settings default to 0, so nothing changes until asked.

// The addresses are only used by the 32-bit build; a host build compiles them
// for the tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::engine::CvarSPartial;
use crate::names::console_name;

pub const RESIZABLE_NAME: &str = console_name!("resizable_windows");
pub const REMEMBER_NAME: &str = console_name!("remember_window_layout");

/// The fallback path's flags, when the cvars could not be registered.
pub static RESIZABLE: AtomicBool = AtomicBool::new(false);
pub static REMEMBER: AtomicBool = AtomicBool::new(false);
static RESIZABLE_CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static REMEMBER_CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Called by `commands.rs` once the two cvars are registered.
pub fn set_cvars(resizable: *mut CvarSPartial, remember: *mut CvarSPartial) {
    RESIZABLE_CVAR.store(resizable, Ordering::Release);
    REMEMBER_CVAR.store(remember, Ordering::Release);
}

fn flag(cvar: &AtomicPtr<CvarSPartial>, fallback: &AtomicBool) -> bool {
    let cvar = cvar.load(Ordering::Acquire);
    if cvar.is_null() {
        fallback.load(Ordering::Relaxed)
    } else {
        // Safety: the engine owns the cvar for the session.
        unsafe { (*cvar).value != 0.0 }
    }
}

fn resizable() -> bool {
    flag(&RESIZABLE_CVAR, &RESIZABLE)
}

fn remember() -> bool {
    flag(&REMEMBER_CVAR, &REMEMBER)
}

/// For the fallback toggle commands' bare-name query.
pub fn resizable_status() -> String {
    if resizable() {
        "GameUI's windows can be resized".to_string()
    } else {
        "only the windows the game makes resizable can be resized".to_string()
    }
}

pub fn remember_status() -> String {
    if remember() {
        "GameUI's windows come back where they were left".to_string()
    } else {
        "GameUI's windows open where the game puts them".to_string()
    }
}

/// One DLL build: its identity.
pub struct DllBuild {
    pub name: &'static str,
    pub time_date_stamp: u32,
    pub size_of_image: u32,
}

/// One `GameUI.dll` build: its identity and `Frame`'s two sizeable methods.
pub struct GameUiBuild {
    pub name: &'static str,
    pub time_date_stamp: u32,
    pub size_of_image: u32,
    /// `Frame::SetSizeable(bool)`, `thiscall`.
    pub set_sizeable: usize,
    /// `Frame::IsSizeable()`, the function `Frame`'s vftable holds at
    /// [`FRAME_SLOT_IS_SIZEABLE`].
    pub is_sizeable: usize,
}

pub const GAMEUI_BUILDS: [GameUiBuild; 2] = [
    GameUiBuild {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cefc,
        size_of_image: 0xe_3000,
        set_sizeable: 0x4_c830,
        is_sizeable: 0x4_c860,
    },
    GameUiBuild {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_99b2,
        size_of_image: 0xd_f000,
        set_sizeable: 0x5_5970,
        is_sizeable: 0x5_4520,
    },
];

pub const VGUI2_BUILDS: [DllBuild; 2] = [
    DllBuild {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cd68,
        size_of_image: 0x3_d000,
    },
    DllBuild {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_91ca,
        size_of_image: 0x2_e000,
    },
];

pub const HW_BUILDS: [DllBuild; 2] = [
    DllBuild {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f3d_8dcc,
        size_of_image: 0x125_9000,
    },
    DllBuild {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_93da,
        size_of_image: 0x14a_a000,
    },
];

/// `Frame`'s `IsSizeable` slot (`+0x2a0`).
const FRAME_SLOT_IS_SIZEABLE: usize = 168;
/// `IPanel` (`VGUI_Panel007`) slots.
const PANEL_SLOT_SET_POS: usize = 2;
const PANEL_SLOT_GET_POS: usize = 3;
const PANEL_SLOT_SET_SIZE: usize = 4;
const PANEL_SLOT_GET_SIZE: usize = 5;
const PANEL_SLOT_IS_VISIBLE: usize = 15;
const PANEL_SLOT_GET_NAME: usize = 36;
const PANEL_SLOT_GET_PANEL: usize = 55;
const PANEL_SLOT_GET_MODULE_NAME: usize = 59;
/// `ISurface` (`VGUI_Surface026`) slots.
const SURFACE_SLOT_GET_SCREEN_SIZE: usize = 32;
const SURFACE_SLOT_GET_POPUP_COUNT: usize = 69;
const SURFACE_SLOT_GET_POPUP: usize = 70;

/// Walk the popups every this many frames.
const POLL_EVERY: u64 = 4;
/// Write the file once nothing has moved for this many walks (~2 s at 120 fps).
const SAVE_AFTER_STILL_POLLS: u64 = 60;
/// How much of a window must stay on screen after a restore.
const KEEP_ON_SCREEN: i32 = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// `module/name<TAB>x y w h`, one window per line.
fn parse(text: &str) -> BTreeMap<String, Rect> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some((key, numbers)) = line.split_once('\t') else {
            continue;
        };
        let n: Vec<i32> = numbers
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        if let [x, y, w, h] = n[..]
            && w > 0
            && h > 0
        {
            out.insert(key.to_string(), Rect { x, y, w, h });
        }
    }
    out
}

fn render(saved: &BTreeMap<String, Rect>) -> String {
    saved
        .iter()
        .map(|(key, r)| format!("{key}\t{} {} {} {}\n", r.x, r.y, r.w, r.h))
        .collect()
}

/// `saved`, moved (never resized) so at least [`KEEP_ON_SCREEN`] pixels of it
/// are on a `screen_w` x `screen_h` screen, title bar included.
fn on_screen(saved: Rect, screen_w: i32, screen_h: i32) -> Rect {
    let keep_w = KEEP_ON_SCREEN.min(saved.w);
    Rect {
        x: saved.x.clamp(keep_w - saved.w, (screen_w - keep_w).max(0)),
        y: saved.y.clamp(0, (screen_h - KEEP_ON_SCREEN).max(0)),
        ..saved
    }
}

fn layout_path() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    let appdata = std::env::var_os("APPDATA")?;
    let dir = std::path::PathBuf::from(appdata).join("dod-studio");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("goldsrc_hooks_windows.txt"))
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashSet};
    use std::ffi::{CStr, c_char, c_void};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;

    type Vpanel = u32;
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    // A `bool` comes back in `al`; read all of `eax` and keep the low byte.
    type BoolOfPanelFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> u32;
    type XyFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, i32, i32);
    type GetXyFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, *mut i32, *mut i32);
    type StrOfPanelFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> *const c_char;
    type GetPanelFn =
        unsafe extern "thiscall" fn(*mut c_void, Vpanel, *const c_char) -> *mut c_void;
    type CountFn = unsafe extern "thiscall" fn(*mut c_void) -> i32;
    type PopupFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> Vpanel;
    type ScreenFn = unsafe extern "thiscall" fn(*mut c_void, *mut i32, *mut i32);
    type SetSizeableFn = unsafe extern "thiscall" fn(*mut c_void, u32);
    type IsSizeableFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;

    struct Api {
        panel: *mut c_void,
        surface: *mut c_void,
        gameui: (usize, usize),
        set_sizeable: usize,
        is_sizeable: usize,
        /// Which builds were recognised, for the log.
        builds: String,
    }

    struct Window {
        key: String,
        restored: bool,
        /// Set when this module made it resizable, so turning the setting off
        /// can put it back.
        made_sizeable: bool,
    }

    enum Install {
        Waiting,
        Ready(Api),
        Refused,
    }

    struct State {
        install: Install,
        frame: u64,
        saved: Option<BTreeMap<String, Rect>>,
        windows: BTreeMap<Vpanel, Window>,
        changed_at: Option<u64>,
    }

    thread_local! {
        // Only the engine's main thread polls, so no lock is needed.
        static STATE: RefCell<State> = const {
            RefCell::new(State {
                install: Install::Waiting,
                frame: 0,
                saved: None,
                windows: BTreeMap::new(),
                changed_at: None,
            })
        };
    }

    unsafe fn slot<F: Copy>(object: *mut c_void, index: usize) -> F {
        unsafe {
            let vftable = *(object as *const *const usize);
            std::mem::transmute_copy(&*vftable.add(index))
        }
    }

    fn module(name: &CStr) -> Option<usize> {
        let handle = unsafe { GetModuleHandleA(name.as_ptr() as *const u8) };
        (!handle.is_null()).then_some(handle as usize)
    }

    /// The build name of the loaded module, from `table` by its identity.
    fn identified(
        base: usize,
        table: &[(&'static str, u32, u32)],
        dll: &str,
    ) -> Result<&'static str, String> {
        // Safety: a module handle the loader gave us.
        let identity = unsafe { crate::pe::image_identity(base as *mut u8) };
        table
            .iter()
            .find(|(_, stamp, size)| identity == Some((*stamp, *size)))
            .map(|(name, _, _)| *name)
            .ok_or_else(|| format!("{dll} is a build this was not checked against"))
    }

    fn interface(module: usize, name: &CStr) -> Option<*mut c_void> {
        let create =
            unsafe { GetProcAddress(module as _, c"CreateInterface".as_ptr() as *const u8) }?;
        // Safety: the interface factory's signature, on a checked build.
        let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
        let object = unsafe { create(name.as_ptr(), std::ptr::null_mut()) };
        (!object.is_null()).then_some(object)
    }

    fn install() -> Option<Result<Api, String>> {
        let (gameui, vgui2, hw) = (
            module(c"GameUI.dll")?,
            module(c"vgui2.dll")?,
            module(c"hw.dll")?,
        );
        Some((|| {
            let ids = |table: &[DllBuild]| -> Vec<(&'static str, u32, u32)> {
                table
                    .iter()
                    .map(|b| (b.name, b.time_date_stamp, b.size_of_image))
                    .collect()
            };
            let ui_name = identified(
                gameui,
                &GAMEUI_BUILDS
                    .iter()
                    .map(|b| (b.name, b.time_date_stamp, b.size_of_image))
                    .collect::<Vec<_>>(),
                "GameUI.dll",
            )?;
            let ui = GAMEUI_BUILDS
                .iter()
                .find(|b| b.name == ui_name)
                .ok_or("GameUI.dll's build is missing from the table")?;
            let vgui2_name = identified(vgui2, &ids(&VGUI2_BUILDS), "vgui2.dll")?;
            let hw_name = identified(hw, &ids(&HW_BUILDS), "hw.dll")?;
            let panel =
                interface(vgui2, c"VGUI_Panel007").ok_or("no VGUI_Panel007 from vgui2.dll")?;
            let surface =
                interface(hw, c"VGUI_Surface026").ok_or("no VGUI_Surface026 from hw.dll")?;
            // Safety: a module handle the loader gave us.
            let size = unsafe { crate::pe::image_identity(gameui as *mut u8) }
                .map_or(0, |(_, size)| size as usize);
            Ok(Api {
                panel,
                surface,
                gameui: (gameui, gameui + size),
                set_sizeable: gameui + ui.set_sizeable,
                is_sizeable: gameui + ui.is_sizeable,
                builds: format!("{ui_name} GameUI, {vgui2_name} vgui2, {hw_name} engine"),
            })
        })())
    }

    fn text(raw: *const c_char) -> String {
        if raw.is_null() {
            String::new()
        } else {
            // Safety: a NUL-terminated string the panel system owns.
            unsafe { CStr::from_ptr(raw) }
                .to_string_lossy()
                .into_owned()
        }
    }

    impl Api {
        /// The GameUI `Frame` behind `vp`, or `None` for any other popup.
        unsafe fn frame(&self, vp: Vpanel) -> Option<*mut c_void> {
            unsafe {
                let module: StrOfPanelFn = slot(self.panel, PANEL_SLOT_GET_MODULE_NAME);
                let get_panel: GetPanelFn = slot(self.panel, PANEL_SLOT_GET_PANEL);
                let object = get_panel(self.panel, vp, module(self.panel, vp));
                if object.is_null() {
                    return None;
                }
                let vftable = *(object as *const usize);
                if vftable < self.gameui.0
                    || vftable + (FRAME_SLOT_IS_SIZEABLE + 1) * 4 > self.gameui.1
                {
                    return None;
                }
                let is_sizeable = *((vftable + FRAME_SLOT_IS_SIZEABLE * 4) as *const usize);
                (is_sizeable == self.is_sizeable).then_some(object)
            }
        }

        unsafe fn sizeable(&self, frame: *mut c_void) -> bool {
            let is_sizeable: IsSizeableFn = unsafe { std::mem::transmute(self.is_sizeable) };
            unsafe { is_sizeable(frame) & 0xff != 0 }
        }

        unsafe fn set_sizeable(&self, frame: *mut c_void, on: bool) {
            let set: SetSizeableFn = unsafe { std::mem::transmute(self.set_sizeable) };
            unsafe { set(frame, on as u32) };
        }

        unsafe fn rect(&self, vp: Vpanel) -> Rect {
            let (mut r, mut w, mut h) = (
                Rect {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
                0,
                0,
            );
            unsafe {
                let get_pos: GetXyFn = slot(self.panel, PANEL_SLOT_GET_POS);
                let get_size: GetXyFn = slot(self.panel, PANEL_SLOT_GET_SIZE);
                get_pos(self.panel, vp, &mut r.x, &mut r.y);
                get_size(self.panel, vp, &mut w, &mut h);
            }
            Rect { w, h, ..r }
        }
    }

    fn walk(state: &mut State, api: &Api) {
        let (resizable, remember) = (resizable(), remember());
        if !resizable && !remember && state.windows.values().all(|w| !w.made_sizeable) {
            state.windows.clear();
            return;
        }
        if remember && state.saved.is_none() {
            let text = layout_path()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default();
            state.saved = Some(parse(&text));
        }
        let mut present = HashSet::new();
        // Safety: slots of interfaces checked by tools/verify_window_layout.py
        // on exactly these builds; every panel comes from the surface itself.
        unsafe {
            let count: CountFn = slot(api.surface, SURFACE_SLOT_GET_POPUP_COUNT);
            let popup: PopupFn = slot(api.surface, SURFACE_SLOT_GET_POPUP);
            let visible: BoolOfPanelFn = slot(api.panel, PANEL_SLOT_IS_VISIBLE);
            let name: StrOfPanelFn = slot(api.panel, PANEL_SLOT_GET_NAME);
            let module: StrOfPanelFn = slot(api.panel, PANEL_SLOT_GET_MODULE_NAME);
            let set_pos: XyFn = slot(api.panel, PANEL_SLOT_SET_POS);
            let set_size: XyFn = slot(api.panel, PANEL_SLOT_SET_SIZE);
            let screen: ScreenFn = slot(api.surface, SURFACE_SLOT_GET_SCREEN_SIZE);

            for i in 0..count(api.surface).clamp(0, 256) {
                let vp = popup(api.surface, i);
                if vp == 0 {
                    continue;
                }
                let Some(frame) = api.frame(vp) else {
                    continue;
                };
                present.insert(vp);
                let window = state.windows.entry(vp).or_insert_with(|| Window {
                    key: format!(
                        "{}/{}",
                        text(module(api.panel, vp)),
                        text(name(api.panel, vp))
                    ),
                    restored: false,
                    made_sizeable: false,
                });

                if resizable && !api.sizeable(frame) {
                    api.set_sizeable(frame, true);
                    window.made_sizeable = true;
                } else if !resizable && window.made_sizeable {
                    api.set_sizeable(frame, false);
                    window.made_sizeable = false;
                }

                if !remember || visible(api.panel, vp) & 0xff == 0 {
                    continue;
                }
                let Some(saved) = state.saved.as_mut() else {
                    continue;
                };
                if !window.restored {
                    window.restored = true;
                    if let Some(&want) = saved.get(&window.key) {
                        let (mut sw, mut sh) = (0, 0);
                        screen(api.surface, &mut sw, &mut sh);
                        let at = on_screen(want, sw, sh);
                        if api.sizeable(frame) {
                            set_size(api.panel, vp, at.w, at.h);
                        }
                        set_pos(api.panel, vp, at.x, at.y);
                        crate::debug::report(&format!(
                            "window_layout: put {} back at {},{} {}x{}",
                            window.key, at.x, at.y, at.w, at.h
                        ));
                    }
                    continue;
                }
                let now = api.rect(vp);
                if now.w > 0 && now.h > 0 && saved.get(&window.key) != Some(&now) {
                    saved.insert(window.key.clone(), now);
                    state.changed_at = Some(state.frame);
                }
            }
        }
        state.windows.retain(|vp, _| present.contains(vp));

        if let Some(at) = state.changed_at
            && state.frame >= at + SAVE_AFTER_STILL_POLLS * POLL_EVERY
        {
            state.changed_at = None;
            if let (Some(path), Some(saved)) = (layout_path(), state.saved.as_ref()) {
                let written = std::fs::write(&path, render(saved)).is_ok();
                unsafe {
                    crate::debug::report(&format!(
                        "window_layout: {} {} window position(s) to {}",
                        if written { "saved" } else { "could not save" },
                        saved.len(),
                        path.display()
                    ))
                };
            }
        }
    }

    pub(super) fn poll() {
        STATE.with(|cell| {
            let Ok(mut state) = cell.try_borrow_mut() else {
                return;
            };
            state.frame += 1;
            if state.frame % POLL_EVERY != 0 {
                return;
            }
            if let Install::Waiting = state.install {
                match install() {
                    None => return, // a DLL isn't loaded yet
                    Some(Ok(api)) => {
                        unsafe {
                            crate::debug::report(&format!(
                                "window_layout: ready ({}) -- {RESIZABLE_NAME} and {REMEMBER_NAME} (#408)",
                                api.builds
                            ))
                        };
                        state.install = Install::Ready(api);
                    }
                    Some(Err(why)) => {
                        unsafe {
                            crate::debug::report(&format!("window_layout: not installed -- {why}"))
                        };
                        state.install = Install::Refused;
                    }
                }
            }
            // Taken out and put back so `walk` can borrow the rest of the state.
            let install = std::mem::replace(&mut state.install, Install::Waiting);
            if let Install::Ready(api) = &install {
                walk(&mut state, api);
            }
            state.install = install;
        });
    }
}

/// Walks GameUI's windows every few frames, from `commands::poll`. Waits for
/// `GameUI.dll`, `vgui2.dll` and `hw.dll`, and costs a cvar read or two while
/// both settings are off.
pub fn poll() {
    #[cfg(target_arch = "x86")]
    hook::poll();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_file_round_trips() {
        let mut saved = BTreeMap::new();
        saved.insert(
            "GameUI/DemoPlayerDialog".to_string(),
            Rect {
                x: 10,
                y: -4,
                w: 700,
                h: 120,
            },
        );
        saved.insert(
            "GameUI/GameConsole".to_string(),
            Rect {
                x: 0,
                y: 0,
                w: 640,
                h: 480,
            },
        );
        assert_eq!(parse(&render(&saved)), saved);
    }

    #[test]
    fn a_bad_line_is_skipped_not_fatal() {
        let parsed = parse("junk\nGameUI/A\t1 2 3\nGameUI/B\t1 2 0 5\nGameUI/C\t1 2 3 4\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed["GameUI/C"],
            Rect {
                x: 1,
                y: 2,
                w: 3,
                h: 4
            }
        );
    }

    #[test]
    fn a_window_saved_off_screen_comes_back_on() {
        let r = |x, y| Rect {
            x,
            y,
            w: 700,
            h: 120,
        };
        // Inside: untouched.
        assert_eq!(on_screen(r(100, 100), 1920, 1080), r(100, 100));
        // Past the right edge of a smaller screen: pulled back, keeping 48 px.
        assert_eq!(on_screen(r(2400, 100), 1280, 720), r(1280 - 48, 100));
        // Off the left: at least 48 px stay visible.
        assert_eq!(on_screen(r(-5000, 100), 1920, 1080), r(48 - 700, 100));
        // Title bar above the top, or below the bottom: pulled back.
        assert_eq!(on_screen(r(100, -30), 1920, 1080), r(100, 0));
        assert_eq!(on_screen(r(100, 5000), 1920, 1080), r(100, 1080 - 48));
    }

    #[test]
    fn no_name_is_the_start_of_another() {
        assert!(!RESIZABLE_NAME.starts_with(REMEMBER_NAME));
        assert!(!REMEMBER_NAME.starts_with(RESIZABLE_NAME));
    }

    #[test]
    fn the_builds_are_told_apart() {
        for pair in [
            (
                GAMEUI_BUILDS[0].time_date_stamp,
                GAMEUI_BUILDS[1].time_date_stamp,
            ),
            (
                VGUI2_BUILDS[0].time_date_stamp,
                VGUI2_BUILDS[1].time_date_stamp,
            ),
            (HW_BUILDS[0].time_date_stamp, HW_BUILDS[1].time_date_stamp),
        ] {
            assert_ne!(pair.0, pair.1);
        }
    }
}
