//! Stops ESC closing GameUI's windows on the 25th Anniversary build (issues
//! #369 and #408): the demo player's VCR bar, the events list, the Load Demo
//! window, the console and every other vgui2 `Frame` in
//! `valve\cl_dlls\GameUI.dll`.
//!
//! ## Why they close
//!
//! None of those windows handles keys itself, so ESC reaches
//! `Frame::OnKeyCodeTyped` (vftable slot 100, the one function every GameUI
//! `Frame` inherits). On ESC (vgui2 `KEY_ESCAPE`, 70), when the engine's
//! surface supports the escape key, that function:
//!
//! - **pre-Anniversary** (`GameUI.dll` +0x4d530): posts `Command "Cancel"` to
//!   the window. A window with a Cancel button (Options, a message box)
//!   closes on it; the demo windows and the console ignore it.
//! - **25th Anniversary** (+0x54930): first posts `CloseFrameButtonPressed`
//!   -- the message the window's own close button sends -- and then the same
//!   `Cancel`. Every window closes.
//!
//! ## The fix: skip one message
//!
//! Two bytes at the start of the `CloseFrameButtonPressed` block become a
//! short jump to the `Cancel` block right after it, so ESC does exactly what
//! it did on the pre-Anniversary build, in every window at once. A window of
//! our own made from GameUI's `Frame` shares the function and is covered too;
//! one made from `client.dll`'s copy, or drawn from scratch, is not (#408).
//!
//! The first version (#396) repointed `CDemoPlayerDialog`'s own vftable slot
//! at a stub and fixed only the VCR bar. That was live-tested on the
//! Anniversary build; then the other windows turned out to close too.
//!
//! Before writing, it checks the loaded function is the one it describes:
//! [`PATTERN`] matches exactly once, both blocks start where [`CLOSE_BLOCK`]
//! and [`CANCEL_BLOCK`] say (each opens with `push 0x18`, the size of the
//! `KeyValues` it allocates), and the first pushes `"CloseFrameButtonPressed"`.
//! The pre-Anniversary GameUI has no match, so there this does nothing, which
//! is also what it needs there. `tools/verify_frame_esc.py` checks all of it
//! against both DLLs.
//!
//! It is on by default, since it only restores the old behaviour;
//! `GOLDSRC_HOOKS_FRAME_ESC=0` turns it off.

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;

use crate::scan;

/// `Frame::OnKeyCodeTyped`'s entry in the Anniversary build, through its
/// `cmp [ebp+8], KEY_ESCAPE; jne`: the SEH frame and stack cookie with their
/// addresses wildcarded.
const PATTERN: &str = "55 8B EC 6A FF 68 ?? ?? ?? ?? 64 A1 00 00 00 00 50 56 A1 ?? ?? ?? ?? \
                       33 C5 50 8D 45 F4 64 A3 00 00 00 00 8B F1 83 7D 08 46 0F 85";
/// vgui2's `KEY_ESCAPE`, the compare [`PATTERN`] ends on. Named for the test
/// that pins it there, and for `tools/verify_frame_esc.py`.
#[cfg(test)]
const KEY_ESCAPE: u8 = 0x46;
/// Where, from the entry, the `CloseFrameButtonPressed` block starts.
const CLOSE_BLOCK: usize = 0x4a;
/// Where the `Command "Cancel"` block starts, right after it.
const CANCEL_BLOCK: usize = 0x8c;
/// Where the close block pushes the message name.
const CLOSE_PUSH: usize = 0x62;
const CLOSE_MESSAGE: &[u8] = b"CloseFrameButtonPressed\0";
/// `push 0x18`: each block opens by allocating a 24-byte `KeyValues`.
const BLOCK_START: [u8; 2] = [0x6a, 0x18];

/// Whether to install at all -- `GOLDSRC_HOOKS_FRAME_ESC=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Set once the attempt is over, installed or not, so [`poll`] stops.
static DONE: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// `jmp short` from the close block to the cancel block.
fn skip() -> [u8; 2] {
    [0xeb, (CANCEL_BLOCK - (CLOSE_BLOCK + 2)) as u8]
}

unsafe fn read_u32(address: usize) -> u32 {
    unsafe { (address as *const u32).read_unaligned() }
}

/// The loaded image's size, SizeOfImage from its PE header.
///
/// Safety: `base` must be a module handle the loader gave us.
unsafe fn image_size(base: usize) -> Option<usize> {
    unsafe {
        let nt = base + read_u32(base + 0x3c) as usize;
        // The "PE" signature, then the 20-byte file header, then the
        // optional header, whose SizeOfImage is 0x38 in: 0x50 in all.
        (read_u32(nt) == 0x0000_4550).then(|| read_u32(nt + 0x50) as usize)
    }
}

