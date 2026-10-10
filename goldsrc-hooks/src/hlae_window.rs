//! Keeps a background capture in the background: with
//! `dodstudio_run_in_background` on, HLAE no longer pulls the game window to
//! the front when a recording starts, and a minimised game is put back behind
//! the other windows instead, since a minimised game draws nothing (#434).
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
//! function then skips itself, as it only undoes an entry that ran), and a
//! game that gets minimised is shown again, without taking the foreground, at
//! the bottom of the window stack.
//!
//! Found by a byte pattern in HLAE's DLL. Another HLAE version may not match:
//! the log says so, and recording pulls the window to the front as before.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

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

const SW_SHOWNOACTIVATE: i32 = 4;
const SW_MINIMIZE: i32 = 6;
const HWND_BOTTOM: isize = 1;
const SWP_NOSIZE: u32 = 0x1;
const SWP_NOMOVE: u32 = 0x2;
const SWP_NOACTIVATE: u32 = 0x10;

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
    fn GetClassNameA(hwnd: *mut c_void, name: *mut u8, max: i32) -> i32;
    fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
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

/// Shows a minimised game again behind every other window, without taking
/// the foreground.
fn put_behind(hwnd: *mut c_void) {
    unsafe {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
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

/// Gets the game out of the way: minimising hands the foreground to the next
/// window, and the next check puts the game back behind it.
pub fn step_aside() {
    if let Some(hwnd) = window() {
        unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
        report("stepped aside: the game runs behind the other windows");
    }
}

/// Called every frame from `run_in_background::poll` with the setting.
pub fn poll(on: bool) {
    if on != SKIPPING.load(Ordering::Acquire) {
        set_skipping(on);
    }
    if !on
        || !FRAMES
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(CHECK_EVERY)
    {
        return;
    }
    if let Some(hwnd) = window()
        && unsafe { IsIconic(hwnd) } != 0
    {
        put_behind(hwnd);
        report(
            "the game was minimised, which stops it drawing; it runs behind the other windows instead",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_starts_with_the_byte_it_swaps() {
        assert!(ENTER_PATTERN.starts_with("55 "));
        assert!(crate::scan::Pattern::parse(ENTER_PATTERN).is_ok());
    }
}
