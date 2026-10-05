//! Hides DoD's spectator bars -- the two dark bands across the top and bottom
//! of the screen while spectating, in a demo or live, and everything drawn on
//! them -- with a cvar, on screen, without a capture running (issue #328).
//!
//! `dodstudio_hide_spectator_bars 1` hides the whole spectator panel: the
//! bands, the score, timer and player name on them, the DUCK menu row and the
//! inset outline. An earlier version had a second cvar for the bands alone,
//! with the text left floating; nobody wanted that, and it went.
//!
//! HLAE's `mirv_movie_hidepanels` only leaves panels out of what it records,
//! and its `mirv_disable_specmenu` does not support DoD.
//!
//! ## What the bands are
//!
//! Two plain `vgui2::Panel`s that `CSpectatorGUI`'s constructor creates
//! (`client.dll+0x82b50`): `topbar` and `bottombarblank`, both given
//! `SetBgColor(0, 0, 0, 196)` by its `ApplySchemeSettings` (`+0x83030`) and
//! painted by the stock `Panel::PaintBackground`. `Spectator.res` renames the
//! first `TopBar` (its `fieldName`). The frame around them, `SpectatorGUI`,
//! paints nothing itself, and `CBottomBar` (`BottomBar`) is the transparent
//! combobox row that DUCK brings up.
//!
//! ## Why the `.res` edit hides one band and not the other
//!
//! `"visible" "0"` reaches both panels. Nothing ever shows `TopBar` again, so
//! it holds. For `bottombarblank` the constructor itself calls
//! `SetVisible(true)` after loading the `.res` (`+0x82e39`), and
//! `CSpectatorGUI::OnThink` (`+0x82ef0`) re-sizes and re-positions it at the
//! bottom edge on top of that, which defeats `tall 0` / `ypos 9999` too.
//!
//! ## How this hides them
//!
//! Every panel's paint, in every module, goes through one function: vgui2's
//! `IPanel::PaintTraverse` (interface `VGUI_Panel007`, vtable slot 41).
//! `client.dll`'s `Panel::PaintTraverse` calls it for each child
//! (`client+0x576f2`), and the engine calls it for the root (`hw+0x3cf1` on
//! the pre-Anniversary build). So this swaps that one vtable slot for a
//! filter: the `SpectatorGUI` frame is not painted, and nor is anything under
//! it, since children are only painted from inside their parent's paint.
//! Nothing about the panel changes -- no visibility flag, no size, no `.res`
//! -- so there is nothing for the game to put back, and turning the cvar off
//! shows the panel again on the next frame.
//!
//! The slot numbers are the same on both builds' `vgui2.dll` (the files
//! differ; the interface does not, and `client.dll`, compiled against it, is
//! byte-identical in both installs). `tools/verify_vgui2_ipanel.py` checks
//! them against the real files. At install the vtable has to identify itself
//! by RTTI as `VPanelWrapper`, the two slots used have to point into
//! `vgui2.dll`'s own code, and each has to end in the `ret` its argument
//! count demands; anything else is refused and logged, and nothing is
//! patched.
//!
//! The hook goes in the first time the cvar is turned on and stays for the
//! session; with the cvar off it costs one flag read per painted panel.
//!
//! ## What was tried before
//!
//! A redirect of `CDoDSpectatorGUI`'s and `CBottomBar`'s "`SetVisible`"
//! vtable slot, live-tested twice in 2026-09 and never called. Slot 8 of a
//! vgui2 `Panel` is `OnChildAdded`; `SetVisible` is slot 29, and neither
//! class paints the bands anyway. `docs/goldsrc_spectator_bars.md` keeps
//! that history.

// The hook itself is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::CvarSPartial;
use crate::names::console_name;

/// Hides the spectator panel: the bands and everything on them.
pub const NAME: &str = console_name!("hide_spectator_bars");

/// `IPanel` (`VGUI_Panel007`) vtable slots, 0-based; slot 0 is the virtual
/// destructor. The same in both builds' `vgui2.dll`.
const SLOT_GET_NAME: usize = 36;
const SLOT_PAINT_TRAVERSE: usize = 41;
/// How many slots the vtable has.
const SLOT_COUNT: usize = 60;

