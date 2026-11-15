//! `ds_capture_stay_behind`: keeps a background capture in the background.
//! With it on, HLAE no longer pulls the game window to the front when a
//! recording starts, and a minimised game is parked off-screen instead, since
//! a minimised game records black frames (#434, #723). Its own setting, apart
//! from `ds_capture_run_in_background`'s speed, so a user can watch a batch at
//! full speed with the game in front.
//!
//! ## What HLAE does
//!
//! With the game windowed, `mirv_recordmovie_start` turns the game window
//! into a borderless popup at the capture size, topmost, at the top-left of
//! the screen, and activates it (AfxHookGoldSrc.dll 2.25.3, `+0xb1b0`:
//! `SetParent(0)`, `WS_POPUP`, `SetWindowPos(HWND_TOPMOST, 0, 0, w, h,
//! SWP_SHOWWINDOW | SWP_FRAMECHANGED)`); `mirv_recordmovie_stop` puts the old
//! style back (`+0xb0f0`). So every clip of a batch took the screen and the
//! keyboard for as long as it recorded, and setting the window style also
//! cleared `WS_MINIMIZE`: that, not the game, is why a minimised game's
//! recordings looked right. Measured 2026-10-10 (PRE, 1280x720, standard
//! render mode):
//!
//! - recording behind another window with the popup skipped: right frames,
//!   and the game never took the foreground;
//! - recording minimised with the popup skipped: every frame black, in the
//!   standard and the fBO render modes alike. The engine draws nothing while
//!   minimised.
//!
//! So the popup is skipped (the entry function returns at once; the exit
//! function then skips itself, as it only undoes an entry that ran).
//!
//! ## A minimised game is parked off-screen (#723)
//!
//! Neither `hw.dll` checks for a minimised window: the engine keeps drawing
//! and swapping, but an iconic window's default framebuffer owns no pixels, so
//! HLAE's `glReadPixels` reads black. A window that isn't iconic but lies
//! wholly off the desktop records full, right frames (measured from outside
//! the game, PRE, `SetWindowPos` to x = -6000). So a minimise is turned into
//! that: the window is restored without being activated and moved left of the
//! virtual desktop, keeping its size and taskbar button. When the user
//! activates it (its taskbar button, alt-tab), or the setting goes off, it
//! moves back where it was. With [`CLOAK_NAME`] on it is DWM-cloaked in place
//! instead, to compare: from outside the process Windows refuses the cloak.
//!
//! Found by a byte pattern in HLAE's DLL. Another HLAE version may not match:
//! the log says so, and recording pulls the window to the front as before.
//!
//! ## Before Studio can reach the game
//!
//! The setting arrives over the pipe, about a second after the window opens.
//! Until then the window opened on top of the user's, and on the
//! pre-Anniversary build the game moved the pointer to the centre of its
//! window once (measured with a foreground logger, 2026-10-11). So a capture
//! batch's launch line carries [`START_BEHIND_FLAG`], and a thread started as
//! the DLL loads keeps the window at the bottom of the stack and puts the
//! pointer back, until the setting takes over.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicUsize, Ordering};

use crate::names::console_name;
use crate::setting::Cvar;

/// Debug: a minimised game is DWM-cloaked instead of parked off-screen, to
/// compare the two (#723).
pub const CLOAK_NAME: &str = console_name!("debug_cloak_instead");
pub static CLOAK_CVAR: Cvar = Cvar::new();

pub const NAME: &str = console_name!("capture_stay_behind");

pub static CVAR: Cvar = Cvar::new();

/// Whether the setting is on now. Off once the client has shut down: the
/// engine has freed the cvar by the time SDL's window teardown calls
/// `behind_imports`' detours with it (#1080).
pub fn wanted() -> bool {
    // The shut-down check comes first, so a freed cvar is never read.
    !crate::engine::client_shut_down() && CVAR.on()
}

