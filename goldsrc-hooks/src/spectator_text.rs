//! `ds_spec_hide_text`: hide the two lines DoD prints when the
//! view becomes a spectator camera (#677) -- the camera mode's name, centred
//! just above the bottom bar ("Locked Chase Cam", "Free Look", ...), and the
//! green "Press DUCK for Spectator Menu" written out above it.
//!
//! Neither had a narrow switch of its own before this: the retired (#342)
//! `ds_hud_hide_element vgui2print 1` took the mode name with every other
//! centre-print, and `message 1` the green line with map text, clan prompts
//! and HLTV director text.
//!
//! ## The camera mode name
//!
//! `CHudSpectator::SetMode` (`+0x38850`), after a real mode change, builds
//! `"%c%s"` with `HUD_PRINTCENTER` and `#Spec_Mode1`..`4`, then calls
//! `CHudTextMessage::MsgFunc_TextMsg` (`+0x4c250`) **directly**, not through
//! the engine's user-message dispatcher -- which is why a `TextMsg` hook
//! (`clan_text`, `center_text`) never sees it:
//!
//! ```text
//!     +0x38a11  push ecx                 ; pbuf: the "%c%s" buffer
//!     +0x38a19  push ecx                 ; iSize: its length
//!     +0x38a1a  push eax                 ; pszName: 0
//!     +0x38a1b  mov  ecx, gHUD.m_TextMessage
//!     +0x38a20  call MsgFunc_TextMsg     ; E8 rel32, ret 0xc   <- patched
//!     +0x38a25  mov  ecx, [...]          ; eax and the flags are not read
//! ```
//!
//! The call becomes `add esp, 0xc; nop; nop`: the three arguments come off
//! the stack exactly as the callee's `ret 0xc` would have taken them, and
//! nothing after it reads `eax`. Only this one call site changes, so every
//! other centre-print (server text, the clan countdown) is untouched.
//!
//! ## The server's copy of the mode name
//!
//! A demo also carries the mode name as an ordinary `TextMsg` from the
//! server: destination 4 with the key `#Spec_Mode1`..`4`, at every POV death
//! and HLTV mode change. That one goes through the engine's message
//! dispatcher, not `SetMode`, so the patch above never sees it, and in game it
//! showed with the patch in place. `clan_text`'s `TextMsg` filter asks
//! [`hide_text_msg`] for it, the same way it asks `center_text`.
//!
//! ## "Press DUCK for Spectator Menu"
//!
//! A helper at `+0x84780` fills a fixed `client_textmessage_t` (green, written
//! out, `Spec_Duck`) and hands it to `CHudMessage::MessageAdd` through the
//! viewport's function table. Its four callers all pass `#Spec_Duck`, and all
//! pop their argument themselves (cdecl), so a `ret` (`C3`) over its first
//! byte hides exactly this line and nothing else.
//!
//! ## Both are applied every frame
//!
//! Like `crosshair.rs`, the decision comes from the bytes: written while the
//! cvar is 1, restored at 0, and a re-loaded `client.dll` is patched again.
//! A line already on screen when the cvar turns on stays until it times out
//! (the green line holds 6 s).
//!
//! `tools/verify_hide_text_offsets.py` checks both sites against a real
//! `client.dll` (one file, identical in every install): the call's target and
//! its `ret 0xc`, the three pushes, every `#Spec_Duck` caller popping its own
//! argument, and nothing branching into either span.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::engine::{self};
use crate::names::console_name;
use crate::scan;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("spec_hide_text");

/// One patched span in `client.dll`.
struct Site {
    /// For the log.
    what: &'static str,
    pattern: &'static str,
    /// Where the patched span starts in `pattern`.
    at: usize,
    /// What the span holds while hidden. Its stock bytes are read at the scan,
    /// so a revert writes back exactly what was there.
    hidden: &'static [u8],
}

/// `CHudSpectator::SetMode`, from `push 4` (`HUD_PRINTCENTER`) to the
/// instruction after the call to `MsgFunc_TextMsg`.
const MODE_NAME: Site = Site {
    what: "the camera mode name",
    pattern: "6A 04 8D 44 24 40 68 ?? ?? ?? ?? 50 E8 ?? ?? ?? ?? 83 C4 20 \
              8D 4C 24 28 8D 7C 24 28 33 C0 51 83 C9 FF F2 AE F7 D1 51 50 \
              B9 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B 0D",
    at: 45,
    // add esp, 0xc; nop; nop
    hidden: &[0x83, 0xc4, 0x0c, 0x90, 0x90],
};

/// The `#Spec_Duck` helper's prologue: the `strncpy` into its own buffer and
/// the first two colour bytes (40 and 200).
const SPEC_DUCK: Site = Site {
    what: "\"Press DUCK for Spectator Menu\"",
    pattern: "8B 44 24 04 68 80 00 00 00 50 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? B0 28 B1 C8",
    at: 0,
    // ret
    hidden: &[0xc3],
};