/// Checks the loaded GameUI has the function this describes, and returns
/// its entry.
fn check(base: usize) -> Result<usize, String> {
    // Safety: `base` is a module handle the loader gave us.
    let size = unsafe { image_size(base) }.ok_or("GameUI.dll has no PE header")?;
    // Safety: `base` is a module handle the loader gave us.
    let entry = unsafe { scan::find_unique(base, PATTERN) }.map_err(|why| {
        format!(
            "{why} -- not the 25th Anniversary GameUI (the pre-Anniversary one doesn't need this)"
        )
    })?;
    // Safety: both spans are inside the matched function.
    let starts = [CLOSE_BLOCK, CANCEL_BLOCK].map(|at| unsafe {
        std::slice::from_raw_parts((entry + at) as *const u8, 2) == BLOCK_START
    });
    if starts != [true, true] {
        return Err(format!(
            "Frame::OnKeyCodeTyped's blocks don't start at +{CLOSE_BLOCK:#x} and +{CANCEL_BLOCK:#x}"
        ));
    }
    let push = entry + CLOSE_PUSH;
    // Safety: inside the matched function; the pushed address is checked
    // against the image before it is read.
    let pushed = unsafe {
        (*(push as *const u8) == 0x68)
            .then(|| read_u32(push + 1) as usize)
            .filter(|&at| at >= base && at + CLOSE_MESSAGE.len() <= base + size)
            .map(|at| std::slice::from_raw_parts(at as *const u8, CLOSE_MESSAGE.len()))
    };
    if pushed != Some(CLOSE_MESSAGE) {
        return Err(format!(
            "Frame::OnKeyCodeTyped doesn't push \"CloseFrameButtonPressed\" at +{:#x}",
            push - base
        ));
    }
    Ok(entry)
}

fn install(base: usize) -> Result<usize, String> {
    let entry = check(base)?;
    // Safety: the two bytes checked above, one whole instruction.
    if !unsafe { crate::patch::write_code_bytes(entry + CLOSE_BLOCK, &skip()) } {
        return Err("could not make Frame::OnKeyCodeTyped writable".to_string());
    }
    Ok(entry - base)
}

/// Installs the fix once `GameUI.dll` is loaded. Runs every frame from
/// `commands::poll`; one atomic load once the attempt is over.
pub fn poll() {
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    if !ENABLED.load(Ordering::Relaxed) {
        DONE.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report(
                "frame_esc: off (GOLDSRC_HOOKS_FRAME_ESC=0) -- on the 25th Anniversary build ESC closes GameUI's windows (#369, #408)",
            )
        };
        return;
    }
    // Safety: a plain module lookup.
    let handle = unsafe { GetModuleHandleA(c"GameUI.dll".as_ptr() as *const u8) };
    if handle.is_null() {
        return; // not loaded yet: try again next frame
    }
    DONE.store(true, Ordering::Relaxed);
    let message = match install(handle as usize) {
        Ok(entry) => {
            INSTALLED.store(true, Ordering::Relaxed);
            format!(
                "frame_esc: ESC no longer closes GameUI's windows (Frame::OnKeyCodeTyped +{entry:#x} skips CloseFrameButtonPressed) (#369, #408)"
            )
        }
        Err(why) => format!("frame_esc: not installed -- {why}"),
    };
    unsafe { crate::debug::report(&message) };
}

/// The `dodstudio_debug_status` line, or `None` when it isn't installed.
pub fn status_line() -> Option<String> {
    INSTALLED.load(Ordering::Relaxed).then(|| {
        "window ESC fix: on (ESC sends Cancel only, as on the pre-Anniversary build)".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skip_lands_on_the_cancel_block() {
        let [op, rel] = skip();
        assert_eq!(op, 0xeb, "jmp rel8");
        assert_eq!(CLOSE_BLOCK + 2 + rel as usize, CANCEL_BLOCK);
        assert_eq!(
            skip().len(),
            BLOCK_START.len(),
            "overwrites exactly one instruction"
        );
    }

    #[test]
    fn the_pattern_parses_and_ends_on_the_escape_compare() {
        assert!(scan::Pattern::parse(PATTERN).is_ok());
        assert!(PATTERN.contains(&format!("83 7D 08 {KEY_ESCAPE:02X} 0F 85")));
    }
}
