//! Keeps the demo player's VCR bar open when ESC is pressed, on the 25th
//! Anniversary build (issue #369, the one thing keeping movie work on the
//! pre-Anniversary install).
//!
//! ## Why the bar closes
//!
//! The VCR bar is `CDemoPlayerDialog`, a vgui2 `Frame` in
//! `valve\cl_dlls\GameUI.dll`. It overrides no key handling, so ESC reaches
//! `Frame::OnKeyCodeTyped` (vftable slot 100). On ESC (vgui2 `KEY_ESCAPE`,
//! 70), when the engine's surface supports the escape key, that function:
//!
//! - **pre-Anniversary** (`GameUI.dll` +0x4d530): posts `Command "Cancel"`
//!   to the frame, which the dialog ignores;
//! - **25th Anniversary** (+0x54930): first posts `CloseFrameButtonPressed`
//!   -- the message the frame's own close button sends -- and then the same
//!   `Cancel`. The dialog closes itself.
//!
//! ## The fix: one vftable slot
//!
//! `CDemoPlayerDialog`'s own vftable slot 100 is pointed at a stub that
//! returns for `KEY_ESCAPE` and jumps to the original for every other key.
//! The demo dialog gets the pre-Anniversary behaviour, apart from the
//! `Cancel` it ignored anyway. Every other dialog shares `Frame`'s function,
//! so patching its code would take ESC-to-close away from all of them;
//! the class' own vftable touches only this one.
//!
//! Before writing, it checks that it has the build it describes: the
//! vftable's RTTI names `CDemoPlayerDialog`, the slot holds the function
//! [`PATTERN`] finds (exactly once), and that function posts
//! `CloseFrameButtonPressed`. The pre-Anniversary GameUI fails the first
//! check, so there this does nothing, which is also what it needs there.
//! `tools/verify_demo_dialog_esc.py` checks all of it against both DLLs.
//!
//! It is on by default, since it only restores the old behaviour;
//! `GOLDSRC_HOOKS_DEMO_DIALOG_ESC=0` turns it off. Not live-tested: the
//! hooks don't run on the Anniversary install yet (#370).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc,
};

use crate::scan;

/// `CDemoPlayerDialog`'s vftable in the 25th Anniversary `GameUI.dll`.
const VFTABLE_RVA: usize = 0x9a350;
/// The decorated name its RTTI must carry.
const CLASS: &str = ".?AVCDemoPlayerDialog@@";
/// `Frame::OnKeyCodeTyped`'s slot.
const SLOT: usize = 100;
/// vgui2's `KEY_ESCAPE`.
const KEY_ESCAPE: u8 = 0x46;

/// `Frame::OnKeyCodeTyped`'s entry in the Anniversary build, through its
/// `cmp [ebp+8], KEY_ESCAPE; jne`: the SEH frame and stack cookie with their
/// addresses wildcarded.
const PATTERN: &str = "55 8B EC 6A FF 68 ?? ?? ?? ?? 64 A1 00 00 00 00 50 56 A1 ?? ?? ?? ?? \
                       33 C5 50 8D 45 F4 64 A3 00 00 00 00 8B F1 83 7D 08 46 0F 85";
/// Where, from that entry, the function pushes the message name.
const CLOSE_PUSH_OFFSET: usize = 0x62;
const CLOSE_MESSAGE: &[u8] = b"CloseFrameButtonPressed\0";

/// Whether to install at all -- `GOLDSRC_HOOKS_DEMO_DIALOG_ESC=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Where the stub jumps for every key but ESC.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// ESC presses the stub kept from closing the bar, for the status line.
static KEPT_OPEN: AtomicU32 = AtomicU32::new(0);

/// Set once the attempt is over, installed or not, so [`poll`] stops.
static DONE: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

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

/// The decorated class name `vftable[-1]`'s RTTI leads to: MSVC's 32-bit
/// layout, as `spectator_bars.rs` and `hudelement.rs` read it. Unlike those,
/// this also runs on a GameUI it doesn't describe (the pre-Anniversary one),
/// where the dwords it follows are whatever happens to be there, so every
/// pointer is checked to lie inside the image before it is read.
///
/// Safety: `[base, base + size)` must be the mapped module.
unsafe fn rtti_class_name(base: usize, size: usize, vftable: usize) -> Option<String> {
    let inside =
        |at: usize, len: usize| (at >= base && at.checked_add(len)? <= base + size).then_some(());
    unsafe {
        inside(vftable.checked_sub(4)?, 4)?;
        let locator = read_u32(vftable - 4) as usize;
        inside(locator, 0x10)?;
        if read_u32(locator) != 0 {
            return None;
        }
        let name_at = (read_u32(locator + 0x0c) as usize).checked_add(8)?;
        inside(name_at, 1)?;
        let bytes =
            std::slice::from_raw_parts(name_at as *const u8, (base + size - name_at).min(128));
        let end = bytes.iter().position(|&b| b == 0)?;
        std::str::from_utf8(&bytes[..end]).ok().map(str::to_owned)
    }
}