/// The start of HLAE's "make the window a popup" function. The exit function
/// starts the same way but has `0F 84` for the second jump and `56 6A EC`
/// after it.
//                          push ebp mov ebp,esp sub esp,14 mov eax,[cookie] xor eax,ebp mov [ebp-4],eax cmp [full],0 jne         cmp [entered],0 jne         push esi mov esi,[GetWindowLongA] push -10
const ENTER_PATTERN: &str = "55 8B EC 83 EC 14 A1 ?? ?? ?? ?? 33 C5 89 45 FC 80 3D ?? ?? ?? ?? 00 0F 85 ?? ?? ?? ?? 80 3D ?? ?? ?? ?? 00 0F 85 ?? ?? ?? ?? 56 8B 35 ?? ?? ?? ?? 6A F0";

const PUSH_EBP: u8 = 0x55;
const RET: u8 = 0xC3;

/// The entry function, once found; 1 when it can't be.
static ENTER: AtomicUsize = AtomicUsize::new(0);
static SKIPPING: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU32 = AtomicU32::new(0);
/// The game window, once found.
static WINDOW: AtomicUsize = AtomicUsize::new(0);

/// How often the minimised check runs, in frames.
const CHECK_EVERY: u32 = 30;
/// Checks a minimised game that still has the foreground waits for Windows to
/// hand it on, before it is parked anyway (it records black minimised).
const FOREGROUND_WAIT_CHECKS: u32 = 4;
/// Consecutive checks that found the game minimised and still in the
/// foreground.
static WAITED: AtomicU32 = AtomicU32::new(0);
/// The setting's value at the last frame, to notice it going off.
static WAS_ON: AtomicBool = AtomicBool::new(false);

/// How a minimise was turned into something that still records (#723).
const SHOWN: u8 = 0;
const PARKED: u8 = 1;
const CLOAKED: u8 = 2;
static HIDDEN: AtomicU8 = AtomicU8::new(SHOWN);
/// Whether the game had the foreground at the last check while hidden: only
/// gaining it brings the window back, so a game that kept the foreground
/// through its minimise isn't brought straight back.
static FOREGROUND_BEFORE: AtomicBool = AtomicBool::new(false);
/// The [`Parking`] while [`PARKED`]; only the game thread touches them.
static HOME_X: AtomicI32 = AtomicI32::new(0);
static HOME_Y: AtomicI32 = AtomicI32::new(0);
static PARKED_X: AtomicI32 = AtomicI32::new(0);
static PARKED_Y: AtomicI32 = AtomicI32::new(0);
/// Room between the parked window and the desktop's left edge, so no frame
/// shadow shows.
const PARK_GAP: i32 = 100;

/// On a capture batch's launch line (`native::patch::builder::START_BEHIND_ARG`; keep
/// the two in step): the window stays behind from the moment it opens.
pub const START_BEHIND_FLAG: &str = "-dodstudio_start_behind";
/// How long the start-up watch runs at most, if the setting never arrives.
const START_WATCH: std::time::Duration = std::time::Duration::from_secs(120);
/// How often it looks.
const START_WATCH_EVERY: std::time::Duration = std::time::Duration::from_millis(5);
/// Pointer moves it undoes at most, in case something keeps moving it.
const MAX_POINTER_PUT_BACKS: u32 = 20;

