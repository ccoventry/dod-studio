//! `dodstudio_panel`: DoD Studio's own window inside the game (issue #408,
//! plan item 4). A window with real tabs, like the Options and Find Servers
//! windows; its Playback tab drives the demo player the way the VCR bar's
//! buttons do.
//!
//! ## What it is
//!
//! Three kinds of GameUI vgui2 panel, built the way GameUI builds its own
//! Options dialog (`COptionsDialog`, a `PropertyDialog`):
//!
//! - **The window**, a `Frame`: allocated with GameUI's `operator new` (so
//!   vgui's delete at shutdown frees it with the matching allocator),
//!   constructed by `Frame::Frame` with `TaskBar` as its parent (the panel the
//!   main menu and the VCR bar hang from, so it shows and hides with the menu),
//!   laid out by `EditablePanel::LoadControlSettings` from `DodStudio.res`, and
//!   shown by `Frame::Activate`.
//! - **The tab strip**, a `PropertySheet` (`PropertySheet::PropertySheet`, the
//!   object a `PropertyDialog` keeps for its pages), sized to the window's
//!   client area every frame by [`poll`] -- what `PropertyDialog::PerformLayout`
//!   does for its own sheet.
//! - **One `PropertyPage` per tab** in [`PAGES`], each laid out by its own
//!   `.res` and added with `PropertySheet::AddPage(page, title)`, as
//!   `COptionsDialog` adds Keyboard, Mouse and the rest. The sheet draws the
//!   tabs and switches pages itself.
//!
//! Being GameUI's `Frame`, the window gets the ESC fix (#396, the shared
//! `Frame::OnKeyCodeTyped`), resizing and remembered placement (#410) and build
//! mode (Ctrl+Shift+Alt+B on a tab, then Save, which writes that tab's `.res`).
//!
//! ## Buttons
//!
//! A button sends its `Command` to the panel it sits on. `PropertyPage`'s own
//! `OnCommand` is an empty `ret 4` on both builds, so the window and its pages
//! each get a copy of their class's vftable with `OnCommand` (slot
//! [`FRAME_SLOT_ON_COMMAND`]) pointing at our handler. A command can be:
//!
//! - one of the VCR bar's own -- `play`, `pause`, `faster`, `slower`,
//!   `stepf`, `stepb`, `start`, `end`, `stop`, `load`, `events`, `save` --
//!   handed to the open VCR bar (`CDemoPlayerDialog::OnCommand`), so it does
//!   exactly what the bar's button does. With no demo in the demo player
//!   there is no bar, and the console says so;
//! - `engine <command>`, run as a console command;
//! - anything else, passed to the class's own `OnCommand` (`Close` on the
//!   window).
//!
//! ## The layout files
//!
//! `<game>\dod\dodstudio_ui\`: `DodStudio.res` for the window, then one per
//! tab (`Playback.res`, `Demos.res`, `Studio.res`). Our own folder beside
//! `dodstudio_hd` -- never `dod\resource`, which is the user's. Each default
//! (`goldsrc-hooks/ui/`, built into the DLL) is written the first time and
//! never again, so build-mode edits stay. `dodstudio_panel reset` puts the
//! defaults back and rebuilds the window.
//!
//! ## Per build
//!
//! Eight `GameUI.dll` addresses and sizes differ between the pre-Anniversary
//! and 25th Anniversary builds, and the Anniversary `Frame::Frame` takes a
//! fourth argument. [`BUILDS`] names each build by PE timestamp and image
//! size, and anything else is refused. `tools/verify_studio_panel.py` checks
//! every one against how GameUI builds its own Load Demo and Options windows,
//! on both movie installs.

// The window is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::names::console_name;

pub const NAME: &str = console_name!("panel");

/// The window's panel name, which its `.res` entry and #410's saved layout
/// (`GameUI/DodStudio`) are keyed by.
const PANEL_NAME: &CStr = c"DodStudio";
const SHEET_NAME: &CStr = c"Sheet";
/// Our folder under the game directory, as the engine's file system resolves
/// a `.res` path and build mode saves one.
const RES_DIR: &str = "dodstudio_ui";
const WINDOW_RES: (&CStr, &str, &str) = (
    c"dodstudio_ui/DodStudio.res",
    "DodStudio.res",
    include_str!("../ui/DodStudio.res"),
);

