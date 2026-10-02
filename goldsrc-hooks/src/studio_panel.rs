//! `dodstudio_panel`: DoD Studio's own window inside the game (issue #408,
//! plan item 4). The first slice: a window with tabs, whose Playback tab
//! drives the demo player the way the VCR bar's buttons do.
//!
//! ## What it is
//!
//! A plain vgui2 `Frame` from `GameUI.dll`, built the way GameUI builds its
//! own dialogs: allocated with GameUI's `operator new` (so vgui's delete at
//! shutdown frees it with the matching allocator), constructed by
//! `Frame::Frame` with GameUI's base panel as its parent (so it shows and hides
//! with the menu, like the VCR bar), and laid out by
//! `EditablePanel::LoadControlSettings` from our own `.res`. Being GameUI's
//! `Frame`, it gets the ESC fix (#396, the shared `Frame::OnKeyCodeTyped`),
//! resizing with controls that follow its size (#410) and build mode
//! (Ctrl+Shift+Alt+B on the window, then Save) for free.
//!
//! The one behaviour of its own is `OnCommand` (vftable slot
//! [`FRAME_SLOT_ON_COMMAND`]): the object gets a copy of `Frame`'s vftable with
//! that slot pointing at [`hook::on_command`]. Anything it doesn't handle goes
//! to `Frame::OnCommand`, which closes the window on `Close`.
//!
//! ## Commands its buttons can send
//!
//! - `tab <name>`: shows the controls whose name starts `<name>_` and hides
//!   the other tabs' (a tab is any `tab_<name>` control in the `.res`).
//!   Controls with no tab prefix stay as they are, on every tab.
//! - The VCR bar's own commands -- `play`, `pause`, `faster`, `slower`,
//!   `stepf`, `stepb`, `start`, `end`, `stop`, `load`, `events`, `save` --
//!   are handed to the open VCR bar (`CDemoPlayerDialog::OnCommand`), so they
//!   do exactly what its buttons do. With no demo in the demo player there is
//!   no bar, and the console says so.
//! - `engine <command>`: runs a console command.
//!
//! ## The layout file
//!
//! `<game>\dod\dodstudio_ui\DodStudioPanel.res`, our own folder beside
//! `dodstudio_hd` -- never `dod\resource`, which is the user's. The default
//! (`goldsrc-hooks/ui/DodStudioPanel.res`, built into the DLL) is written there
//! the first time the window opens and never again, so build-mode edits stay.
//! `dodstudio_panel reset` puts the default back.
//!
//! ## Per build
//!
//! Five `GameUI.dll` addresses differ between the pre-Anniversary and 25th
//! Anniversary builds, and the Anniversary `Frame::Frame` takes a fourth
//! argument. [`BUILDS`] names each build by PE timestamp and image size, and
//! anything else is refused. `tools/verify_studio_panel.py` checks every
//! address against both movie installs.

// The window is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::names::console_name;

pub const NAME: &str = console_name!("panel");

/// The window's panel name, which its `.res` entry and #410's saved layout
/// (`GameUI/DodStudioPanel`) are keyed by.
const PANEL_NAME: &std::ffi::CStr = c"DodStudioPanel";
/// Where the layout lives, relative to the game directory -- the path the
/// engine's file system resolves it by, and the one build mode saves to.
const RES_PATH: &std::ffi::CStr = c"dodstudio_ui/DodStudioPanel.res";
const RES_DIR: &str = "dodstudio_ui";
const RES_FILE: &str = "DodStudioPanel.res";
const DEFAULT_RES: &str = include_str!("../ui/DodStudioPanel.res");

/// The GameUI panel the window is parented to, as the VCR bar and the main
/// menu are.
const TASKBAR: &str = "TaskBar";

/// The tab shown when the window first opens.
const FIRST_TAB: &str = "playback";

/// What the VCR bar's `OnCommand` handles, from the strings it compares
/// against (both builds' `GameUI.dll`).
const VCR_COMMANDS: &[&str] = &[
    "play", "pause", "faster", "slower", "stepf", "stepb", "start", "end", "stop", "load",
    "events", "save",
];