const SW_SHOWNOACTIVATE: i32 = 4;
const SW_MINIMIZE: i32 = 6;
const HWND_BOTTOM: isize = 1;
const SW_SHOWMINNOACTIVE: u32 = 7;
const SWP_NOSIZE: u32 = 0x1;
const SWP_NOMOVE: u32 = 0x2;
const SWP_NOZORDER: u32 = 0x4;
const SWP_NOACTIVATE: u32 = 0x10;
const SM_XVIRTUALSCREEN: i32 = 76;
const DWMWA_CLOAK: u32 = 13;
/// From another thread, so the call never waits on the game's message loop.
const SWP_ASYNCWINDOWPOS: u32 = 0x4000;
const GW_HWNDNEXT: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct WindowPlacement {
    length: u32,
    flags: u32,
    show_cmd: u32,
    min_position: [i32; 2],
    max_position: [i32; 2],
    normal_position: Rect,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(
        callback: unsafe extern "system" fn(*mut c_void, isize) -> i32,
        param: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: *mut c_void) -> i32;
    fn IsWindow(hwnd: *mut c_void) -> i32;
    fn IsIconic(hwnd: *mut c_void) -> i32;
    fn GetForegroundWindow() -> *mut c_void;
    fn GetClassNameA(hwnd: *mut c_void, name: *mut u8, max: i32) -> i32;
    fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
    fn GetWindow(hwnd: *mut c_void, cmd: u32) -> *mut c_void;
    fn GetClientRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
    fn GetWindowRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
    // `[x, y]`, as `studio_panel` declares `GetCursorPos`: one signature per
    // name across the crate.
    fn ClientToScreen(hwnd: *mut c_void, point: *mut [i32; 2]) -> i32;
    fn GetCursorPos(point: *mut [i32; 2]) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn SetWindowPos(
        hwnd: *mut c_void,
        after: isize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flags: u32,
    ) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn GetWindowPlacement(hwnd: *mut c_void, placement: *mut WindowPlacement) -> i32;
    fn SetWindowPlacement(hwnd: *mut c_void, placement: *const WindowPlacement) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: *mut c_void,
        attribute: u32,
        value: *const c_void,
        size: u32,
    ) -> i32;
}

fn report(line: &str) {
    unsafe { crate::debug::report(&format!("hlae_window: {line}")) };
}

fn enter_address() -> Option<usize> {
    match ENTER.load(Ordering::Acquire) {
        0 => {}
        1 => return None,
        found => return Some(found),
    }
    let module = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleHandleA(
            c"AfxHookGoldSrc.dll".as_ptr().cast(),
        )
    } as usize;
    if module == 0 {
        // HLAE isn't loaded (yet): look again later.
        return None;
    }
    // Safety: a module handle the loader gave us.
    match unsafe { crate::scan::find_unique(module, ENTER_PATTERN) } {
        Ok(found) => {
            report(&format!(
                "HLAE's recording popup is at AfxHookGoldSrc.dll+{:#x}",
                found - module
            ));
            ENTER.store(found, Ordering::Release);
            Some(found)
        }
        Err(why) => {
            report(&format!(
                "HLAE's recording popup wasn't found ({why}); recordings will bring the game to the front"
            ));
            ENTER.store(1, Ordering::Release);
            None
        }
    }
}

fn set_skipping(skip: bool) {
    let Some(address) = enter_address() else {
        return;
    };
    let want = if skip { RET } else { PUSH_EBP };
    // Safety: the scan proved this byte is mapped code.
    let present = unsafe { *(address as *const u8) };
    if present != want {
        if present != PUSH_EBP && present != RET {
            report(&format!(
                "HLAE's recording popup starts {present:#04x}; left alone"
            ));
            ENTER.store(1, Ordering::Release);
            return;
        }
        if !unsafe { crate::patch::write_code_bytes(address, &[want]) } {
            report("could not make HLAE's recording popup writable");
            ENTER.store(1, Ordering::Release);
            return;
        }
        report(if skip {
            "recordings leave the game window where it is"
        } else {
            "recordings bring the game window to the front again (HLAE's own behaviour)"
        });
    }
    SKIPPING.store(skip, Ordering::Release);
}

unsafe extern "system" fn find_window(hwnd: *mut c_void, _: isize) -> i32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid != std::process::id() || unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    let mut class = [0u8; 32];
    let n = unsafe { GetClassNameA(hwnd, class.as_mut_ptr(), class.len() as i32) };
    if class.get(..n.max(0) as usize) == Some(b"SDL_app".as_slice()) {
        WINDOW.store(hwnd as usize, Ordering::Release);
        return 0;
    }
    1
}

