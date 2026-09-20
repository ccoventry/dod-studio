//! `dodtools_crosshair`: hide DoD's crosshair, and make it stay hidden.
//!
//! ## Why the stock cvar is not enough
//!
//! DoD registers a `crosshair` cvar, and setting it to 0 appears to work for
//! exactly one frame. `CHud::Redraw` is one of the three cvars it **silently
//! forces back** every frame (`docs/goldsrc_client_dll_survey.md` section 8,
//! step 3 -- the other two in that block, `r_drawentities` and `cl_lw`, do not
//! merely reset, they quit the game). So there is no way to turn the crosshair
//! off from the console and have it stay off.
//!
//! That is the gap: not "there is no setting", but "the setting is overwritten
//! before it can take effect".
//!
//! ## What this patches
//!
//! `CHudDoDCrossHair` is an ordinary HUD element, so `CHud::Redraw` reaches it
//! through the element list and calls `Draw` from vftable slot 3 like every
//! other one. Its `Draw` begins:
//!
//! ```text
//!     client+0x2cd20  mov  eax, [esp+4]      ; 8B 44 24 04
//!     client+0x2cd24  push esi               ; 56
//!     client+0x2cd25  push eax
//!     client+0x2cd26  mov  esi, ecx
//!     client+0x2cd28  call client+0x2d2b0
//! ```
//!
//! Writing `33 C0 C2 04 00` over those first five bytes makes it
//! `xor eax, eax; ret 4` -- which is **byte-for-byte `CHudBase::Draw`**, the
//! do-nothing base implementation at `client+0x21940` that every element
//! inherits and most override. So the element is still in the list, still gets
//! called, and does exactly what an element that draws nothing does.
//!
//! ## Why the function and not the vftable
//!
//! Issue #265's idea is to write `CHudBase::Draw`'s address into an element's
//! vftable slot 3, which is one dword. That works and is cheaper. It also
//! needs the vftable's address at runtime, which means either resolving RTTI
//! in the loaded module or signature-matching the constructor that stores it.
//!
//! Patching the function needs neither: one signature on the function itself,
//! five bytes, and reverting restores the five bytes the signature already
//! proves were there. The general `dodtools_hudelement` command in #265 is
//! still worth having -- this is not it, and does not block it.
//!
//! ## It covers both the POV and the spectator crosshair
//!
//! One function draws both, and the stub is at its first instruction, so both
//! die. `Draw` branches on the observer-mode global at `client+0x1e88d4`:
//!
//! ```text
//!     mode == 0            the POV path -- and the only one that reads the
//!                          `crosshair` cvar
//!     mode == 3 or 4       client+0x2d1f0, the spectator crosshair
//!                          (3 and 4 are roaming and in-eye in HL's numbering)
//!     anything else        nothing is drawn
//! ```
//!
//! Worth recording for #219, which asks why POV and HLTV first-person differ:
//! **the spectator branch never reads the `crosshair` cvar at all.** So even
//! without `CHud::Redraw` forcing the value back, `crosshair 0` could not have
//! hidden the spectator crosshair -- there is no code path by which it would.
//!
//! ## What is not covered
//!
//! DoD also calls the engine's own `pfnSetCrosshair` (`gEngfuncs[13]`) from its
//! weapon-sprite code -- once with a null sprite (clearing it) and once with a
//! real one. That is a second, engine-drawn crosshair which this does not
//! touch, and whether it is ever visible has **not** been established. A live
//! test settles it: if anything remains on screen with this set to 0, that is
//! what it is.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The cvar name, for status and error text. Registered in `commands.rs`.
pub const NAME: &str = console_name!("hide_crosshair");

/// `CHudDoDCrossHair::Draw`'s prologue. The wildcards cover the relative call
/// and the one absolute address that follows it, both of which move with the
/// build.
const PATTERN: &str = "8B 44 24 04 56 50 8B F1 E8 ?? ?? ?? ?? A1 ?? ?? ?? ?? 85 C0";

/// The five bytes the stock function starts with.
const STOCK: &[u8] = &[0x8b, 0x44, 0x24, 0x04, 0x56];

/// `xor eax, eax; ret 4` -- `CHudBase::Draw`, exactly.
const HIDDEN: &[u8] = &[0x33, 0xc0, 0xc2, 0x04, 0x00];

/// The resolved address of `CHudDoDCrossHair::Draw`, or 0 before the first
/// successful scan.
static DRAW_ADDRESS: AtomicUsize = AtomicUsize::new(0);

/// The module base the address was resolved against. Measured (five game
/// sessions, five `LoadLibraryA("client.dll")` log lines, no reload between
/// demos inside a session, `docs/goldsrc_dod_quirks.md`): a plain demo change
/// does not reload `client.dll`. This guards a rescan for whatever *would*
/// reload it -- a mod change, returning to the menu -- neither of which has
/// been tested.
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Whether the crosshair is currently suppressed in the loaded module.
///
/// `pub(crate)` rather than private: `spectator_crosshair`'s tests set this
/// directly to exercise the "hiding wins" note in its own `status()`, the same
/// way this module's own tests set it.
pub(crate) static HIDDEN_NOW: AtomicBool = AtomicBool::new(false);