/// One `GameUI.dll` build: its identity and what building a `Frame` takes.
pub struct Build {
    pub name: &'static str,
    pub time_date_stamp: u32,
    pub size_of_image: u32,
    /// `operator new(size_t)`, cdecl.
    pub operator_new: usize,
    /// `Frame::Frame(Panel *parent, const char *name, bool showTaskbarIcon
    /// [, bool])`, thiscall.
    pub frame_ctor: usize,
    /// Whether `frame_ctor` takes the Anniversary build's fourth argument.
    pub frame_ctor_fourth_arg: bool,
    /// `EditablePanel::LoadControlSettings(const char *path, const char
    /// *pathID)`, thiscall.
    pub load_control_settings: usize,
    /// `sizeof(Frame)`: the first field of a subclass sits here.
    pub frame_size: usize,
}

pub const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cefc,
        size_of_image: 0xe_3000,
        operator_new: 0x7_a483,
        frame_ctor: 0x4_a610,
        frame_ctor_fourth_arg: false,
        load_control_settings: 0x4_d6b0,
        frame_size: 0x110,
    },
    Build {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_99b2,
        size_of_image: 0xd_f000,
        operator_new: 0x8_8104,
        frame_ctor: 0x5_2950,
        frame_ctor_fourth_arg: true,
        load_control_settings: 0x5_4620,
        frame_size: 0x118,
    },
];

/// `Panel::OnCommand(const char *)`, the slot the VCR bar overrides too.
const FRAME_SLOT_ON_COMMAND: usize = 87;
/// How many of `Frame`'s vftable slots the copy carries. `Frame` has about
/// 190; the rest are never called through a `Frame` pointer.
const VFTABLE_SLOTS: usize = 240;
/// `Frame::Activate()`: what GameUI calls on a dialog it has just built
/// (`jmp [vftable+0x280]`, both builds).
const FRAME_SLOT_ACTIVATE: usize = 160;
/// `Panel::GetVPanel()`, the first virtual.
const PANEL_SLOT_GET_VPANEL: usize = 0;

/// `IPanel` (`VGUI_Panel007`) slots, the same table #410 checks.
const IPANEL_GET_POS: usize = 3;
const IPANEL_GET_SIZE: usize = 5;
const IPANEL_SET_VISIBLE: usize = 14;
const IPANEL_IS_VISIBLE: usize = 15;
const IPANEL_GET_CHILD_COUNT: usize = 17;
const IPANEL_GET_CHILD: usize = 18;
const IPANEL_GET_PARENT: usize = 19;
const IPANEL_GET_NAME: usize = 36;
const IPANEL_GET_PANEL: usize = 55;
const IPANEL_GET_MODULE_NAME: usize = 59;
/// `ISurface` (`VGUI_Surface026`) slots.
const SURFACE_GET_POPUP_COUNT: usize = 69;
const SURFACE_GET_POPUP: usize = 70;

/// Our window's object, or 0 before it is built. #410 accepts it as a GameUI
/// window although its vftable is our copy.
static OBJECT: AtomicUsize = AtomicUsize::new(0);

/// The window's `Frame` object, for #410's walk.
pub fn object() -> usize {
    OBJECT.load(Ordering::Acquire)
}

/// The tab a control belongs to: the part of its name before the first `_`,
/// when that names one of `tabs`.
fn tab_of<'a>(control: &'a str, tabs: &[String]) -> Option<&'a str> {
    let (prefix, _) = control.split_once('_')?;
    tabs.iter().any(|t| t == prefix).then_some(prefix)
}

/// What a button's command asks for.
#[derive(Debug, PartialEq, Eq)]
enum Action<'a> {
    Tab(&'a str),
    Vcr(&'a str),
    Engine(&'a str),
    /// Anything else, for `Frame::OnCommand`.
    Frame,
}

fn action(command: &str) -> Action<'_> {
    if let Some(tab) = command.strip_prefix("tab ") {
        return Action::Tab(tab.trim());
    }
    if let Some(line) = command.strip_prefix("engine ") {
        return Action::Engine(line.trim());
    }
    match VCR_COMMANDS
        .iter()
        .find(|c| c.eq_ignore_ascii_case(command))
    {
        Some(c) => Action::Vcr(c),
        None => Action::Frame,
    }
}

fn res_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default()
        .join("dod")
        .join(RES_DIR)
        .join(RES_FILE)
}