/// The game's main window.
fn window() -> Option<*mut c_void> {
    let cached = WINDOW.load(Ordering::Acquire) as *mut c_void;
    if !cached.is_null() && unsafe { IsWindow(cached) } != 0 {
        return Some(cached);
    }
    WINDOW.store(0, Ordering::Release);
    unsafe { EnumWindows(find_window, 0) };
    let found = WINDOW.load(Ordering::Acquire) as *mut c_void;
    (!found.is_null()).then_some(found)
}

fn cloak_wanted() -> bool {
    CLOAK_CVAR.on()
}

/// The left edge that puts a window `width` wide wholly left of a virtual
/// desktop starting at `virtual_left` (negative with a monitor left of the
/// primary one).
fn parked_x(virtual_left: i32, width: i32) -> i32 {
    virtual_left
        .saturating_sub(width.max(0))
        .saturating_sub(PARK_GAP)
}

/// Where a parked window came from and where it was put, top-left corners in
/// screen coordinates (the size never changes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Parking {
    home: Point,
    parked: Point,
}

impl Parking {
    /// Parks a window `width` wide whose top-left was `home`: same row, left
    /// of the virtual desktop.
    fn new(home: Point, width: i32, virtual_left: i32) -> Self {
        Self {
            home,
            parked: Point {
                x: parked_x(virtual_left, width),
                y: home.y,
            },
        }
    }

    /// Where to move the window back to, seen at `now`: home while it is still
    /// where it was parked, nothing once something else moved it (HLAE's popup,
    /// where the pattern wasn't found), as that move is the newer one.
    fn way_back(&self, now: Point) -> Option<Point> {
        (now == self.parked).then_some(self.home)
    }

    /// `r`, a rect of the parked window, moved back by the distance it was
    /// parked. A distance, so it holds in any coordinates that only differ from
    /// the screen's by an offset (a placement's workspace coordinates).
    fn unpark_rect(&self, r: Rect) -> Rect {
        let dx = self.home.x.saturating_sub(self.parked.x);
        let dy = self.home.y.saturating_sub(self.parked.y);
        Rect {
            left: r.left.saturating_add(dx),
            top: r.top.saturating_add(dy),
            right: r.right.saturating_add(dx),
            bottom: r.bottom.saturating_add(dy),
        }
    }
}

fn remembered() -> Option<Parking> {
    (HIDDEN.load(Ordering::Acquire) == PARKED).then(|| Parking {
        home: Point {
            x: HOME_X.load(Ordering::Relaxed),
            y: HOME_Y.load(Ordering::Relaxed),
        },
        parked: Point {
            x: PARKED_X.load(Ordering::Relaxed),
            y: PARKED_Y.load(Ordering::Relaxed),
        },
    })
}

fn remember(p: Parking) {
    HOME_X.store(p.home.x, Ordering::Relaxed);
    HOME_Y.store(p.home.y, Ordering::Relaxed);
    PARKED_X.store(p.parked.x, Ordering::Relaxed);
    PARKED_Y.store(p.parked.y, Ordering::Relaxed);
    HIDDEN.store(PARKED, Ordering::Release);
}

/// Whether the user brought a hidden game forward since the last check.
fn activated(before: bool, now: bool) -> bool {
    now && !before
}

fn top_left(r: &Rect) -> Point {
    Point {
        x: r.left,
        y: r.top,
    }
}

/// Cloaks or uncloaks the game window from inside its own process, logging
/// the result. True when DWM accepted it.
fn set_cloak(hwnd: *mut c_void, on: bool) -> bool {
    let value: i32 = on.into();
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK,
            (&value as *const i32).cast(),
            size_of::<i32>() as u32,
        )
    };
    report(&format!(
        "DwmSetWindowAttribute(DWMWA_CLOAK, {}) returned {hr:#010x}{}",
        u8::from(on),
        if hr == 0 { " (accepted)" } else { " (refused)" }
    ));
    hr == 0
}