const SITES: [Site; 2] = [MODE_NAME, SPEC_DUCK];

/// What one site's scan found.
struct State {
    /// The span's address, 0 before a successful scan.
    address: AtomicUsize,
    /// The module base `address` belongs to.
    base: AtomicUsize,
    /// A base the scan already failed on, so it isn't repeated every frame.
    failed_base: AtomicUsize,
    /// The stock bytes, little-endian, `hidden.len()` of them.
    stock: AtomicU64,
    /// Whether a failure has been reported, so it is said once.
    complained: AtomicBool,
}

impl State {
    const fn new() -> Self {
        Self {
            address: AtomicUsize::new(0),
            base: AtomicUsize::new(0),
            failed_base: AtomicUsize::new(0),
            stock: AtomicU64::new(0),
            complained: AtomicBool::new(false),
        }
    }
}

static STATES: [State; 2] = [State::new(), State::new()];

pub static CVAR: Cvar = Cvar::new();
/// Set the first time the cvar is on, so a session that never asks for it
/// never scans.
static TOUCHED: AtomicBool = AtomicBool::new(false);
/// Whether both lines are hidden right now.
static HIDDEN_NOW: AtomicBool = AtomicBool::new(false);
/// The cvar's value as last seen: what [`hide_text_msg`] reads.
static WANTED: AtomicBool = AtomicBool::new(false);

/// `TextMsg`'s destination byte for the middle of the screen.
const HUD_PRINTCENTER: u8 = 4;
/// The start of the mode names' `titles.txt` keys.
const MODE_KEY: &[u8] = b"#Spec_Mode";

/// Whether a `TextMsg` payload (destination byte, then the key) is the
/// server's copy of the camera mode name.
fn is_mode_name(payload: &[u8]) -> bool {
    payload.first() == Some(&HUD_PRINTCENTER) && payload[1..].starts_with(MODE_KEY)
}

/// Whether `clan_text`'s `TextMsg` filter should drop this payload: the
/// cvar is on and it is the server's copy of the mode name (see the module
/// doc). An atomic load and a byte compare.
pub(crate) fn hide_text_msg(payload: &[u8]) -> bool {
    WANTED.load(Ordering::Relaxed) && is_mode_name(payload)
}

