//! `ds_hud_hide_hints`: hide DoD's auto-help hint panel, the boxed tip with a
//! soldier's head in the top-left corner that a POV demo shows wherever the
//! recording player had `_ah 1` (#286).
//!
//! ## Why `_ah 0` can't do it
//!
//! `client.dll` registers `_ah` and never reads it: one store of its
//! `cvar_t*` (`gHUD+0x6e2a8`), no load, no lookup by name. It only rides in
//! userinfo to the server, which decides whether to send a hint. The hint
//! then reaches the client as a `HudText` user message whose flag byte
//! (after the text) is nonzero, and that message is in the demo. So the
//! viewer's `_ah` has no say in playback.
//!
//! ## What opens the panel
//!
//! ```text
//! CHudMessage::MsgFunc_HudText        +0x25470
//!     flag > 0 and gViewPortInterface: gViewPortInterface->ShowHint(text)
//! DoDViewport::ShowHint(text)         +0x1f8c0   __thiscall, one argument
//!     CHintPanel::SetHint(team, text) +0x207b0   text, team image, a timer
//!                                                 of 0.05 s per character
//!     tail-jumps to SetVisible(true)  [vtbl+0x74], which ends `ret 4`
//! ```
//!
//! Nothing else opens it, and the panel closes itself when its timer runs
//! out. A `HudText` with no flag byte is an ordinary HUD message and goes
//! another way, so map text and DoD's prompts are not touched.
//!
//! ## The patch
//!
//! `ret 4` (`C2 04 00`) over `ShowHint`'s first three bytes: the hint is
//! dropped and its one argument comes off the stack as the tail-jump's
//! `ret 4` would have taken it. The first instruction is
//! `mov eax, [team global]`, whose operand is an absolute address that
//! moves with the load base, so the stock bytes are read at the scan and
//! written back at 0, the way `spectator_text.rs` keeps its own.
//!
//! Like `crosshair.rs`, the decision comes from the bytes every frame: a
//! re-loaded `client.dll` is patched again. Turning the cvar on also closes a
//! hint already up (`SetVisible(false)` on the viewport's hint panel, the
//! slot `ShowHint` itself shows it through): its own timer can leave it up
//! for many seconds after a seek.
//!
//! `client.dll` is one file, identical in every install, so one pattern
//! covers both engine builds. `tools/verify_hint_panel_offsets.py` checks it
//! against a real `client.dll`: one match, that it is the function
//! `MsgFunc_HudText` calls through the viewport, the `ret 4` it ends in, and
//! that nothing branches into the three bytes.
//!
//! Defaults to 1: the panel is the recorder's own tutorial, it tells a viewer
//! nothing about the footage, and it covers a corner of the shot.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("hud_hide_hints");

/// `DoDViewport::ShowHint`, whole: from its first instruction
/// (`mov eax, [abs]`) through the team check, the `SetHint` call on
/// `[this+0x14]` (the hint panel) and the tail-jump to its `SetVisible`
/// (`[vtbl+0x74]`), so the two offsets [`close_open_hint`] uses are pinned.
const PATTERN: &str = "A1 ?? ?? ?? ?? 56 83 F8 01 8B F1 75 0C 8A 0D ?? ?? ?? ?? 84 C9 74 02 33 C0 \
                       8B 4C 24 08 51 8B 4E 14 50 E8 ?? ?? ?? ?? 8B 4E 14 5E \
                       C7 44 24 04 01 00 00 00 8B 11 FF 62 74";

/// `ret 4`, over the start of the function.
const HIDDEN: [u8; 3] = [0xc2, 0x04, 0x00];