/// Writes the default layout when there is none (or when `reset`), and says
/// what it did. Never touches an existing file otherwise: build-mode edits are
/// the user's.
fn ensure_res(reset: bool) -> Result<Option<String>, String> {
    let path = res_path();
    if path.exists() && !reset {
        return Ok(None);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, DEFAULT_RES)
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(Some(format!(
        "wrote the default layout to {}",
        path.display()
    )))
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{CStr, c_char, c_void};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;

    type Vpanel = u32;
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    type OperatorNewFn = unsafe extern "C" fn(usize) -> *mut c_void;
    type FrameCtor3 = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32);
    type FrameCtor4 =
        unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32, u32);
    type LoadSettingsFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char);
    type OnCommandFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char);
    type GetVpanelFn = unsafe extern "thiscall" fn(*mut c_void) -> Vpanel;
    type PanelBoolFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> u32;
    type PanelSetBoolFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, u32);
    type PanelIntFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> i32;
    type ChildFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, i32) -> Vpanel;
    type ParentFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> Vpanel;
    type StrFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> *const c_char;
    type GetPanelFn =
        unsafe extern "thiscall" fn(*mut c_void, Vpanel, *const c_char) -> *mut c_void;
    type CountFn = unsafe extern "thiscall" fn(*mut c_void) -> i32;
    type PopupFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> Vpanel;
    type ActivateFn = unsafe extern "thiscall" fn(*mut c_void);
    type GetXyFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, *mut i32, *mut i32);

    /// `Frame::OnCommand`, for whatever our handler passes on.
    static FRAME_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    /// The tab on show.
    static TAB: Mutex<String> = Mutex::new(String::new());

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

    fn interface(module: usize, name: &CStr) -> Option<*mut c_void> {
        let create =
            unsafe { GetProcAddress(module as _, c"CreateInterface".as_ptr() as *const u8) }?;
        // Safety: the interface factory's signature.
        let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
        let object = unsafe { create(name.as_ptr(), std::ptr::null_mut()) };
        (!object.is_null()).then_some(object)
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

    struct Vgui {
        panel: *mut c_void,
        surface: *mut c_void,
    }

    impl Vgui {
        fn get() -> Result<Self, String> {
            let vgui2 = module(c"vgui2.dll").ok_or("vgui2.dll is not loaded")?;
            let hw = module(c"hw.dll").ok_or("hw.dll is not loaded")?;
            Ok(Self {
                panel: interface(vgui2, c"VGUI_Panel007").ok_or("no VGUI_Panel007")?,
                surface: interface(hw, c"VGUI_Surface026").ok_or("no VGUI_Surface026")?,
            })
        }

        unsafe fn name(&self, vp: Vpanel) -> String {
            let name: StrFn = unsafe { slot(self.panel, IPANEL_GET_NAME) };
            text(unsafe { name(self.panel, vp) })
        }

        unsafe fn module_name(&self, vp: Vpanel) -> *const c_char {
            let module: StrFn = unsafe { slot(self.panel, IPANEL_GET_MODULE_NAME) };
            unsafe { module(self.panel, vp) }
        }

        unsafe fn object(&self, vp: Vpanel) -> *mut c_void {
            let get: GetPanelFn = unsafe { slot(self.panel, IPANEL_GET_PANEL) };
            unsafe { get(self.panel, vp, self.module_name(vp)) }
        }

        /// GameUI's popups (its dialogs), by panel name.
        unsafe fn gameui_popups(&self) -> Vec<(Vpanel, String)> {
            unsafe {
                let count: CountFn = slot(self.surface, SURFACE_GET_POPUP_COUNT);
                let popup: PopupFn = slot(self.surface, SURFACE_GET_POPUP);
                (0..count(self.surface).clamp(0, 256))
                    .map(|i| popup(self.surface, i))
                    .filter(|&vp| vp != 0 && text(self.module_name(vp)) == "GameUI")
                    .map(|vp| (vp, self.name(vp)))
                    .collect()
            }
        }

        /// Where a panel is and what holds it, for the log.
        unsafe fn describe(&self, vp: Vpanel) -> String {
            unsafe {
                let get_pos: GetXyFn = slot(self.panel, IPANEL_GET_POS);
                let get_size: GetXyFn = slot(self.panel, IPANEL_GET_SIZE);
                let visible: PanelBoolFn = slot(self.panel, IPANEL_IS_VISIBLE);
                let parent: ParentFn = slot(self.panel, IPANEL_GET_PARENT);
                let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
                get_pos(self.panel, vp, &mut x, &mut y);
                get_size(self.panel, vp, &mut w, &mut h);
                let p = parent(self.panel, vp);
                format!(
                    "at {x},{y} {w}x{h}, visible {}, {} control(s), parent {:?} (visible {})",
                    visible(self.panel, vp) & 0xff,
                    self.children(vp).len(),
                    self.name(p),
                    if p == 0 {
                        0
                    } else {
                        visible(self.panel, p) & 0xff
                    }
                )
            }
        }

        unsafe fn children(&self, vp: Vpanel) -> Vec<(Vpanel, String)> {
            unsafe {
                let count: PanelIntFn = slot(self.panel, IPANEL_GET_CHILD_COUNT);
                let child: ChildFn = slot(self.panel, IPANEL_GET_CHILD);
                (0..count(self.panel, vp).clamp(0, 512))
                    .map(|i| child(self.panel, vp, i))
                    .filter(|&c| c != 0)
                    .map(|c| (c, self.name(c)))
                    .collect()
            }
        }
    }

    fn gameui() -> Result<(usize, &'static Build), String> {
        let base = module(c"GameUI.dll").ok_or("GameUI.dll is not loaded yet")?;
        // Safety: a module handle the loader gave us.
        let identity = unsafe { crate::pe::image_identity(base as *mut u8) };
        BUILDS
            .iter()
            .find(|b| identity == Some((b.time_date_stamp, b.size_of_image)))
            .map(|b| (base, b))
            .ok_or_else(|| "GameUI.dll is a build this was not checked against".to_string())
    }

    /// The panel GameUI's own dialogs hang from: `TaskBar`, the parent of its
    /// main menu (`GameMenu`) and of the VCR bar on both builds (listed live,
    /// 2026-10-01). Taking just any GameUI popup's parent picked a dialog's
    /// inner panel once, and the window never showed.
    unsafe fn base_panel(vgui: &Vgui) -> Result<*mut c_void, String> {
        unsafe {
            let parent: ParentFn = slot(vgui.panel, IPANEL_GET_PARENT);
            for (vp, _) in vgui.gameui_popups() {
                let p = parent(vgui.panel, vp);
                if p == 0 || vgui.name(p) != TASKBAR || text(vgui.module_name(p)) != "GameUI" {
                    continue;
                }
                let object = vgui.object(p);
                if !object.is_null() {
                    return Ok(object);
                }
            }
        }
        Err("GameUI's menu isn't up yet -- open it (ESC) once, then try again".to_string())
    }

    unsafe fn build() -> Result<*mut c_void, String> {
        let (base, build) = gameui()?;
        let vgui = Vgui::get()?;
        let parent = unsafe { base_panel(&vgui) }?;
        unsafe {
            let new: OperatorNewFn = std::mem::transmute(base + build.operator_new);
            let object = new(build.frame_size);
            if object.is_null() {
                return Err("GameUI's operator new returned null".to_string());
            }
            std::ptr::write_bytes(object as *mut u8, 0, build.frame_size);
            if build.frame_ctor_fourth_arg {
                let ctor: FrameCtor4 = std::mem::transmute(base + build.frame_ctor);
                ctor(object, parent, PANEL_NAME.as_ptr(), 1, 0);
            } else {
                let ctor: FrameCtor3 = std::mem::transmute(base + build.frame_ctor);
                ctor(object, parent, PANEL_NAME.as_ptr(), 1);
            }

            // Our copy of Frame's vftable, RTTI locator ([-1]) included, with
            // OnCommand ours. Leaked: it lives as long as the window.
            let original = *(object as *const *const usize);
            let mut copy = vec![0usize; VFTABLE_SLOTS + 1];
            for (i, entry) in copy.iter_mut().enumerate() {
                *entry = *original.offset(i as isize - 1);
            }
            FRAME_ON_COMMAND.store(copy[FRAME_SLOT_ON_COMMAND + 1], Ordering::Release);
            copy[FRAME_SLOT_ON_COMMAND + 1] = on_command as *const () as usize;
            let copy: &'static mut [usize] = Box::leak(copy.into_boxed_slice());
            *(object as *mut *const usize) = copy.as_ptr().add(1);

            let load: LoadSettingsFn = std::mem::transmute(base + build.load_control_settings);
            load(object, RES_PATH.as_ptr(), std::ptr::null());
            Ok(object)
        }
    }

    /// The window's panel, if it still exists as the object we built.
    unsafe fn window(vgui: &Vgui) -> Option<(*mut c_void, Vpanel)> {
        let object = OBJECT.load(Ordering::Acquire) as *mut c_void;
        if object.is_null() {
            return None;
        }
        unsafe {
            let get_vpanel: GetVpanelFn = slot(object, PANEL_SLOT_GET_VPANEL);
            let vp = get_vpanel(object);
            (vp != 0 && vgui.object(vp) == object).then_some((object, vp))
        }
    }

    /// Shows `tab`'s controls and hides the other tabs'.
    unsafe fn show_tab(vgui: &Vgui, vp: Vpanel, tab: &str) {
        unsafe {
            let set_visible: PanelSetBoolFn = slot(vgui.panel, IPANEL_SET_VISIBLE);
            let children = vgui.children(vp);
            let tabs: Vec<String> = children
                .iter()
                .filter_map(|(_, n)| n.strip_prefix("tab_").map(str::to_string))
                .collect();
            for (child, name) in &children {
                if let Some(owner) = tab_of(name, &tabs)
                    && owner != "tab"
                {
                    set_visible(vgui.panel, *child, (owner == tab) as u32);
                }
            }
        }
        if let Ok(mut current) = TAB.lock() {
            *current = tab.to_string();
        }
    }

    /// Hands a VCR command to the open VCR bar.
    unsafe fn to_vcr_bar(vgui: &Vgui, command: &str) -> Result<(), String> {
        unsafe {
            let (bar, _) = vgui
                .gameui_popups()
                .into_iter()
                .find(|(_, name)| name == "DemoPlayerDialog")
                .ok_or("there's no demo in the demo player -- start one with viewdemo")?;
            let object = vgui.object(bar);
            if object.is_null() {
                return Err("the VCR bar has no object".to_string());
            }
            let on_command: OnCommandFn = slot(object, FRAME_SLOT_ON_COMMAND);
            let line = std::ffi::CString::new(command).map_err(|e| e.to_string())?;
            on_command(object, line.as_ptr());
        }
        Ok(())
    }

    /// Our `OnCommand`: vgui2 calls it for every button the window holds.
    pub(super) unsafe extern "thiscall" fn on_command(this: *mut c_void, raw: *const c_char) {
        let command = text(raw);
        let result = match (action(&command), Vgui::get()) {
            (_, Err(why)) => Err(why),
            (Action::Tab(tab), Ok(vgui)) => unsafe {
                let get_vpanel: GetVpanelFn = slot(this, PANEL_SLOT_GET_VPANEL);
                show_tab(&vgui, get_vpanel(this), tab);
                Ok(())
            },
            (Action::Vcr(c), Ok(vgui)) => unsafe { to_vcr_bar(&vgui, c) },
            (Action::Engine(line), Ok(_)) => {
                let ran = std::ffi::CString::new(format!("{line}\n"))
                    .is_ok_and(|l| crate::engine::client_cmd(&l));
                if ran {
                    Ok(())
                } else {
                    Err(format!("could not run \"{line}\""))
                }
            }
            (Action::Frame, Ok(_)) => {
                let original = FRAME_ON_COMMAND.load(Ordering::Acquire);
                if original != 0 {
                    // Safety: Frame's own OnCommand, from its vftable.
                    let original: OnCommandFn = unsafe { std::mem::transmute(original) };
                    unsafe { original(this, raw) };
                }
                Ok(())
            }
        };
        let outcome = match &result {
            Ok(()) => "done".to_string(),
            Err(why) => {
                crate::commands::console_print(&format!("{NAME}: {why}\n"));
                why.clone()
            }
        };
        unsafe {
            crate::debug::report(&format!(
                "studio_panel: button \"{}\" -- {outcome}",
                command.escape_debug()
            ))
        };
    }

    /// Opens the window (building it the first time), or closes it when open.
    pub(super) fn toggle(reset: bool) -> Result<String, String> {
        let mut notes = Vec::new();
        if let Some(note) = ensure_res(reset)? {
            notes.push(note);
        }
        let vgui = Vgui::get()?;
        unsafe {
            let (object, vp) = match window(&vgui) {
                Some(found) if !reset => found,
                _ => {
                    let object = build()?;
                    OBJECT.store(object as usize, Ordering::Release);
                    let (base, b) = gameui()?;
                    notes.push(format!(
                        "built the window ({} GameUI, object {:#x}, GameUI at {base:#x})",
                        b.name, object as usize
                    ));
                    window(&vgui).ok_or("the new window has no panel")?
                }
            };
            let is_visible: PanelBoolFn = slot(vgui.panel, IPANEL_IS_VISIBLE);
            let set_visible: PanelSetBoolFn = slot(vgui.panel, IPANEL_SET_VISIBLE);
            if is_visible(vgui.panel, vp) & 0xff != 0 && !reset {
                set_visible(vgui.panel, vp, 0);
                notes.push("closed".to_string());
            } else {
                let tab = TAB
                    .lock()
                    .ok()
                    .map(|t| t.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| FIRST_TAB.to_string());
                show_tab(&vgui, vp, &tab);
                // Frame::Activate, as GameUI opens its own dialogs: shows it,
                // brings it to the front and gives it focus.
                let activate: ActivateFn = slot(object, FRAME_SLOT_ACTIVATE);
                activate(object);
                notes.push(vgui.describe(vp));
                notes.push(format!(
                    "open on the {tab} tab (press ESC for the menu if you can't see it)"
                ));
            }
        }
        Ok(notes.join("; "))
    }
}

