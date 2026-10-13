//! While a capture batch keeps the game behind (#434), the game's own calls
//! that would bring its window forward or move the user's pointer are turned
//! into ones that don't.
//!
//! Measured on 2026-10-11 with a foreground logger: PRE moved the pointer to
//! the centre of its window as the window opened and again as the first demo
//! loaded, and once confined it (the pointer stopped at x = 1080 while being
//! put back); the window opened on top of the user's for half a second, until
//! the game's busy start-up thread got round to a request to move it down.
//! Which modules call what, from their import tables (both movie installs):
//!
//! | | `SetCursorPos` | `ClipCursor` | `ShowWindow` | `SetWindowPos` |
//! |---|---|---|---|---|
//! | `hw.dll` (PRE only) | yes | | yes | |
//! | `SDL2.dll` | yes | yes | yes | yes |
//! | `vgui2.dll` | yes | | yes | yes |
//! | `vgui.dll` | | yes | yes | yes |
//!
//! POST's `hw.dll` imports none of them, which is why only PRE moved the
//! pointer on its own. Each module's import slot is pointed at a function here
//! that, while the game is meant to stay behind and isn't in the foreground,
//! swallows a pointer move or confinement and shows or moves the game window
//! without activating it, at the bottom of the stack. Otherwise it calls what
//! the slot held. Only these modules' own calls change (the same trade-off
//! `engine::hook_import` makes); HLAE's are left alone.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Memory::{
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect,
};

use crate::pe;

/// The modules hooked, in the order of the `M` index below.
const MODULES: [&str; 4] = ["hw.dll", "SDL2.dll", "vgui2.dll", "vgui.dll"];
const N: usize = MODULES.len();

static HOOKED: [AtomicBool; N] = [const { AtomicBool::new(false) }; N];
static REAL_SET_CURSOR_POS: [AtomicPtr<c_void>; N] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; N];
static REAL_CLIP_CURSOR: [AtomicPtr<c_void>; N] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; N];
static REAL_SHOW_WINDOW: [AtomicPtr<c_void>; N] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; N];
static REAL_SET_WINDOW_POS: [AtomicPtr<c_void>; N] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; N];

/// Set while the start-up watch runs, before the setting arrives over the pipe.
pub static STARTING_BEHIND: AtomicBool = AtomicBool::new(false);

static SWALLOWED_MOVES: AtomicU32 = AtomicU32::new(0);
static SWALLOWED_CLIPS: AtomicU32 = AtomicU32::new(0);

type SetCursorPosFn = unsafe extern "system" fn(i32, i32) -> i32;
type ClipCursorFn = unsafe extern "system" fn(*const c_void) -> i32;
type ShowWindowFn = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type SetWindowPosFn = unsafe extern "system" fn(*mut c_void, isize, i32, i32, i32, i32, u32) -> i32;

const SW_SHOWNORMAL: i32 = 1;
const SW_SHOWNOACTIVATE: i32 = 4;
const SW_SHOW: i32 = 5;
const SW_SHOWNA: i32 = 8;
const SW_RESTORE: i32 = 9;
const SW_SHOWDEFAULT: i32 = 10;
const HWND_TOP: isize = 0;
const HWND_BOTTOM: isize = 1;
const HWND_TOPMOST: isize = -1;
const SWP_NOSIZE: u32 = 0x1;
const SWP_NOMOVE: u32 = 0x2;
const SWP_NOZORDER: u32 = 0x4;
const SWP_NOACTIVATE: u32 = 0x10;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32;
    fn GetClassNameA(hwnd: *mut c_void, name: *mut u8, max: i32) -> i32;
    fn SetWindowPos(
        hwnd: *mut c_void,
        after: isize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flags: u32,
    ) -> i32;
}

fn report(line: &str) {
    unsafe { crate::debug::report(&format!("behind_imports: {line}")) };
}

/// Whether the game is meant to be behind now: during the start-up watch, or
/// with `dodstudio_stay_behind_while_capturing` on.
fn wanted() -> bool {
    STARTING_BEHIND.load(Ordering::Acquire) || crate::hlae_window::wanted()
}