/// `MsgFunc_HudText`'s flagged branch: `mov ecx, [gViewPortInterface]; test
/// ecx, ecx; je; mov edx, [ecx]; push edi; call [edx+0x14]` (`ShowHint`).
// The four below are only read by the 32-bit build's `close_open_hint`.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
const VIEWPORT_CALL: &str = "8B 0D ?? ?? ?? ?? 85 C9 74 08 8B 11 57 FF 52 14";
/// Where `gViewPortInterface`'s address sits in [`VIEWPORT_CALL`].
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
const VIEWPORT_AT: usize = 2;
/// The hint panel's pointer in the object `ShowHint` runs on.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
const HINT_PANEL_OFFSET: usize = 0x14;
/// `Panel::SetVisible(bool)`, the slot `ShowHint` tail-jumps through.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
const SET_VISIBLE_SLOT: usize = 0x74 / 4;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// `ShowHint`'s address, 0 before a successful scan.
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// The module base [`ADDRESS`] belongs to.
static BASE: AtomicUsize = AtomicUsize::new(0);
/// A base the scan already failed on, so it isn't repeated every frame.
static FAILED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The three stock bytes, little-endian.
static STOCK: AtomicU32 = AtomicU32::new(0);
/// Set the first time the cvar is on, so a session that never asks for it
/// never scans.
static TOUCHED: AtomicBool = AtomicBool::new(false);
/// Whether hints are blocked right now.
static HIDDEN_NOW: AtomicBool = AtomicBool::new(false);
/// Whether a failure has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

/// Called by `commands.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn pack(bytes: [u8; 3]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}

fn unpack(packed: u32) -> [u8; 3] {
    let b = packed.to_le_bytes();
    [b[0], b[1], b[2]]
}

/// `ShowHint`'s address in the module at `base`, scanning once per module.
fn resolve(base: usize) -> Result<usize, String> {
    if BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    if FAILED_BASE.load(Ordering::Acquire) == base {
        return Err("DoDViewport::ShowHint was not found in this client.dll".into());
    }
    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session. The pattern starts with the stock
    // `A1`, so a function already stubbed never matches.
    let address = match unsafe { scan::find_unique(base, PATTERN) } {
        Ok(found) => found,
        Err(why) => {
            FAILED_BASE.store(base, Ordering::Release);
            return Err(format!("could not find DoDViewport::ShowHint -- {why}"));
        }
    };
    // Safety: the pattern matched, so these bytes are mapped code.
    let present = unsafe { *(address as *const [u8; 3]) };
    STOCK.store(pack(present), Ordering::Release);
    ADDRESS.store(address, Ordering::Release);
    BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Puts `ShowHint` in the wanted state, returning whether anything was
/// written.
fn apply(base: usize, hidden: bool) -> Result<bool, String> {
    let address = resolve(base)?;
    let stock = unpack(STOCK.load(Ordering::Acquire));
    let want = if hidden { HIDDEN } else { stock };
    // Safety: `resolve` proved the span is mapped code.
    let present = unsafe { *(address as *const [u8; 3]) };
    if present == want {
        return Ok(false);
    }
    if present != stock && present != HIDDEN {
        return Err(format!(
            "DoDViewport::ShowHint starts {present:02x?}, neither the stock {stock:02x?} nor {HIDDEN:02x?} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &want) } {
        return Err("could not make DoDViewport::ShowHint writable".into());
    }
    Ok(true)
}

/// Closes a hint already on screen, so turning the cvar on doesn't leave the
/// last one up: its own timer can run several seconds, longer after a seek.
/// Only called right after the stub went in, so no new hint can follow.
#[cfg(not(target_arch = "x86"))]
fn close_open_hint(_base: usize) -> Result<bool, String> {
    Err("only the 32-bit build draws hints".into())
}

#[cfg(target_arch = "x86")]
fn close_open_hint(base: usize) -> Result<bool, String> {
    // Safety: a mapped module; scanned once per switch-on, not per frame.
    let call = unsafe { scan::find_unique(base, VIEWPORT_CALL) }
        .map_err(|why| format!("could not find gViewPortInterface -- {why}"))?;
    unsafe {
        let global = (call as *const u8)
            .add(VIEWPORT_AT)
            .cast::<usize>()
            .read_unaligned();
        let viewport = (global as *const *const u8).read_unaligned();
        if viewport.is_null() {
            return Ok(false);
        }
        let panel = viewport
            .add(HINT_PANEL_OFFSET)
            .cast::<*mut std::ffi::c_void>()
            .read_unaligned();
        if panel.is_null() {
            return Ok(false);
        }
        let vftable = *(panel as *const *const usize);
        let set_visible: unsafe extern "thiscall" fn(*mut std::ffi::c_void, u32) =
            std::mem::transmute(*vftable.add(SET_VISIBLE_SLOT));
        set_visible(panel, 0);
    }
    Ok(true)
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if wanted {
        TOUCHED.store(true, Ordering::Relaxed);
    } else if !TOUCHED.load(Ordering::Relaxed) {
        return;
    }
    let Some(base) = engine::client_module_base() else {
        return;
    };
    match apply(base, wanted) {
        Ok(wrote) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            HIDDEN_NOW.store(wanted, Ordering::Relaxed);
            if wrote {
                let closed = match wanted.then(|| close_open_hint(base)) {
                    Some(Err(why)) => format!("; a hint on screen stays up: {why}"),
                    _ => String::new(),
                };
                unsafe {
                    crate::debug::report(&format!(
                        "hint_panel: {NAME} = {} -- auto-help hints {}{closed}",
                        u8::from(wanted),
                        if wanted { "hidden" } else { "show again" }
                    ))
                };
            }
        }
        Err(why) => {
            HIDDEN_NOW.store(false, Ordering::Relaxed);
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("hint_panel: {NAME} not applied yet -- {why}"))
                };
            }
        }
    }
}