/// Turns a minimise into a window that still records: restored without
/// activation, then parked off-screen (or cloaked, with [`CLOAK_NAME`] on).
fn hide(hwnd: *mut c_void) {
    // Minimised again while parked: restoring it lands it on the parked spot,
    // so the home it had before still holds.
    let earlier = remembered();
    // Still iconic: at the bottom, the restore doesn't flash it over the
    // user's window before the move.
    send_to_bottom(hwnd);
    if cloak_wanted() {
        // Cloaked before the restore, so it never shows.
        if set_cloak(hwnd, true) {
            unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
            HIDDEN.store(CLOAKED, Ordering::Release);
            FOREGROUND_BEFORE.store(has_foreground(hwnd), Ordering::Relaxed);
            report(&format!(
                "the game was minimised, which records black frames; cloaked in place instead ({CLOAK_NAME} 1)"
            ));
            return;
        }
        report("the cloak was refused; parking off-screen instead");
    }
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    // The restored window's own rect is the normal rect GetWindowPlacement
    // holds, in screen coordinates; the placement's is in workspace
    // coordinates, off by the taskbar when it is docked left or top.
    let Some(rect) = window_rect(hwnd) else {
        report(
            "the game was minimised; restored behind the other windows (its rect wasn't readable, so not parked)",
        );
        return;
    };
    let home = earlier.map_or(top_left(&rect), |p| p.home);
    let virtual_left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let parking = Parking::new(home, rect.right - rect.left, virtual_left);
    unsafe {
        SetWindowPos(
            hwnd,
            0,
            parking.parked.x,
            parking.parked.y,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE,
        );
    }
    remember(parking);
    FOREGROUND_BEFORE.store(has_foreground(hwnd), Ordering::Relaxed);
    report(&format!(
        "the game was minimised, which records black frames; parked off-screen at ({}, {}) instead, {}x{}, home ({}, {}); activating it brings it back",
        parking.parked.x,
        parking.parked.y,
        rect.right - rect.left,
        rect.bottom - rect.top,
        home.x,
        home.y
    ));
}

/// Puts a hidden game back where it was, without activating it. `why` is for
/// the log.
fn bring_back(why: &str) {
    let parking = remembered();
    let how = HIDDEN.swap(SHOWN, Ordering::AcqRel);
    if how == SHOWN {
        return;
    }
    let Some(hwnd) = window() else {
        return;
    };
    if how == CLOAKED {
        set_cloak(hwnd, false);
        report(&format!("uncloaked: {why}"));
        return;
    }
    let Some(parking) = parking else { return };
    if unsafe { IsIconic(hwnd) } != 0 {
        // Minimised on the parked spot: move the rect it restores to back by
        // the distance it was parked, which holds in the placement's workspace
        // coordinates too, and leave it minimised.
        let mut wp = WindowPlacement {
            length: size_of::<WindowPlacement>() as u32,
            ..Default::default()
        };
        if unsafe { GetWindowPlacement(hwnd, &mut wp) } == 0 {
            return;
        }
        wp.normal_position = parking.unpark_rect(wp.normal_position);
        wp.show_cmd = SW_SHOWMINNOACTIVE;
        unsafe { SetWindowPlacement(hwnd, &wp) };
        report(&format!(
            "minimised on the parked spot; it restores at ({}, {}) again: {why}",
            parking.home.x, parking.home.y
        ));
        return;
    }
    let now = window_rect(hwnd).map(|r| top_left(&r));
    match now.and_then(|now| parking.way_back(now)) {
        Some(home) => {
            unsafe {
                SetWindowPos(
                    hwnd,
                    0,
                    home.x,
                    home.y,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE,
                );
            }
            report(&format!(
                "back on screen at ({}, {}): {why}",
                home.x, home.y
            ));
        }
        None => report(&format!(
            "something else moved the window off the parked spot; left where it is: {why}"
        )),
    }
}