fn draw_address() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };

    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = DRAW_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }

    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session.
    let address = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find CHudDoDCrossHair::Draw -- {why}"))?;

    let present = unsafe { std::slice::from_raw_parts(address as *const u8, STOCK.len()) };
    if present != STOCK {
        return Err(format!(
            "+{:#x} starts {present:02x?}, not the expected {STOCK:02x?}",
            address - base
        ));
    }

    DRAW_ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Shows or hides the crosshair, returning whether anything was written.
///
/// `hidden` is the cvar's own sense: `true` stubs the draw, `false` is the
/// game's stock behaviour.
///
/// Idempotent and cheap to call every frame, which is how it is used. That is
/// not tidiness -- deciding from the bytes rather than a flag is what would
/// let the setting survive `client.dll` being unloaded and reloaded, on
/// whatever transition actually does that (see [`SCANNED_BASE`]) -- a
/// reloaded module comes back with the stock prologue.
pub fn set_hidden(hidden: bool) -> Result<bool, String> {
    let address = draw_address()?;
    let want: &[u8] = if hidden { HIDDEN } else { STOCK };

    let present = unsafe { std::slice::from_raw_parts(address as *const u8, STOCK.len()) };
    if present == want {
        HIDDEN_NOW.store(hidden, Ordering::Release);
        return Ok(false);
    }
    if present != STOCK && present != HIDDEN {
        return Err(format!(
            "CHudDoDCrossHair::Draw starts {present:02x?}, which is neither the stock prologue nor this module's stub -- something else has patched it"
        ));
    }

    if !unsafe { crate::patch::write_code_bytes(address, want) } {
        return Err("could not make CHudDoDCrossHair::Draw writable".to_string());
    }
    HIDDEN_NOW.store(hidden, Ordering::Release);
    Ok(true)
}

/// Whether the crosshair is currently suppressed.
pub fn hidden() -> bool {
    HIDDEN_NOW.load(Ordering::Relaxed)
}

/// One line for `dodtools_debug_status`.
pub fn status() -> String {
    if !hidden() {
        return "the crosshair draws normally (the stock `crosshair` cvar cannot turn it off -- CHud::Redraw forces the value back every frame)".into();
    }
    "CHudDoDCrossHair::Draw is stubbed, so the crosshair does not draw".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two states must be the same width: this overwrites a prologue in
    /// place, so a shorter or longer replacement would leave a partial
    /// instruction behind and desynchronise everything after it.
    #[test]
    fn both_states_are_the_same_width() {
        assert_eq!(STOCK.len(), HIDDEN.len());
        assert_ne!(STOCK, HIDDEN);
    }

    /// `HIDDEN` has to be `xor eax, eax; ret 4`. Spelled out here because a
    /// wrong `ret` operand would unbalance the stack on a `__thiscall` that
    /// takes one float, and the symptom would be a crash some frames later
    /// rather than at the patch.
    #[test]
    fn the_stub_is_exactly_chudbase_draw() {
        assert_eq!(HIDDEN, &[0x33, 0xc0, 0xc2, 0x04, 0x00]);
        assert_eq!(&HIDDEN[..2], &[0x33, 0xc0], "xor eax, eax");
        assert_eq!(HIDDEN[2], 0xc2, "ret imm16");
        assert_eq!(
            u16::from_le_bytes([HIDDEN[3], HIDDEN[4]]),
            4,
            "one 4-byte argument (float flTime) is popped by the callee"
        );
    }

    /// The stock bytes must be the literal start of the pattern, so that a
    /// revert writes back exactly what the signature already proved was there.
    #[test]
    fn the_stock_bytes_are_the_head_of_the_pattern() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert!(tokens.len() >= STOCK.len());
        for (i, byte) in STOCK.iter().enumerate() {
            assert_eq!(
                u8::from_str_radix(tokens[i], 16).ok(),
                Some(*byte),
                "pattern byte {i} is {:?}, STOCK says {byte:#04x}",
                tokens[i]
            );
        }
    }

    /// The pattern has to be longer than what is overwritten, or it would not
    /// distinguish this function from any other with the same prologue --
    /// there are 18 of those in the image.
    #[test]
    fn the_pattern_reaches_past_the_bytes_it_overwrites() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert!(
            tokens.len() > STOCK.len(),
            "the signature must be more specific than the span it patches"
        );
        for token in &tokens {
            assert!(
                *token == "??" || u8::from_str_radix(token, 16).is_ok(),
                "{token:?} is not a hex byte or a wildcard"
            );
        }
    }

    #[test]
    fn status_explains_why_the_stock_cvar_does_not_work() {
        HIDDEN_NOW.store(false, Ordering::Release);
        assert!(status().contains("forces the value back"));
        HIDDEN_NOW.store(true, Ordering::Release);
        assert!(status().contains("does not draw"));
        HIDDEN_NOW.store(false, Ordering::Release);
    }
}