/// One line for `ds_debug_status`, once the cvar has been on.
pub fn status_line() -> Option<String> {
    if !TOUCHED.load(Ordering::Relaxed) {
        return None;
    }
    Some(if HIDDEN_NOW.load(Ordering::Relaxed) {
        format!("{NAME} = 1 -- DoDViewport::ShowHint is stubbed, so auto-help hints do not open")
    } else {
        format!("{NAME}: auto-help hints show")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_is_hex_or_wildcard_and_starts_on_the_stock_mov() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(
            tokens[0], "A1",
            "mov eax, [abs32]: the byte the stub replaces"
        );
        assert!(tokens.len() > HIDDEN.len());
        for token in &tokens {
            assert!(
                *token == "??" || u8::from_str_radix(token, 16).is_ok(),
                "{token:?}"
            );
        }
    }

    /// `ShowHint` takes one argument and its tail-jump returns with `ret 4`,
    /// so the stub must pop exactly four bytes.
    #[test]
    fn the_stub_is_ret_4() {
        assert_eq!(HIDDEN, [0xc2, 0x04, 0x00]);
    }

    /// The panel offset and `SetVisible` slot are the ones the pattern pins
    /// in `ShowHint`'s own code (`mov ecx, [esi+0x14]`, `jmp [edx+0x74]`).
    #[test]
    fn close_uses_the_offsets_show_hint_uses() {
        assert!(PATTERN.contains(&format!("8B 4E {HINT_PANEL_OFFSET:02X} 5E")));
        assert!(PATTERN.ends_with(&format!("FF 62 {:02X}", SET_VISIBLE_SLOT * 4)));
        let call: Vec<&str> = VIEWPORT_CALL.split_whitespace().collect();
        assert_eq!(&call[..VIEWPORT_AT], &["8B", "0D"], "mov ecx, [abs32]");
        assert!(
            call[VIEWPORT_AT..VIEWPORT_AT + 4]
                .iter()
                .all(|t| *t == "??")
        );
        assert!(
            VIEWPORT_CALL.ends_with(&format!("FF 52 {:02X}", 5 * 4)),
            "slot 5, ShowHint"
        );
    }

    #[test]
    fn stock_bytes_survive_the_round_trip() {
        let bytes = [0xa1, 0xd0, 0x88];
        assert_eq!(unpack(pack(bytes)), bytes);
    }
}