/// Moves the game to the bottom of the window stack, leaving the foreground
/// where it is.
fn send_to_bottom(hwnd: *mut c_void) {
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
        );
    }
}

fn has_foreground(hwnd: *mut c_void) -> bool {
    let foreground = unsafe { GetForegroundWindow() };
    foreground == hwnd
}

/// Gets the game out of the way. Without the foreground it only goes to the
/// bottom of the window stack. With it, minimising hands the foreground to the
/// next window, and a later check parks the game off-screen (#723).
///
/// Minimising a game that didn't have the foreground gave it the foreground:
/// the restore right after it activated the window, and the game kept the
/// keyboard for 4.3 s until the next step aside (POST, 2026-10-11, a
/// foreground log).
pub fn step_aside() {
    let Some(hwnd) = window() else {
        return;
    };
    if has_foreground(hwnd) {
        unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
        report("stepped aside: minimised, so the foreground goes to the next window");
    } else {
        send_to_bottom(hwnd);
        report("stepped aside: behind the other windows");
    }
}

/// Whether `args` (the game's command line, program first) asks for the
/// window to stay behind from the start.
fn asks_start_behind<S: AsRef<str>>(args: &[S]) -> bool {
    args.iter()
        .skip(1)
        .any(|a| a.as_ref().eq_ignore_ascii_case(START_BEHIND_FLAG))
}

/// Starts the start-up watch when the launch line asks for it. Called once,
/// from the DLL's worker thread, before the game opens its window.
pub fn start_behind_if_asked() {
    // `args_os`, not `args`: `std::env::args()` panics on any argument that
    // isn't valid Unicode (a demo or folder name), and a panic here aborts
    // the game under `panic = "abort"`. A lossy argument just fails to match.
    let args: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    if !asks_start_behind(&args) {
        return;
    }
    report(&format!(
        "{START_BEHIND_FLAG}: the window stays behind from the moment it opens"
    ));
    crate::behind_imports::STARTING_BEHIND.store(true, Ordering::Release);
    crate::behind_imports::install_loaded();
    std::thread::spawn(watch_start);
}

fn cursor() -> Option<Point> {
    let mut p = [0; 2];
    (unsafe { GetCursorPos(&mut p) } != 0).then_some(Point { x: p[0], y: p[1] })
}

/// The centre of the game's client area, for the log.
fn client_centre(hwnd: *mut c_void) -> Option<Point> {
    let mut r = Rect::default();
    if unsafe { GetClientRect(hwnd, &mut r) } == 0 {
        return None;
    }
    let mut p = [(r.right - r.left) / 2, (r.bottom - r.top) / 2];
    (unsafe { ClientToScreen(hwnd, &mut p) } != 0).then_some(Point { x: p[0], y: p[1] })
}

/// The game window on screen, frame included.
fn window_rect(hwnd: *mut c_void) -> Option<Rect> {
    let mut r = Rect::default();
    (unsafe { GetWindowRect(hwnd, &mut r) } != 0).then_some(r)
}

fn inside(p: Point, r: &Rect) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

/// A move between two samples 5 ms apart that a hand doesn't make: a fast
/// flick covers about 250 px in 30 ms (the foreground logs), 40-50 px in 5 ms.
const JUMP_PX: i32 = 150;

/// Whether the pointer went from `before` to `after` in one jump, landing on
/// the game's window: the game moving it, not the user. Not "to the centre":
/// on PRE it landed 28 px off the client area's centre (2026-10-11), and a
/// window as big as the screen covers wherever it started.
fn jumped_into(before: Point, after: Point, window: &Rect) -> bool {
    let (dx, dy) = (after.x - before.x, after.y - before.y);
    dx * dx + dy * dy >= JUMP_PX * JUMP_PX && inside(after, window)
}

