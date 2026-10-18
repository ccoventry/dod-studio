//! While Review highlights runs, the viewer's own spectator buttons do
//! nothing (#826): a stray click or jump can't move an HLTV highlight's
//! camera to someone else or change the spectator mode.
//!
//! ## Where the buttons go
//!
//! Every spectator button reaches the camera through one function,
//! `CHudSpectator::HandleButtonsDown(int buttons)` (`client+0x386a0`; the
//! SDK's `cl_dll/hud_spectator.cpp`). The nine `+` commands that call it are
//! `+attack` (1), `+jump` (2), `+duck` (4), `+forward` (8), `+back` (0x10),
//! `+use` (0x20), `+moveleft` (0x200), `+moveright` (0x400) and `+attack2`
//! (0x800), each one `push <bit>; mov ecx, <the spectator HUD>; call`. In it:
//!
//! - `+attack` / `+attack2` step to the next or previous player;
//! - `+jump` cycles the mode (1 -> 2 -> 4 -> 3, in-eye to free roam);
//! - `+duck` opens the spectator menu and `+use` toggles the inset window.
//!
//! It is also what the game's switch after a death presses (`+attack`, see
//! `spectator_follow`). Movement in free roam and the camera's angles don't go
//! through it, and neither do the console's `spec_mode` (which the review
//! itself sends) or ESC.
//!
//! ## The patch
//!
//! Its first instruction is `sub esp, 0xc` (`83 EC 0C`), three bytes, and the
//! function is `thiscall` with one argument, ending `ret 4`. Writing
//! `C2 04 00` (`ret 4`) over that instruction returns before anything runs;
//! writing the three bytes back restores it. The `+` command still marks its
//! key down, so nothing else about input changes.
//!
//! It is on only while a review holds `spectator_follow`'s lock, and off the
//! frame after the review stops or ends. `client.dll` is the same file in
//! both engine builds; `tools/verify_spectator_buttons_offsets.py` checks the
//! pattern against it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// `HandleButtonsDown`'s start: the prologue, the client-time call, and the
/// first gate (`gViewPort`). Wildcards cover the absolute addresses.
const PATTERN: &str = "83 EC 0C 56 57 8B F1 FF 15 ?? ?? ?? ?? 8B 86 50 17 00 00 \
                       83 CF FF DD 5C 24 0C D9 40 0C E8 ?? ?? ?? ?? 89 44 24 08 \
                       A1 ?? ?? ?? ?? 85 C0 0F 84";

/// `sub esp, 0xc`: the stock first instruction.
const STOCK: &[u8] = &[0x83, 0xec, 0x0c];
/// `ret 4`: returns at once, popping the one argument.
const IGNORED: &[u8] = &[0xc2, 0x04, 0x00];

static ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// The module base it was resolved against (see `crosshair::SCANNED_BASE`).
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// Whether a failure to apply has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

fn address() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session.
    let address = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the spectator buttons -- {why}"))?;
    ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Ignores or restores the spectator buttons, returning whether a byte was
/// written. Decides from the bytes, so it is cheap to call every frame.
fn set_ignored(ignored: bool) -> Result<bool, String> {
    let address = address()?;
    let want = if ignored { IGNORED } else { STOCK };
    // Safety: the scan proved the pattern's bytes are mapped code.
    let present = unsafe { std::slice::from_raw_parts(address as *const u8, STOCK.len()) };
    if present == want {
        return Ok(false);
    }
    if present != STOCK && present != IGNORED {
        return Err(format!(
            "the spectator buttons start {present:02x?}, neither {STOCK:02x?} nor {IGNORED:02x?} -- something else has patched them"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, want) } {
        return Err("could not make the spectator buttons writable".to_string());
    }
    Ok(true)
}

/// Follows the review's hold. Called every frame from `commands::poll`.
pub fn poll() {
    let wanted = crate::spectator_follow::review_holds();
    // Nothing to undo before the first review: skip the scan until one runs.
    if !wanted && ADDRESS.load(Ordering::Relaxed) == 0 {
        return;
    }
    match set_ignored(wanted) {
        Ok(false) => COMPLAINED.store(false, Ordering::Relaxed),
        Ok(true) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            unsafe { crate::debug::report(&format!("spectator_buttons: {}", describe(wanted))) };
        }
        Err(why) => {
            if !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("spectator_buttons: not applied -- {why}"))
                };
            }
        }
    }
}

fn describe(ignored: bool) -> &'static str {
    if ignored {
        "a review is running: clicks, jump and duck no longer move the spectator camera"
    } else {
        "the spectator buttons work again"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_states_are_one_whole_instruction_wide() {
        assert_eq!(STOCK.len(), IGNORED.len());
        assert_ne!(STOCK, IGNORED);
        // `ret imm16` popping the one 4-byte argument of a thiscall.
        assert_eq!(IGNORED[0], 0xc2);
        assert_eq!(u16::from_le_bytes([IGNORED[1], IGNORED[2]]), 4);
    }

    /// A restore writes back exactly what the scan proved was there.
    #[test]
    fn the_stock_bytes_are_the_head_of_the_pattern() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert!(tokens.len() > STOCK.len());
        for (i, byte) in STOCK.iter().enumerate() {
            assert_eq!(
                u8::from_str_radix(tokens[i], 16).ok(),
                Some(*byte),
                "byte {i}"
            );
        }
        for token in &tokens {
            assert!(
                *token == "??" || u8::from_str_radix(token, 16).is_ok(),
                "{token:?}"
            );
        }
    }
}