/// One tab: its panel name, its title on the tab strip, and its layout file
/// (path for the engine, file name, built-in default).
pub struct Page {
    pub name: &'static CStr,
    pub title: &'static CStr,
    pub res: (&'static CStr, &'static str, &'static str),
}

/// The tabs, in strip order.
pub const PAGES: [Page; 3] = [
    Page {
        name: c"Playback",
        title: c"Playback",
        res: (
            c"dodstudio_ui/Playback.res",
            "Playback.res",
            include_str!("../ui/Playback.res"),
        ),
    },
    Page {
        name: c"Demos",
        title: c"Demos",
        res: (
            c"dodstudio_ui/Demos.res",
            "Demos.res",
            include_str!("../ui/Demos.res"),
        ),
    },
    Page {
        name: c"Studio",
        title: c"Studio",
        res: (
            c"dodstudio_ui/Studio.res",
            "Studio.res",
            include_str!("../ui/Studio.res"),
        ),
    },
];

/// What the VCR bar's `OnCommand` handles, from the strings it compares
/// against (both builds' `GameUI.dll`).
const VCR_COMMANDS: &[&str] = &[
    "play", "pause", "faster", "slower", "stepf", "stepb", "start", "end", "stop", "load",
    "events", "save",
];

/// One `GameUI.dll` build: its identity and what building the window takes.
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
    /// `PropertySheet::PropertySheet(Panel *parent, const char *name)`.
    pub sheet_ctor: usize,
    /// `sizeof(PropertySheet)`, what `PropertyDialog` allocates for its own.
    pub sheet_size: usize,
    /// `PropertyPage::PropertyPage(Panel *parent, const char *name, bool)`.
    pub page_ctor: usize,
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
        sheet_ctor: 0x7_74f0,
        sheet_size: 0xac,
        page_ctor: 0x6_55f0,
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
        sheet_ctor: 0x8_54f0,
        sheet_size: 0xb4,
        page_ctor: 0x7_1cd0,
    },
];

/// What each page is allocated, comfortably more than `PropertyPage` (the
/// smallest Options page, a subclass with its own fields, is 0xc0 on
/// pre-Anniversary and 0xcc on Anniversary). Extra room is never touched.
const PAGE_ALLOC: usize = 0x400;

/// `Panel::OnCommand(const char *)`; the VCR bar overrides the same slot.
const FRAME_SLOT_ON_COMMAND: usize = 87;
/// `Frame::Activate()`: what GameUI calls on a dialog it has just built
/// (`jmp [vftable+0x280]`, both builds).
const FRAME_SLOT_ACTIVATE: usize = 160;
/// `Frame::GetClientArea(int &x, int &y, int &wide, int &tall)`, which
/// `PropertyDialog::PerformLayout` sizes its sheet by.
const FRAME_SLOT_GET_CLIENT_AREA: usize = 186;
/// `PropertySheet::AddPage(Panel *page, const char *title)`, what
/// `PropertyDialog::AddPage` jumps to.
const SHEET_SLOT_ADD_PAGE: usize = 134;
/// How many vftable slots a copy carries. `Frame` has about 190; the rest are
/// never called through these objects.
const VFTABLE_SLOTS: usize = 240;
/// `Panel::GetVPanel()`, the first virtual.
const PANEL_SLOT_GET_VPANEL: usize = 0;

/// `IPanel` (`VGUI_Panel007`) slots, the same table #410 checks.
const IPANEL_SET_POS: usize = 2;
const IPANEL_GET_POS: usize = 3;
const IPANEL_SET_SIZE: usize = 4;
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

/// The GameUI panel the window is parented to, as the VCR bar and the main
/// menu are.
const TASKBAR: &str = "TaskBar";
/// The gap between the window's client area and the tab strip.
const SHEET_MARGIN: i32 = 4;

/// Our window's object, or 0 before it is built. #410 accepts it as a GameUI
/// window although its vftable is our copy.
static OBJECT: AtomicUsize = AtomicUsize::new(0);
/// Its tab strip.
static SHEET: AtomicUsize = AtomicUsize::new(0);

/// The window's `Frame` object, for #410's walk.
pub fn object() -> usize {
    OBJECT.load(Ordering::Acquire)
}

/// What a button's command asks for.
#[derive(Debug, PartialEq, Eq)]
enum Action<'a> {
    Vcr(&'a str),
    Engine(&'a str),
    /// Anything else, for the class's own `OnCommand`.
    Own,
}

fn action(command: &str) -> Action<'_> {
    if let Some(line) = command.strip_prefix("engine ") {
        return Action::Engine(line.trim());
    }
    match VCR_COMMANDS
        .iter()
        .find(|c| c.eq_ignore_ascii_case(command))
    {
        Some(c) => Action::Vcr(c),
        None => Action::Own,
    }
}