/// Off the game's thread: until the setting arrives over the pipe, keeps the
/// window at the bottom of the stack and undoes the game moving the pointer to
/// its centre while another window has the foreground.
fn watch_start() {
    let started = std::time::Instant::now();
    let mut seen = false;
    let mut last = cursor();
    let mut put_backs = 0;
    while started.elapsed() < START_WATCH && !wanted() {
        std::thread::sleep(START_WATCH_EVERY);
        // The game's modules load as it starts: hook each as it appears.
        if !crate::behind_imports::all_installed() {
            crate::behind_imports::install_loaded();
        }
        let now = cursor();
        let Some(hwnd) = window() else {
            last = now;
            continue;
        };
        if !seen {
            seen = true;
            report(&format!(
                "the window opened {} ms in; keeping it behind",
                started.elapsed().as_millis()
            ));
        }
        if has_foreground(hwnd) {
            last = now;
            continue;
        }
        if !unsafe { GetWindow(hwnd, GW_HWNDNEXT) }.is_null() {
            unsafe {
                SetWindowPos(
                    hwnd,
                    HWND_BOTTOM,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                );
            }
        }
        if let (Some(before), Some(after), Some(rect)) = (last, now, window_rect(hwnd))
            && jumped_into(before, after, &rect)
            && put_backs < MAX_POINTER_PUT_BACKS
        {
            unsafe { SetCursorPos(before.x, before.y) };
            put_backs += 1;
            let centre = client_centre(hwnd).unwrap_or_default();
            report(&format!(
                "the game moved the pointer to ({}, {}) (its client centre is ({}, {})); put it back at ({}, {})",
                after.x, after.y, centre.x, centre.y, before.x, before.y
            ));
            last = Some(before);
            continue;
        }
        last = now;
    }
    crate::behind_imports::STARTING_BEHIND.store(false, Ordering::Release);
    report(&format!(
        "start-up watch ended after {} ms",
        started.elapsed().as_millis()
    ));
}