/// What each slot's function pops: `this` is in `ecx`, so a `VPANEL` alone is
/// `ret 4`, and `PaintTraverse(VPANEL, bool, bool)` is `ret 0xc`.
const RET_GET_NAME: u16 = 4;
const RET_PAINT_TRAVERSE: u16 = 0xc;

/// The vtable's class, by RTTI.
const WRAPPER_CLASS: &str = ".?AVVPanelWrapper@@";

/// The spectator frame. The bands (`TopBar`, `bottombarblank`), the labels
/// and the menu row are all its children, so not painting it hides them all.
const FRAME: &[u8] = b"SpectatorGUI";

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static HIDE: AtomicBool = AtomicBool::new(false);

/// The stock functions, captured at install.
static STOCK_PAINT_TRAVERSE: AtomicUsize = AtomicUsize::new(0);
static GET_NAME: AtomicUsize = AtomicUsize::new(0);
/// Set once install has been tried and refused, so it is tried and logged once.
static INSTALL_FAILED: AtomicBool = AtomicBool::new(false);
/// Paints skipped so far, for `dodstudio_debug_status`.
static SKIPPED: AtomicU32 = AtomicU32::new(0);

/// Called once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn installed() -> bool {
    STOCK_PAINT_TRAVERSE.load(Ordering::Acquire) != 0
}

/// Whether the spectator panel is hidden right now: the cvar is on and the
/// filter that does the hiding is in place. `spectator_hud` lays out the top
/// of the screen by it.
pub fn hiding() -> bool {
    HIDE.load(Ordering::Relaxed) && installed()
}

/// Whether a panel is the one to leave unpainted.
fn hides(name: &[u8]) -> bool {
    name.eq_ignore_ascii_case(FRAME)
}

/// The size a function's first `ret imm16` pops, looking at most `window`
/// bytes in. Both builds' wrappers are short, straight-line forwarders, so
/// the first `C2 xx 00` is the function's own return; a plain `ret` (`C3`)
/// first means it pops nothing. `tools/verify_vgui2_ipanel.py` runs the same
/// scan against the real files.
fn first_ret_size(code: &[u8]) -> Option<u16> {
    let mut i = 0;
    while i < code.len() {
        match code[i] {
            0xc3 => return Some(0),
            0xc2 if i + 2 < code.len() && code[i + 2] == 0 => return Some(code[i + 1] as u16),
            _ => i += 1,
        }
    }
    None
}

/// How far into a wrapper to look for its `ret`.
const RET_WINDOW: usize = 64;

#[cfg(target_arch = "x86")]
mod hook {
    use super::*;

