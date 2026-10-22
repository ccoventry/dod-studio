//! `ds_hud_overview_hide_speaker`: leave the speaker icon
//! (`sprites/mapsprites/speakerIcon.spr`) off the players' icons on the
//! overview map (#677).
//!
//! ## What the game does
//!
//! Inside `CHudDoDMap::DrawOverviewEntities`' per-player loop, right after a
//! player's own icon, the game decides whether to put the speaker on it:
//!
//! ```text
//!     +0x2f266  ... call CHudSpectator+0x3a230  ; is he talking? -> a timer
//!     +0x2f287  mov  eax, [esi+4]               ; <- E9 to +0x2f31e
//!     +0x2f28a  test eax, eax
//!     +0x2f28c  je   ...                        ; a player, index 1..64,
//!     ...                                       ; flagged; or the timer
//!     +0x2f2b6  jne  +0x2f31e                   ; still running
//!     +0x2f2b8  ...                             ; draw speakerIcon.spr
//!     +0x2f31c  jmp  +0x2f322
//!     +0x2f31e  mov  bl, [esp+0x13]             ; the next entry
//! ```
//!
//! What sets the flag is a guess (a player talking, or a voice command);
//! where it is drawn is not. The first five bytes of the decision, two whole
//! instructions, become a `jmp` to the game's own "no speaker" exit, which
//! the `jne` above names, so the loop goes straight on to the next entry. The
//! talking timer is still kept; nothing reads `eax` or the flags at
//! `+0x2f31e`, and the stock path into it arrives with `eax` holding an FPU
//! status word anyway. Nothing branches into the patched span.
//!
//! The player icons stay, unlike `ds_hud_overview_players 4`, which was the
//! only way to lose the speaker without blanking the sprite file.
//!
//! `tools/verify_hide_text_offsets.py` checks the span, the exit, that the
//! skipped block draws the handle `CHudSpectator::VidInit` loads from
//! `speakerIcon.spr`, and that nothing branches into the span.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("hud_overview_hide_speaker");

/// From the talking check's `mov ecx, [esi]` to the first instruction of the
/// speaker draw, so both the span and the `jne` that names its exit are in it.
const PATTERN: &str = "8B 0E 51 B9 ?? ?? ?? ?? E8 ?? ?? ?? ?? 84 C0 74 10 \
                       DD 44 24 20 DC 05 ?? ?? ?? ?? D9 9E 28 01 00 00 \
                       8B 46 04 85 C0 74 17 8B 06 83 F8 01 7C 10 83 F8 40 7F 0B \
                       8B 0C 85 ?? ?? ?? ?? 85 C9 75 13 \
                       D9 86 28 01 00 00 DC 5C 24 20 DF E0 25 00 41 00 00 75 66 \
                       8A 5C 24 13";
/// The patched span in [`PATTERN`]: `mov eax, [esi+4]; test eax, eax`.
const SPAN_AT: usize = 33;
/// What the span holds in the stock game.
const STOCK: [u8; 5] = [0x8b, 0x46, 0x04, 0x85, 0xc0];
/// The `jne rel8` whose target is the "no speaker" exit.
const JNE_AT: usize = 80;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The span's address, 0 before a successful scan.
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// A base the scan already failed on, so it isn't repeated every frame.
static FAILED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The `jmp` to the exit, packed little-endian.
static HIDDEN: AtomicU64 = AtomicU64::new(0);
/// Set the first time the cvar is on, so a session that never asks for it
/// never scans.
static TOUCHED: AtomicBool = AtomicBool::new(false);
static HIDDEN_NOW: AtomicBool = AtomicBool::new(false);
static COMPLAINED: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// `jmp rel32` from the span at `span` to `target`.
fn jmp_to(span: usize, target: usize) -> [u8; 5] {
    let rel = (target as i64 - (span as i64 + 5)) as i32;
    let mut out = [0xe9, 0, 0, 0, 0];
    out[1..].copy_from_slice(&rel.to_le_bytes());
    out
}