/// Follows the setting. Called every frame from `commands::poll`.
pub fn poll() {
    let on = wanted();
    if on != SKIPPING.load(Ordering::Acquire) {
        set_skipping(on);
    }
    // Followed apart from SKIPPING, which stays false when HLAE's popup isn't
    // found.
    if WAS_ON.swap(on, Ordering::AcqRel) && !on {
        bring_back(&format!("{NAME} is off"));
    }
    if !on
        || !FRAMES
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(CHECK_EVERY)
    {
        return;
    }
    // Turned on without the launch flag, or a module that loaded late.
    if !crate::behind_imports::all_installed() {
        crate::behind_imports::install_loaded();
    }
    let Some(hwnd) = window() else {
        return;
    };
    let iconic = unsafe { IsIconic(hwnd) } != 0;
    if !iconic {
        WAITED.store(0, Ordering::Relaxed);
        if HIDDEN.load(Ordering::Acquire) != SHOWN {
            let foreground = has_foreground(hwnd);
            if activated(
                FOREGROUND_BEFORE.swap(foreground, Ordering::Relaxed),
                foreground,
            ) {
                bring_back("the game was activated");
            } else if let Some(parking) = remembered()
                && window_rect(hwnd).is_some_and(|r| parking.way_back(top_left(&r)).is_none())
            {
                // Moved by something else (HLAE's popup when its pattern
                // wasn't found): that position wins, and a later minimise
                // parks from there.
                HIDDEN.store(SHOWN, Ordering::Release);
                report("something else moved the window off the parked spot; no longer parked");
            }
        }
        return;
    }
    // Shown again while it still has the foreground, it would keep it.
    if has_foreground(hwnd) && WAITED.fetch_add(1, Ordering::Relaxed) < FOREGROUND_WAIT_CHECKS {
        return;
    }
    WAITED.store(0, Ordering::Relaxed);
    hide(hwnd);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_starts_with_the_byte_it_swaps() {
        assert!(ENTER_PATTERN.starts_with("55 "));
        assert!(crate::scan::Pattern::parse(ENTER_PATTERN).is_ok());
    }

    #[test]
    fn the_start_behind_flag_is_a_whole_argument_after_the_program() {
        assert!(asks_start_behind(&[
            "hl.exe",
            "-game",
            "dod",
            "-dodstudio_start_behind"
        ]));
        assert!(asks_start_behind(&["hl.exe", "-DODSTUDIO_START_BEHIND"]));
        assert!(!asks_start_behind(&["-dodstudio_start_behind"]));
        assert!(!asks_start_behind(&["hl.exe", "-dodstudio_start_behind_x"]));
        assert!(!asks_start_behind(&[
            "hl.exe",
            "+exec",
            "dodstudio_helper.cfg"
        ]));
    }

    #[test]
    fn a_parked_window_sits_wholly_left_of_the_virtual_desktop() {
        // One monitor: the desktop starts at 0.
        assert_eq!(parked_x(0, 1296), -1396);
        // A monitor left of the primary one.
        assert_eq!(parked_x(-2560, 1296), -3956);
        assert!(parked_x(-2560, 1296) + 1296 < -2560);
        // Nonsense in, no overflow out.
        assert_eq!(parked_x(i32::MIN, 1296), i32::MIN);
        assert_eq!(parked_x(0, -5), -PARK_GAP);
    }

    #[test]
    fn parking_keeps_the_row_and_goes_home_only_from_the_parked_spot() {
        let home = Point { x: 400, y: 200 };
        let p = Parking::new(home, 1296, 0);
        assert_eq!(p.parked, Point { x: -1396, y: 200 });
        assert_eq!(p.way_back(p.parked), Some(home));
        // HLAE's popup put it at the top-left: that move wins.
        assert_eq!(p.way_back(Point { x: 0, y: 0 }), None);
    }

    #[test]
    fn a_minimised_parked_window_restores_at_home() {
        let p = Parking::new(Point { x: 400, y: 200 }, 1296, 0);
        // The placement's workspace coordinates, 48 px off the screen's
        // with the taskbar docked left: the distance still holds.
        let parked = Rect {
            left: -1396 - 48,
            top: 200,
            right: -1396 - 48 + 1296,
            bottom: 200 + 759,
        };
        assert_eq!(
            p.unpark_rect(parked),
            Rect {
                left: 400 - 48,
                top: 200,
                right: 400 - 48 + 1296,
                bottom: 200 + 759,
            }
        );
    }

    #[test]
    fn only_gaining_the_foreground_brings_it_back() {
        assert!(activated(false, true));
        assert!(!activated(true, true), "kept it through the minimise");
        assert!(!activated(false, false));
        assert!(!activated(true, false));
    }

    #[test]
    fn the_cloak_name_is_a_debug_cvar() {
        assert_eq!(CLOAK_NAME, "ds_debug_cloak_instead");
    }

    #[test]
    fn a_jump_onto_the_window_is_the_game_and_a_flick_is_not() {
        let screen = Rect {
            left: 0,
            top: 0,
            right: 3440,
            bottom: 1440,
        };
        // Measured on PRE: one 20 ms sample, onto the window's middle.
        assert!(jumped_into(
            Point { x: 2308, y: 544 },
            Point { x: 1692, y: 695 },
            &screen
        ));
        // A fast hand flick, sampled every 5 ms.
        assert!(!jumped_into(
            Point { x: 1000, y: 700 },
            Point { x: 1050, y: 690 },
            &screen
        ));
        // Off the window.
        let small = Rect {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 720,
        };
        assert!(!jumped_into(
            Point { x: 100, y: 100 },
            Point { x: 2000, y: 900 },
            &small
        ));
    }
}