    type PaintTraverseFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, u32, u32);
    type VPanelToPtrFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_void) -> *mut c_void;

    /// Whether `IPanel::GetName(vpanel)` is the spectator frame's.
    unsafe fn is_the_frame(ipanel: *mut c_void, vpanel: *mut c_void) -> bool {
        // Safety: captured from the vtable at install, checked there.
        let get_name: VPanelToPtrFn =
            unsafe { std::mem::transmute(GET_NAME.load(Ordering::Relaxed)) };
        let name = unsafe { get_name(ipanel, vpanel) } as *const c_char;
        !name.is_null() && hides(unsafe { CStr::from_ptr(name) }.to_bytes())
    }

    /// The filter. `bool`s arrive as 4-byte stack slots and are passed on
    /// untouched; `thiscall` pops the three arguments either way, as the
    /// stock function's `ret 0xc` does.
    pub(super) unsafe extern "thiscall" fn paint_traverse(
        ipanel: *mut c_void,
        vpanel: *mut c_void,
        force_repaint: u32,
        allow_force: u32,
    ) {
        if HIDE.load(Ordering::Relaxed)
            && !vpanel.is_null()
            && unsafe { is_the_frame(ipanel, vpanel) }
        {
            SKIPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // Safety: the stock function, captured at install.
        let stock: PaintTraverseFn =
            unsafe { std::mem::transmute(STOCK_PAINT_TRAVERSE.load(Ordering::Relaxed)) };
        unsafe { stock(ipanel, vpanel, force_repaint, allow_force) }
    }

    /// The decorated class name behind `vftable[-1]`'s RTTI (MSVC, 32-bit).
    ///
    /// Safety: `vftable - 4` must be readable.
    unsafe fn rtti_class_name(
        vftable: usize,
        inside: impl Fn(usize, usize) -> bool,
    ) -> Option<String> {
        unsafe {
            let locator = (vftable as *const u32).sub(1).read_unaligned() as usize;
            if !inside(locator, 0x10) || (locator as *const u32).read_unaligned() != 0 {
                return None;
            }
            let descriptor = ((locator + 0x0c) as *const u32).read_unaligned() as usize;
            if !inside(descriptor, 8 + WRAPPER_CLASS.len() + 1) {
                return None;
            }
            CStr::from_ptr((descriptor + 8) as *const c_char)
                .to_str()
                .ok()
                .map(str::to_owned)
        }
    }

    pub(super) fn install() -> Result<String, String> {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

        let module = unsafe { GetModuleHandleA(c"vgui2.dll".as_ptr() as *const u8) };
        if module.is_null() {
            return Err("vgui2.dll is not loaded".to_string());
        }
        let base = module as usize;
        let create = unsafe { GetProcAddress(module, c"CreateInterface".as_ptr() as *const u8) }
            .ok_or("vgui2.dll exports no CreateInterface")?;
        // Safety: the Source/GoldSrc factory signature.
        let create: unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void =
            unsafe { std::mem::transmute(create) };
        let ipanel = unsafe { create(c"VGUI_Panel007".as_ptr(), std::ptr::null_mut()) };
        if ipanel.is_null() {
            return Err("vgui2.dll has no VGUI_Panel007".to_string());
        }

        let size = unsafe { crate::pe::image_size(base as *mut u8) }
            .ok_or("vgui2.dll has no PE header")?;
        let inside = |address: usize, len: usize| address >= base && address + len <= base + size;
        let (code_rva, code_len) = unsafe { crate::pe::code_range(base as *mut u8) }
            .ok_or("vgui2.dll has no code section")?;
        let in_code = |address: usize| {
            address >= base + code_rva && address + RET_WINDOW <= base + code_rva + code_len
        };

        // Safety: a live object's first dword is its vtable.
        let vtable = unsafe { (ipanel as *const usize).read_unaligned() };
        if !inside(vtable.wrapping_sub(4), (SLOT_COUNT + 1) * 4) {
            return Err(format!(
                "the VGUI_Panel007 vtable {vtable:#x} is not inside vgui2.dll"
            ));
        }
        // Safety: checked to lie inside the image.
        let class = unsafe { rtti_class_name(vtable, inside) };
        if class.as_deref() != Some(WRAPPER_CLASS) {
            return Err(format!(
                "the VGUI_Panel007 vtable identifies as {class:?}, not {WRAPPER_CLASS}"
            ));
        }

        let slot =
            |index: usize| unsafe { ((vtable + index * 4) as *const usize).read_unaligned() };
        let mut found = [0usize; 2];
        for (i, (index, ret, what)) in [
            (SLOT_GET_NAME, RET_GET_NAME, "GetName"),
            (SLOT_PAINT_TRAVERSE, RET_PAINT_TRAVERSE, "PaintTraverse"),
        ]
        .into_iter()
        .enumerate()
        {
            let function = slot(index);
            if !in_code(function) {
                return Err(format!(
                    "slot {index} ({what}) points at {function:#x}, outside vgui2.dll's code -- something else has hooked it"
                ));
            }
            // Safety: inside the code section, with the window in bounds.
            let code = unsafe { std::slice::from_raw_parts(function as *const u8, RET_WINDOW) };
            let pops = first_ret_size(code);
            if pops != Some(ret) {
                return Err(format!(
                    "slot {index} should be {what} (ret {ret:#x}), but it returns with {pops:?}"
                ));
            }
            found[i] = function;
        }

        GET_NAME.store(found[0], Ordering::Release);
        let hook = paint_traverse as *const () as usize;
        let slot_address = vtable + SLOT_PAINT_TRAVERSE * 4;
        STOCK_PAINT_TRAVERSE.store(found[1], Ordering::Release);
        // Safety: a vtable slot inside vgui2.dll, checked above; the write
        // happens on the main thread, which is also the only one that paints.
        if !unsafe { crate::patch::write_code_bytes(slot_address, &(hook as u32).to_le_bytes()) } {
            STOCK_PAINT_TRAVERSE.store(0, Ordering::Release);
            return Err("could not make the VGUI_Panel007 vtable writable".to_string());
        }
        Ok(format!(
            "IPanel::PaintTraverse (vgui2.dll+{:#x}, slot {SLOT_PAINT_TRAVERSE}) now goes through the spectator-panel filter",
            found[1] - base
        ))
    }
}