fn argument() -> Option<String> {
    let engfuncs = crate::engine::engfuncs()?;
    // Safety: the engine's own argument accessors, during a command.
    unsafe {
        if (engfuncs.cmd_argc)() < 2 {
            return None;
        }
        let raw = (engfuncs.cmd_argv)(1);
        (!raw.is_null()).then(|| {
            std::ffi::CStr::from_ptr(raw)
                .to_string_lossy()
                .to_ascii_lowercase()
        })
    }
}

/// `dodstudio_panel [reset]`: opens or closes the window; `reset` writes the
/// default layout back and rebuilds it.
pub unsafe extern "C" fn command() {
    let reset = argument().as_deref() == Some("reset");
    #[cfg(target_arch = "x86")]
    let result = hook::toggle(reset);
    #[cfg(not(target_arch = "x86"))]
    let result: Result<String, String> = {
        let _ = reset;
        Err("only the 32-bit build has a window".to_string())
    };
    let line = match result {
        Ok(what) => format!("{NAME}: {what}"),
        Err(why) => format!("{NAME}: {why}"),
    };
    crate::commands::console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&format!("studio_panel: {line}")) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_belongs_to_the_tab_its_name_starts_with() {
        let tabs = vec!["playback".to_string(), "demos".to_string()];
        assert_eq!(tab_of("playback_play", &tabs), Some("playback"));
        assert_eq!(tab_of("demos_load", &tabs), Some("demos"));
        // Not a tab: shown on every tab.
        assert_eq!(tab_of("footer_label", &tabs), None);
        assert_eq!(tab_of("Title", &tabs), None);
    }

    #[test]
    fn button_commands_are_sorted_into_what_they_do() {
        assert_eq!(action("tab demos"), Action::Tab("demos"));
        assert_eq!(action("play"), Action::Vcr("play"));
        assert_eq!(action("Faster"), Action::Vcr("faster"));
        assert_eq!(
            action("engine dodstudio_debug_status"),
            Action::Engine("dodstudio_debug_status")
        );
        assert_eq!(action("Close"), Action::Frame);
    }

    #[test]
    fn the_default_layout_names_every_tab_button_and_control_it_switches() {
        for tab in ["playback", "demos", "studio"] {
            assert!(DEFAULT_RES.contains(&format!("\"tab_{tab}\"")));
            assert!(DEFAULT_RES.contains(&format!("\"tab {tab}\"")));
            assert!(DEFAULT_RES.contains(&format!("\"{tab}_")));
        }
        assert!(DEFAULT_RES.contains("\"DodStudioPanel\""));
        assert!(DEFAULT_RES.contains(FIRST_TAB));
    }

    #[test]
    fn the_builds_match_the_window_layout_table() {
        for (ours, theirs) in BUILDS
            .iter()
            .zip(crate::window_layout::GAMEUI_BUILDS.iter())
        {
            assert_eq!(ours.name, theirs.name);
            assert_eq!(ours.time_date_stamp, theirs.time_date_stamp);
            assert_eq!(ours.size_of_image, theirs.size_of_image);
        }
    }
}