/// Whether one of the game's own windows (the game, its console) is in front.
fn game_in_front() -> bool {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_null() {
        return false;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(fg, &mut pid) };
    pid == std::process::id()
}

fn holding_back() -> bool {
    wanted() && !game_in_front()
}

fn is_game_window(hwnd: *mut c_void) -> bool {
    let mut class = [0u8; 32];
    let n = unsafe { GetClassNameA(hwnd, class.as_mut_ptr(), class.len() as i32) };
    class.get(..n.max(0) as usize) == Some(b"SDL_app".as_slice())
}

/// What a show command becomes for a window that must not come forward.
fn without_activation(cmd: i32) -> Option<i32> {
    match cmd {
        SW_SHOW => Some(SW_SHOWNA),
        SW_SHOWNORMAL | SW_RESTORE | SW_SHOWDEFAULT => Some(SW_SHOWNOACTIVATE),
        // Everything else: hiding and minimising are fine, and a window can't
        // be maximised (SW_SHOWMAXIMIZED) without activating it; the batch's
        // window isn't maximised.
        _ => None,
    }
}

/// `after` and `flags` for a z-order change that must not bring the window
/// forward.
fn kept_down(after: isize, flags: u32) -> (isize, u32) {
    let flags = flags | SWP_NOACTIVATE;
    if flags & SWP_NOZORDER == 0 && (after == HWND_TOP || after == HWND_TOPMOST) {
        (HWND_BOTTOM, flags)
    } else {
        (after, flags)
    }
}

fn real<F: Copy>(slot: &AtomicPtr<c_void>) -> F {
    let p = slot.load(Ordering::Acquire);
    // Safety: only ever stored from the IAT slot this replaces, which holds a
    // function of type `F`.
    unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) }
}

unsafe extern "system" fn set_cursor_pos<const M: usize>(x: i32, y: i32) -> i32 {
    if holding_back() {
        if SWALLOWED_MOVES.fetch_add(1, Ordering::Relaxed) < 5 {
            report(&format!(
                "{} moved the pointer to ({x}, {y}); left it where the user has it",
                MODULES[M]
            ));
        }
        return 1;
    }
    unsafe { real::<SetCursorPosFn>(&REAL_SET_CURSOR_POS[M])(x, y) }
}

unsafe extern "system" fn clip_cursor<const M: usize>(rect: *const c_void) -> i32 {
    if !rect.is_null() && holding_back() {
        if SWALLOWED_CLIPS.fetch_add(1, Ordering::Relaxed) < 5 {
            report(&format!(
                "{} confined the pointer; left it free",
                MODULES[M]
            ));
        }
        return 1;
    }
    unsafe { real::<ClipCursorFn>(&REAL_CLIP_CURSOR[M])(rect) }
}

unsafe extern "system" fn show_window<const M: usize>(hwnd: *mut c_void, cmd: i32) -> i32 {
    let show: ShowWindowFn = real(&REAL_SHOW_WINDOW[M]);
    if holding_back()
        && is_game_window(hwnd)
        && let Some(quiet) = without_activation(cmd)
    {
        // Down first, while it may still be hidden, so it never shows on top.
        // Our own call, so straight to user32.
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
            )
        };
        report(&format!(
            "{} showed the game window ({cmd}); shown behind, not activated",
            MODULES[M]
        ));
        return unsafe { show(hwnd, quiet) };
    }
    unsafe { show(hwnd, cmd) }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn set_window_pos<const M: usize>(
    hwnd: *mut c_void,
    after: isize,
    x: i32,
    y: i32,
    cx: i32,
    cy: i32,
    flags: u32,
) -> i32 {
    let set_pos: SetWindowPosFn = real(&REAL_SET_WINDOW_POS[M]);
    if holding_back() && is_game_window(hwnd) {
        let (after, flags) = kept_down(after, flags);
        return unsafe { set_pos(hwnd, after, x, y, cx, cy, flags) };
    }
    unsafe { set_pos(hwnd, after, x, y, cx, cy, flags) }
}