/// The stub, hand-assembled. `thiscall` with one argument: `ecx` is the
/// dialog, `[esp+4]` the key code, and the callee pops it.
///
/// ```asm
///         cmp  dword ptr [esp+4], KEY_ESCAPE
///         jne  .game
///         lock inc dword ptr [KEPT_OPEN]
///         ret  4
/// .game:  jmp  dword ptr [ORIGINAL]
/// ```
fn stub(original: usize, kept_open: usize) -> Vec<u8> {
    let mut code = vec![0x83, 0x7c, 0x24, 0x04, KEY_ESCAPE]; // cmp dword ptr [esp+4], imm8
    code.extend_from_slice(&[0x75, 0x0a]); // jne .game (17)
    code.extend_from_slice(&[0xf0, 0xff, 0x05]); // lock inc dword ptr [abs32]
    code.extend_from_slice(&(kept_open as u32).to_le_bytes());
    code.extend_from_slice(&[0xc2, 0x04, 0x00]); // ret 4
    debug_assert_eq!(code.len(), 17);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(original as u32).to_le_bytes());
    code
}

/// Checks the loaded GameUI is the build this describes, and returns the
/// slot's address and the function it holds.
fn check(base: usize) -> Result<(usize, usize), String> {
    // Safety: `base` is a module handle the loader gave us.
    let size = unsafe { image_size(base) }.ok_or("GameUI.dll has no PE header")?;
    if VFTABLE_RVA + (SLOT + 1) * 4 > size {
        return Err(format!(
            "GameUI.dll is too small ({size:#x} bytes) to be the 25th Anniversary one"
        ));
    }
    let vftable = base + VFTABLE_RVA;
    // Safety: the walk checks every pointer against the image first.
    let name = unsafe { rtti_class_name(base, size, vftable) };
    if name.as_deref() != Some(CLASS) {
        return Err(format!(
            "GameUI.dll +{VFTABLE_RVA:#x} identifies as {name:?}, not {CLASS} -- not the 25th Anniversary GameUI (the pre-Anniversary one doesn't need this)"
        ));
    }
    let slot = vftable + SLOT * 4;
    let present = unsafe { read_u32(slot) } as usize;
    // Safety: `base` is a module handle the loader gave us.
    let expected = unsafe { scan::find_unique(base, PATTERN) }?;
    if present != expected {
        return Err(format!(
            "slot {SLOT} holds +{:#x}, not Frame::OnKeyCodeTyped at +{:#x} -- something else has patched it",
            present.wrapping_sub(base),
            expected - base
        ));
    }
    let push = expected + CLOSE_PUSH_OFFSET;
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
    Ok((slot, expected))
}

fn install(base: usize) -> Result<usize, String> {
    let (slot, original) = check(base)?;
    ORIGINAL.store(original, Ordering::Release);
    let code = stub(ORIGINAL.as_ptr() as usize, KEPT_OPEN.as_ptr() as usize);
    // Safety: a fresh page we own, sized for the bytes copied in.
    let page = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            code.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        )
    };
    if page.is_null() {
        return Err("could not allocate an executable page for the stub".to_string());
    }
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), page as *mut u8, code.len()) };
    // Safety: `slot` is the dword checked above.
    if !unsafe { crate::patch::write_code_bytes(slot, &(page as u32).to_le_bytes()) } {
        return Err("could not make the vftable writable".to_string());
    }
    Ok(original - base)
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
                "demo_dialog_esc: off (GOLDSRC_HOOKS_DEMO_DIALOG_ESC=0) -- on the 25th Anniversary build ESC closes the demo player (#369)",
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
        Ok(original) => {
            INSTALLED.store(true, Ordering::Relaxed);
            format!(
                "demo_dialog_esc: ESC no longer closes the demo player (CDemoPlayerDialog slot {SLOT}, Frame::OnKeyCodeTyped +{original:#x}) (#369)"
            )
        }
        Err(why) => format!("demo_dialog_esc: not installed -- {why}"),
    };
    unsafe { crate::debug::report(&message) };
}

/// The `dodstudio_debug_status` line, or `None` when it isn't installed.
pub fn status_line() -> Option<String> {
    INSTALLED.load(Ordering::Relaxed).then(|| {
        format!(
            "demo player ESC fix: on, kept the VCR bar open {} time(s)",
            KEPT_OPEN.load(Ordering::Relaxed)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        #[rustfmt::skip]
        assert_eq!(
            stub(0x1111_1111, 0x2222_2222),
            vec![
                0x83, 0x7c, 0x24, 0x04, 0x46,             //  0 cmp  dword ptr [esp+4], 0x46
                0x75, 0x0a,                               //  5 jne  .game (17)
                0xf0, 0xff, 0x05, 0x22, 0x22, 0x22, 0x22, //  7 lock inc [KEPT_OPEN]
                0xc2, 0x04, 0x00,                         // 14 ret  4
                0xff, 0x25, 0x11, 0x11, 0x11, 0x11,       // 17 .game: jmp dword [ORIGINAL]
            ]
        );
    }

    #[test]
    fn the_pattern_parses_and_ends_on_the_escape_compare() {
        assert!(scan::Pattern::parse(PATTERN).is_ok());
        assert!(PATTERN.contains(&format!("83 7D 08 {KEY_ESCAPE:02X} 0F 85")));
    }
}