fn pack(bytes: &[u8]) -> u64 {
    let mut out = [0u8; 8];
    out[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(out)
}

fn unpack(packed: u64, len: usize) -> Vec<u8> {
    packed.to_le_bytes()[..len].to_vec()
}

/// The site's address in the module at `base`, scanning once per module.
fn resolve(site: &Site, state: &State, base: usize) -> Result<usize, String> {
    if state.base.load(Ordering::Acquire) == base {
        let cached = state.address.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    if state.failed_base.load(Ordering::Acquire) == base {
        return Err(format!("{} was not found in this client.dll", site.what));
    }
    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session.
    let found = match unsafe { scan::find_unique(base, site.pattern) } {
        Ok(found) => found,
        Err(why) => {
            state.failed_base.store(base, Ordering::Release);
            return Err(format!("could not find {} -- {why}", site.what));
        }
    };
    let address = found + site.at;
    // Safety: the pattern matched, so the span is mapped code.
    let present =
        unsafe { std::slice::from_raw_parts(address as *const u8, site.hidden.len()) }.to_vec();
    if present == site.hidden {
        state.failed_base.store(base, Ordering::Release);
        return Err(format!(
            "{} is already patched in a client.dll this module never scanned",
            site.what
        ));
    }
    state.stock.store(pack(&present), Ordering::Release);
    state.address.store(address, Ordering::Release);
    state.base.store(base, Ordering::Release);
    Ok(address)
}

/// Puts one site in the wanted state, returning whether anything was written.
fn apply(site: &Site, state: &State, base: usize, hidden: bool) -> Result<bool, String> {
    let address = resolve(site, state, base)?;
    let stock = unpack(state.stock.load(Ordering::Acquire), site.hidden.len());
    let want: &[u8] = if hidden { site.hidden } else { &stock };
    // Safety: `resolve` proved the span is mapped code.
    let present = unsafe { std::slice::from_raw_parts(address as *const u8, want.len()) };
    if present == want {
        return Ok(false);
    }
    if present != stock.as_slice() && present != site.hidden {
        return Err(format!(
            "{} holds {present:02x?}, neither the stock {stock:02x?} nor {:02x?} -- something else has patched it",
            site.what, site.hidden
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, want) } {
        return Err(format!("could not make {} writable", site.what));
    }
    Ok(true)
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let Some(wanted) = CVAR.try_on() else {
        return;
    };
    WANTED.store(wanted, Ordering::Relaxed);
    if wanted {
        TOUCHED.store(true, Ordering::Relaxed);
    } else if !TOUCHED.load(Ordering::Relaxed) {
        return;
    }
    let Some(base) = engine::client_module_base() else {
        return;
    };
    let mut all = true;
    for (site, state) in SITES.iter().zip(&STATES) {
        match apply(site, state, base, wanted) {
            Ok(wrote) => {
                state.complained.store(false, Ordering::Relaxed);
                if wrote {
                    unsafe {
                        crate::debug::report(&format!(
                            "spectator_text: {NAME} = {} -- {} {}",
                            u8::from(wanted),
                            site.what,
                            if wanted { "hidden" } else { "shows again" }
                        ))
                    };
                }
            }
            Err(why) => {
                all = false;
                if wanted && !state.complained.swap(true, Ordering::Relaxed) {
                    unsafe {
                        crate::debug::report(&format!(
                            "spectator_text: {NAME} not applied yet -- {why}"
                        ))
                    };
                }
            }
        }
    }
    HIDDEN_NOW.store(wanted && all, Ordering::Relaxed);
}

/// One line for `ds_debug_status`, once the cvar has been on.
pub fn status_line() -> Option<String> {
    if !TOUCHED.load(Ordering::Relaxed) {
        return None;
    }
    Some(if HIDDEN_NOW.load(Ordering::Relaxed) {
        format!(
            "{NAME} = 1 -- the camera mode name and \"Press DUCK for Spectator Menu\" are hidden"
        )
    } else {
        format!("{NAME}: the camera mode name and \"Press DUCK for Spectator Menu\" show")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(site: &Site) -> Vec<&'static str> {
        site.pattern.split_whitespace().collect()
    }

    #[test]
    fn every_pattern_is_hex_or_wildcard_and_reaches_past_its_span() {
        for site in &SITES {
            let t = tokens(site);
            assert!(t.len() > site.at + site.hidden.len(), "{}", site.what);
            assert_ne!(t[0], "??", "a pattern can't start with a wildcard");
            for token in &t {
                assert!(
                    *token == "??" || u8::from_str_radix(token, 16).is_ok(),
                    "{token:?} in {}",
                    site.what
                );
            }
            assert!(site.hidden.len() <= 8, "the stock bytes are kept in a u64");
        }
    }

    /// The mode name's span is the `call rel32`, and what replaces it pops the
    /// callee's three arguments: `ret 0xc` on a `__thiscall` of three.
    #[test]
    fn the_mode_name_call_becomes_add_esp_12() {
        let t = tokens(&MODE_NAME);
        assert_eq!(t[MODE_NAME.at], "E8", "call rel32");
        assert_eq!(t[MODE_NAME.at - 5], "B9", "mov ecx, imm32");
        assert_eq!(MODE_NAME.hidden.len(), 5, "as wide as the call");
        assert_eq!(&MODE_NAME.hidden[..3], &[0x83, 0xc4, 0x0c], "add esp, 0xc");
        assert!(MODE_NAME.hidden[3..].iter().all(|&b| b == 0x90));
        // HUD_PRINTCENTER is what this call site prints with.
        assert_eq!(&t[..2], &["6A", "04"]);
    }

    /// `ret` over the helper's first byte, which the pattern pins as the
    /// start of `mov eax, [esp+4]` -- its first instruction.
    #[test]
    fn the_spec_duck_helper_returns_at_once() {
        let t = tokens(&SPEC_DUCK);
        assert_eq!(SPEC_DUCK.at, 0);
        assert_eq!(&t[..4], &["8B", "44", "24", "04"]);
        assert_eq!(SPEC_DUCK.hidden, &[0xc3]);
    }

    #[test]
    fn only_a_centre_spec_mode_key_is_the_servers_mode_name() {
        assert!(is_mode_name(b"\x04#Spec_Mode4\0"));
        assert!(is_mode_name(b"\x04#Spec_Mode1\0"));
        assert!(!is_mode_name(b"\x03#Spec_Mode4\0"), "chat, not centre");
        assert!(!is_mode_name(b"\x04#Spec_Duck\0"));
        assert!(!is_mode_name(b"\x04#clan_ready_rules\0"));
        assert!(!is_mode_name(b"\x04"));
        assert!(!is_mode_name(b""));
    }

    #[test]
    fn stock_bytes_survive_the_round_trip() {
        let bytes = [0xe8, 0x2b, 0x38, 0x01, 0x00];
        assert_eq!(unpack(pack(&bytes), bytes.len()), bytes);
        assert_eq!(unpack(pack(&[0x8b]), 1), [0x8b]);
    }
}