/// Points `module`'s import of `user32!name` at `replacement`, keeping what it
/// held in `real_store`. False when the module doesn't import it.
fn hook(
    module: *mut c_void,
    name: &str,
    replacement: *mut c_void,
    real_store: &AtomicPtr<c_void>,
) -> bool {
    let Some(slot) = (unsafe { pe::find_iat_slot(module as *mut u8, "USER32.dll", name) }) else {
        return false;
    };
    unsafe {
        real_store.store(*slot, Ordering::Release);
        let mut old: PAGE_PROTECTION_FLAGS = 0;
        if VirtualProtect(
            slot as *mut c_void,
            size_of::<*mut c_void>(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        ) == 0
        {
            return false;
        }
        *slot = replacement;
        VirtualProtect(slot as *mut c_void, size_of::<*mut c_void>(), old, &mut old);
    }
    true
}

fn install_one<const M: usize>(module: *mut c_void) {
    // SetWindowPos first: show_window uses it.
    let mut hooked = Vec::new();
    if hook(
        module,
        "SetWindowPos",
        set_window_pos::<M> as *mut c_void,
        &REAL_SET_WINDOW_POS[M],
    ) {
        hooked.push("SetWindowPos");
    }
    if hook(
        module,
        "ShowWindow",
        show_window::<M> as *mut c_void,
        &REAL_SHOW_WINDOW[M],
    ) {
        hooked.push("ShowWindow");
    }
    if hook(
        module,
        "SetCursorPos",
        set_cursor_pos::<M> as *mut c_void,
        &REAL_SET_CURSOR_POS[M],
    ) {
        hooked.push("SetCursorPos");
    }
    if hook(
        module,
        "ClipCursor",
        clip_cursor::<M> as *mut c_void,
        &REAL_CLIP_CURSOR[M],
    ) {
        hooked.push("ClipCursor");
    }
    report(&format!(
        "{}: {}",
        MODULES[M],
        if hooked.is_empty() {
            "imports none of them".to_string()
        } else {
            hooked.join(", ")
        }
    ));
}

/// Hooks every listed module that has loaded and isn't hooked yet. Cheap once
/// all are done; call it as often as convenient (the start-up watch, the
/// setting turning on).
pub fn install_loaded() {
    fn one<const M: usize>() {
        if HOOKED[M].load(Ordering::Acquire) {
            return;
        }
        let name = format!("{}\0", MODULES[M]);
        let module = unsafe { GetModuleHandleA(name.as_ptr()) };
        if module.is_null() || HOOKED[M].swap(true, Ordering::AcqRel) {
            return;
        }
        install_one::<M>(module);
    }
    one::<0>();
    one::<1>();
    one::<2>();
    one::<3>();
}

/// Whether every listed module is hooked.
pub fn all_installed() -> bool {
    HOOKED.iter().all(|h| h.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_lose_their_activation_and_others_pass() {
        assert_eq!(without_activation(SW_SHOW), Some(SW_SHOWNA));
        assert_eq!(without_activation(SW_SHOWNORMAL), Some(SW_SHOWNOACTIVATE));
        assert_eq!(without_activation(SW_RESTORE), Some(SW_SHOWNOACTIVATE));
        assert_eq!(without_activation(0), None); // SW_HIDE
        assert_eq!(without_activation(6), None); // SW_MINIMIZE
    }

    #[test]
    fn a_move_to_the_top_goes_to_the_bottom_and_never_activates() {
        assert_eq!(kept_down(HWND_TOP, 0), (HWND_BOTTOM, SWP_NOACTIVATE));
        assert_eq!(kept_down(HWND_TOPMOST, 0), (HWND_BOTTOM, SWP_NOACTIVATE));
        // A move or resize that leaves the z-order alone stays one.
        assert_eq!(
            kept_down(HWND_TOP, SWP_NOZORDER | SWP_NOMOVE),
            (HWND_TOP, SWP_NOZORDER | SWP_NOMOVE | SWP_NOACTIVATE)
        );
    }

    #[test]
    fn every_module_has_its_slots() {
        assert_eq!(HOOKED.len(), MODULES.len());
        assert_eq!(REAL_SET_CURSOR_POS.len(), MODULES.len());
    }
}