fn span_address(base: usize) -> Result<usize, String> {
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    if FAILED_BASE.load(Ordering::Acquire) == base {
        return Err("the speaker icon's draw was not found in this client.dll".to_string());
    }
    // Safety: a mapped module, for the session.
    let found = match unsafe { scan::find_unique(base, PATTERN) } {
        Ok(found) => found,
        Err(why) => {
            FAILED_BASE.store(base, Ordering::Release);
            return Err(format!("could not find the speaker icon's draw -- {why}"));
        }
    };
    let jne = found + JNE_AT;
    // Safety: inside the matched pattern.
    let rel = unsafe { *((jne + 1) as *const i8) };
    let exit = (jne + 2).wrapping_add_signed(rel as isize);
    let span = found + SPAN_AT;
    let mut packed = [0u8; 8];
    packed[..5].copy_from_slice(&jmp_to(span, exit));
    HIDDEN.store(u64::from_le_bytes(packed), Ordering::Release);
    ADDRESS.store(span, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(span)
}

/// Shows or hides the speaker icon, returning whether anything was written.
/// Decides from the bytes, so it is cheap to call every frame.
fn set_hidden(base: usize, hidden: bool) -> Result<bool, String> {
    let span = span_address(base)?;
    let jump = HIDDEN.load(Ordering::Acquire).to_le_bytes();
    let jump = &jump[..STOCK.len()];
    let want: &[u8] = if hidden { jump } else { &STOCK };
    // Safety: the scan proved these bytes are mapped code.
    let present = unsafe { std::slice::from_raw_parts(span as *const u8, STOCK.len()) };
    if present == want {
        return Ok(false);
    }
    if present != STOCK && present != jump {
        return Err(format!(
            "the speaker icon's draw starts {present:02x?}, neither the stock {STOCK:02x?} nor {jump:02x?} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(span, want) } {
        return Err("could not make the speaker icon's draw writable".to_string());
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
    match set_hidden(base, wanted) {
        Ok(wrote) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            HIDDEN_NOW.store(wanted, Ordering::Relaxed);
            if wrote {
                unsafe {
                    crate::debug::report(&format!(
                        "overview_speaker: {NAME} = {}",
                        if wanted {
                            "1 (no speaker icon on the overview map)"
                        } else {
                            "0 (normal)"
                        }
                    ))
                };
            }
        }
        Err(why) => {
            HIDDEN_NOW.store(false, Ordering::Relaxed);
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "overview_speaker: {NAME} not applied yet -- {why}"
                    ))
                };
            }
        }
    }
}

/// One line for `ds_debug_status`, once the cvar has been on.
pub fn status_line() -> Option<String> {
    TOUCHED.load(Ordering::Relaxed).then(|| {
        if HIDDEN_NOW.load(Ordering::Relaxed) {
            format!("{NAME} = 1 -- the overview map draws no speaker icons")
        } else {
            format!("{NAME}: the overview map draws speaker icons")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Vec<&'static str> {
        PATTERN.split_whitespace().collect()
    }

    #[test]
    fn the_offsets_index_the_instructions_they_name() {
        let t = tokens();
        for (i, byte) in STOCK.iter().enumerate() {
            assert_eq!(
                u8::from_str_radix(t[SPAN_AT + i], 16).ok(),
                Some(*byte),
                "STOCK is the pattern's own bytes at the span"
            );
        }
        assert_eq!(&t[SPAN_AT + 5..SPAN_AT + 7], &["74", "17"], "je after it");
        assert_eq!(t[JNE_AT], "75", "jne rel8");
        assert_eq!(
            &t[JNE_AT + 2..JNE_AT + 4],
            &["8A", "5C"],
            "the draw starts after the jne"
        );
        assert!(t.len() > JNE_AT + 2);
    }

    #[test]
    fn the_jump_lands_on_its_target() {
        // As in the shipped client.dll: +0x2f287 to +0x2f31e.
        assert_eq!(
            jmp_to(0x0192_f287, 0x0192_f31e),
            [0xe9, 0x92, 0x00, 0x00, 0x00]
        );
        // And backwards.
        assert_eq!(jmp_to(0x1000, 0x0f00), [0xe9, 0xfb, 0xfe, 0xff, 0xff]);
        assert_eq!(STOCK.len(), jmp_to(0, 0).len(), "as wide as the span");
    }
}