/// Where the tab strip goes in a window whose client area is `client`
/// (x, y, wide, tall): all of it, inset by [`SHEET_MARGIN`].
fn sheet_bounds(client: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = client;
    (
        x + SHEET_MARGIN,
        y + SHEET_MARGIN,
        (w - 2 * SHEET_MARGIN).max(1),
        (h - 2 * SHEET_MARGIN).max(1),
    )
}

fn res_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default()
        .join("dod")
        .join(RES_DIR)
}

/// Writes each default layout that is missing (or all of them on `reset`),
/// and says what it wrote. Never touches an existing file otherwise:
/// build-mode edits are the user's.
fn ensure_res(reset: bool) -> Result<Option<String>, String> {
    let dir = res_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let mut written = Vec::new();
    for (_, file, default) in std::iter::once(WINDOW_RES).chain(PAGES.iter().map(|p| p.res)) {
        let path = dir.join(file);
        if path.exists() && !reset {
            continue;
        }
        std::fs::write(&path, default)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        written.push(file);
    }
    Ok((!written.is_empty()).then(|| format!("wrote {} to {}", written.join(", "), dir.display())))
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;

    type Vpanel = u32;
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    type OperatorNewFn = unsafe extern "C" fn(usize) -> *mut c_void;
    type FrameCtor3 = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32);
    type FrameCtor4 =
        unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32, u32);
    type SheetCtor = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char);
    type PageCtor = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32);
    type AddPageFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char);
    type LoadSettingsFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char);
    type OnCommandFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char);
    type ActivateFn = unsafe extern "thiscall" fn(*mut c_void);
    type ClientAreaFn =
        unsafe extern "thiscall" fn(*mut c_void, *mut i32, *mut i32, *mut i32, *mut i32);
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
    type XyFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, i32, i32);
    type GetXyFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, *mut i32, *mut i32);

    /// Each class's own `OnCommand`, for whatever our handler passes on.
    static FRAME_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);

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

        unsafe fn children(&self, vp: Vpanel) -> usize {
            unsafe {
                let count: PanelIntFn = slot(self.panel, IPANEL_GET_CHILD_COUNT);
                let child: ChildFn = slot(self.panel, IPANEL_GET_CHILD);
                (0..count(self.panel, vp).clamp(0, 512))
                    .filter(|&i| child(self.panel, vp, i) != 0)
                    .count()
            }
        }

        unsafe fn rect(&self, vp: Vpanel) -> (i32, i32, i32, i32) {
            unsafe {
                let get_pos: GetXyFn = slot(self.panel, IPANEL_GET_POS);
                let get_size: GetXyFn = slot(self.panel, IPANEL_GET_SIZE);
                let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
                get_pos(self.panel, vp, &mut x, &mut y);
                get_size(self.panel, vp, &mut w, &mut h);
                (x, y, w, h)
            }
        }

        /// Where a panel is and what holds it, for the log.
        unsafe fn describe(&self, vp: Vpanel) -> String {
            unsafe {
                let visible: PanelBoolFn = slot(self.panel, IPANEL_IS_VISIBLE);
                let parent: ParentFn = slot(self.panel, IPANEL_GET_PARENT);
                let (x, y, w, h) = self.rect(vp);
                let p = parent(self.panel, vp);
                format!(
                    "at {x},{y} {w}x{h}, visible {}, {} child panel(s), parent {:?}",
                    visible(self.panel, vp) & 0xff,
                    self.children(vp),
                    self.name(p),
                )
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

    /// Gives `object` a copy of its vftable with `OnCommand` pointing at
    /// `handler`, and returns the class's own `OnCommand`. The copy (RTTI
    /// locator at [-1] included) is leaked: it lives as long as the object.
    unsafe fn own_on_command(object: *mut c_void, handler: usize) -> usize {
        unsafe {
            let original = *(object as *const *const usize);
            let mut copy = vec![0usize; VFTABLE_SLOTS + 1];
            for (i, entry) in copy.iter_mut().enumerate() {
                *entry = *original.offset(i as isize - 1);
            }
            let own = copy[FRAME_SLOT_ON_COMMAND + 1];
            copy[FRAME_SLOT_ON_COMMAND + 1] = handler;
            let copy: &'static mut [usize] = Box::leak(copy.into_boxed_slice());
            *(object as *mut *const usize) = copy.as_ptr().add(1);
            own
        }
    }

    /// GameUI's `operator new`, zeroed.
    unsafe fn allocate(base: usize, build: &Build, size: usize) -> Result<*mut c_void, String> {
        unsafe {
            let new: OperatorNewFn = std::mem::transmute(base + build.operator_new);
            let object = new(size);
            if object.is_null() {
                return Err("GameUI's operator new returned null".to_string());
            }
            std::ptr::write_bytes(object as *mut u8, 0, size);
            Ok(object)
        }
    }

    /// Builds the window, its tab strip and its pages.
    unsafe fn build() -> Result<(*mut c_void, *mut c_void), String> {
        let (base, build) = gameui()?;
        let vgui = Vgui::get()?;
        let parent = unsafe { base_panel(&vgui) }?;
        unsafe {
            let load: LoadSettingsFn = std::mem::transmute(base + build.load_control_settings);

            let frame = allocate(base, build, build.frame_size)?;
            if build.frame_ctor_fourth_arg {
                let ctor: FrameCtor4 = std::mem::transmute(base + build.frame_ctor);
                ctor(frame, parent, PANEL_NAME.as_ptr(), 1, 0);
            } else {
                let ctor: FrameCtor3 = std::mem::transmute(base + build.frame_ctor);
                ctor(frame, parent, PANEL_NAME.as_ptr(), 1);
            }
            FRAME_ON_COMMAND.store(
                own_on_command(frame, frame_on_command as *const () as usize),
                Ordering::Release,
            );
            load(frame, WINDOW_RES.0.as_ptr(), std::ptr::null());

            let sheet = allocate(base, build, build.sheet_size)?;
            let sheet_ctor: SheetCtor = std::mem::transmute(base + build.sheet_ctor);
            sheet_ctor(sheet, frame, SHEET_NAME.as_ptr());

            let page_ctor: PageCtor = std::mem::transmute(base + build.page_ctor);
            let add_page: AddPageFn = slot(sheet, SHEET_SLOT_ADD_PAGE);
            for page in &PAGES {
                let object = allocate(base, build, PAGE_ALLOC)?;
                page_ctor(object, frame, page.name.as_ptr(), 1);
                PAGE_ON_COMMAND.store(
                    own_on_command(object, page_on_command as *const () as usize),
                    Ordering::Release,
                );
                load(object, page.res.0.as_ptr(), std::ptr::null());
                add_page(sheet, object, page.title.as_ptr());
            }
            Ok((frame, sheet))
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

    /// Sizes the tab strip to the window's client area, as
    /// `PropertyDialog::PerformLayout` does for its own. Writes only when it
    /// differs.
    unsafe fn fit_sheet(vgui: &Vgui, frame: *mut c_void) {
        let sheet = SHEET.load(Ordering::Acquire) as *mut c_void;
        if sheet.is_null() {
            return;
        }
        unsafe {
            let client_area: ClientAreaFn = slot(frame, FRAME_SLOT_GET_CLIENT_AREA);
            let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
            client_area(frame, &mut x, &mut y, &mut w, &mut h);
            let get_vpanel: GetVpanelFn = slot(sheet, PANEL_SLOT_GET_VPANEL);
            let vp = get_vpanel(sheet);
            if vp == 0 {
                return;
            }
            let want = sheet_bounds((x, y, w, h));
            let now = vgui.rect(vp);
            if (now.0, now.1) != (want.0, want.1) {
                let set_pos: XyFn = slot(vgui.panel, IPANEL_SET_POS);
                set_pos(vgui.panel, vp, want.0, want.1);
            }
            if (now.2, now.3) != (want.2, want.3) {
                let set_size: XyFn = slot(vgui.panel, IPANEL_SET_SIZE);
                set_size(vgui.panel, vp, want.2, want.3);
            }
        }
    }

    pub(super) fn poll() {
        if OBJECT.load(Ordering::Relaxed) == 0 {
            return;
        }
        let Ok(vgui) = Vgui::get() else { return };
        unsafe {
            if let Some((frame, _)) = window(&vgui) {
                fit_sheet(&vgui, frame);
            }
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

    unsafe fn handle(this: *mut c_void, raw: *const c_char, own: &AtomicUsize) {
        let command = text(raw);
        let result = match action(&command) {
            Action::Vcr(c) => Vgui::get().and_then(|vgui| unsafe { to_vcr_bar(&vgui, c) }),
            Action::Engine(line) => {
                let ran = std::ffi::CString::new(format!("{line}\n"))
                    .is_ok_and(|l| crate::engine::client_cmd(&l));
                if ran {
                    Ok(())
                } else {
                    Err(format!("could not run \"{line}\""))
                }
            }
            Action::Own => {
                let original = own.load(Ordering::Acquire);
                if original != 0 {
                    // Safety: the class's own OnCommand, from its vftable.
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

    /// The window's `OnCommand`.
    unsafe extern "thiscall" fn frame_on_command(this: *mut c_void, raw: *const c_char) {
        unsafe { handle(this, raw, &FRAME_ON_COMMAND) }
    }

    /// Each page's `OnCommand`: the buttons on a tab send their commands here.
    unsafe extern "thiscall" fn page_on_command(this: *mut c_void, raw: *const c_char) {
        unsafe { handle(this, raw, &PAGE_ON_COMMAND) }
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
                    if let Some((old, old_vp)) = window(&vgui) {
                        // Rebuilt on reset: the old one is hidden and left
                        // for GameUI to delete with its parent.
                        let set_visible: PanelSetBoolFn = slot(vgui.panel, IPANEL_SET_VISIBLE);
                        set_visible(vgui.panel, old_vp, 0);
                        let _ = old;
                    }
                    let (frame, sheet) = build()?;
                    OBJECT.store(frame as usize, Ordering::Release);
                    SHEET.store(sheet as usize, Ordering::Release);
                    let (_, b) = gameui()?;
                    notes.push(format!(
                        "built the window with {} tab(s) ({} GameUI)",
                        PAGES.len(),
                        b.name
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
                fit_sheet(&vgui, object);
                // Frame::Activate, as GameUI opens its own dialogs: shows it,
                // brings it to the front and gives it focus.
                let activate: ActivateFn = slot(object, FRAME_SLOT_ACTIVATE);
                activate(object);
                notes.push(format!(
                    "open (press ESC for the menu if you can't see it); {}",
                    vgui.describe(vp)
                ));
            }
        }
        Ok(notes.join("; "))
    }
}

/// Keeps the tab strip sized to the window. Called every frame from
/// `commands::poll`; one atomic load until the window has been opened.
pub fn poll() {
    #[cfg(target_arch = "x86")]
    hook::poll();
}

fn argument() -> Option<String> {
    let engfuncs = crate::engine::engfuncs()?;
    // Safety: the engine's own argument accessors, during a command.
    unsafe {
        if (engfuncs.cmd_argc)() < 2 {
            return None;
        }
        let raw = (engfuncs.cmd_argv)(1);
        (!raw.is_null()).then(|| CStr::from_ptr(raw).to_string_lossy().to_ascii_lowercase())
    }
}

/// `dodstudio_panel [reset]`: opens or closes the window; `reset` writes the
/// default layouts back and rebuilds it.
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
    fn button_commands_are_sorted_into_what_they_do() {
        assert_eq!(action("play"), Action::Vcr("play"));
        assert_eq!(action("Faster"), Action::Vcr("faster"));
        assert_eq!(
            action("engine dodstudio_debug_status"),
            Action::Engine("dodstudio_debug_status")
        );
        assert_eq!(action("Close"), Action::Own);
    }

    #[test]
    fn the_tab_strip_fills_the_client_area_inside_a_margin() {
        assert_eq!(sheet_bounds((2, 28, 516, 210)), (6, 32, 508, 202));
        // Never inverted, however small the window gets.
        let tiny = sheet_bounds((0, 0, 3, 3));
        assert!(tiny.2 >= 1 && tiny.3 >= 1);
    }

    #[test]
    fn every_default_layout_is_built_in_and_names_its_own_file() {
        assert!(WINDOW_RES.2.contains("\"DodStudio\""));
        assert!(WINDOW_RES.2.contains("dodstudio_ui/DodStudio.res"));
        for page in &PAGES {
            let path = page.res.0.to_str().unwrap();
            assert_eq!(path, format!("{RES_DIR}/{}", page.res.1));
            assert!(page.res.2.starts_with(&format!("\"{path}\"")), "{path}");
        }
        // The Playback tab carries every VCR button.
        for command in [
            "start", "slower", "stepb", "pause", "play", "stepf", "faster", "end", "stop",
        ] {
            assert!(
                PAGES[0]
                    .res
                    .2
                    .contains(&format!("\"Command\"\t\t\"{command}\"")),
                "{command}"
            );
        }
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