#[cfg(not(target_arch = "x86"))]
mod hook {
    pub(super) fn install() -> Result<String, String> {
        Err("only a 32-bit x86 build can hook vgui2.dll".to_string())
    }
}

/// Reads the cvar and installs the filter the first time it is on. Called
/// every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if HIDE.swap(wanted, Ordering::Relaxed) != wanted {
        let state = if wanted { "1 (hidden)" } else { "0 (shown)" };
        unsafe { crate::debug::report(&format!("spectator_bars: {NAME} = {state}")) };
    }
    if !wanted || installed() || INSTALL_FAILED.load(Ordering::Relaxed) {
        return;
    }
    let report = match hook::install() {
        Ok(what) => format!("spectator_bars: {what} (#328)"),
        Err(why) => {
            INSTALL_FAILED.store(true, Ordering::Relaxed);
            let line =
                format!("spectator_bars: not installed -- {why}; {NAME} does nothing this session");
            crate::commands::console_print(&format!("{line}\n"));
            line
        }
    };
    unsafe { crate::debug::report(&report) };
}

/// One `dodstudio_debug_status` line, once the cvar has been turned on.
pub fn status_line() -> Option<String> {
    if INSTALL_FAILED.load(Ordering::Relaxed) {
        return Some(format!(
            "spectator bars: the filter could not be installed (see the hook log); {NAME} does nothing"
        ));
    }
    if !installed() {
        return None;
    }
    let hidden = HIDE.load(Ordering::Relaxed);
    Some(format!(
        "spectator bars: {} ({NAME} = {}; {} panel paint(s) skipped so far)",
        if hidden {
            "the spectator panel is hidden"
        } else {
            "nothing hidden"
        },
        hidden as u8,
        SKIPPED.load(Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_spectator_frame_is_left_unpainted() {
        assert!(hides(b"SpectatorGUI"));
        assert!(hides(b"spectatorgui"));
        // Its children are never reached once the frame is skipped, and are
        // not matched on their own; the scoreboard has a TopBar too.
        for other in [
            &b"TopBar"[..],
            b"bottombarblank",
            b"playerlabel",
            b"BottomBar",
            b"ClientScoreBoard",
            b"",
        ] {
            assert!(!hides(other), "{other:?}");
        }
    }

    #[test]
    fn the_ret_scan_reads_a_wrappers_own_return() {
        // Pre-Anniversary vgui2.dll+0x15ed0, IPanel::PaintTraverse.
        let pre = [
            0x8b, 0x54, 0x24, 0x04, 0x8b, 0x01, 0x52, 0xff, 0x90, 0xe8, 0x00, 0x00, 0x00, 0x8b,
            0x4c, 0x24, 0x0c, 0x8b, 0x10, 0x51, 0x8b, 0x4c, 0x24, 0x0c, 0x51, 0x8b, 0xc8, 0xff,
            0x52, 0x0c, 0xc2, 0x0c, 0x00,
        ];
        assert_eq!(first_ret_size(&pre), Some(RET_PAINT_TRAVERSE));
        // A getter: mov ecx,[esp+4]; mov eax,[ecx]; call [eax+0x78]; ret 4.
        let getter = [
            0x8b, 0x4c, 0x24, 0x04, 0x8b, 0x01, 0xff, 0x50, 0x78, 0xc2, 0x04, 0x00,
        ];
        assert_eq!(first_ret_size(&getter), Some(RET_GET_NAME));
        assert_eq!(first_ret_size(&[0x90, 0xc3]), Some(0));
        assert_eq!(first_ret_size(&[0x90, 0x90]), None);
    }

    #[test]
    fn the_slots_are_the_verified_ones() {
        // tools/verify_vgui2_ipanel.py reads these two out of this file.
        assert_eq!((SLOT_GET_NAME, SLOT_PAINT_TRAVERSE), (36, 41));
        const { assert!(SLOT_PAINT_TRAVERSE < SLOT_COUNT) };
    }
}
