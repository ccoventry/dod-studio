//! `dodtools_scoreboard`: stop a POV demo's recorded TAB presses from putting
//! the scoreboard on screen.
//!
//! ## The problem this exists for
//!
//! A POV demo replays whatever the recording player typed, and a key bound to
//! `+showscores` is no exception: pressing TAB during a match writes a Type-3
//! `ConsoleCommand` frame into the demo, and playback executes it. So every
//! time the player checked the score, the scoreboard covers the shot. HLTV
//! demos do not have this problem -- nobody was pressing anything -- which is
//! why it looks like a POV-only quirk.
//!
//! The workaround before this was to edit `dod/resource/ui/ScoreBoard.res` and
//! move the dialog off-screen. That works, but it is a persistent edit to a
//! game file for a per-take decision, and it has to be undone to read a score
//! again.
//!
//! ## Why there is no cvar for it already
//!
//! There isn't one, and the reason is structural. DoD's scoreboard is **not a
//! HUD element** -- it is VGUI2, `CDoDClientScoreBoardDialog` :
//! `CClientScoreBoardDialog` : `IScoreBoardInterface`, laid out from
//! `Resource/UI/ScoreBoard.res`. It appears nowhere in the HUD element
//! inventory (`docs/goldsrc_client_dll_survey.md` section 1), so `hud_draw 0`
//! walks straight past it. The only score-related cvar DoD's client registers
//! at all is `spec_scoreboard`, which is spectator-mode-only and toggles on
//! change rather than gating anything.
//!
//! ## What this patches, and why it is one byte
//!
//! `+showscores` is registered to a function whose entire body is eight
//! instructions:
//!
//! ```text
//!     push 0x1a8aa24        ; the in_score kbutton
//!     call client+0x3bae0   ; KeyDown bookkeeping
//!     mov  ecx, [gViewPort]
//!     add  esp, 4
//!     test ecx, ecx
//!     je   .done            ; <- 74 05
//!     mov  eax, [ecx]
//!     jmp  [eax+0x18]       ; ShowScoreBoard
//! .done:
//!     ret
//! ```
//!
//! Turning that `je` (`74`) into a `jmp` (`EB`) makes the branch
//! unconditional, so the function always falls through to its own `ret`. One
//! byte, and the displacement is already correct because the landing site does
//! not move.
//!
//! Patching the `je` rather than writing `C3` over the entry point is
//! deliberate: the `call` above it is the engine's `+`/`-` key bookkeeping for
//! the `in_score` kbutton, and skipping it would leave that state claiming the
//! key is up while it is held. Only the virtual call is removed.
//!
//! `-showscores` is left alone. Hiding an already-hidden dialog is a no-op, and
//! leaving it working is what makes the suppression self-correcting: a
//! scoreboard already on screen when this is switched on disappears at the
//! player's next key release rather than sticking.
//!
//! ## Scope, stated plainly
//!
//! This suppresses the **command**. DoD also shows the scoreboard at round end
//! and through `spec_scoreboard`, and those reach the dialog by other call
//! sites that have not been traced. If a take needs "no scoreboard under any
//! circumstances", this is not yet that.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The cvar name, for status and error text. Registered in `commands.rs`.
pub const NAME: &str = console_name!("hide_scoreboard");

/// The whole `+showscores` handler, wildcarded over the two absolute addresses
/// and the one relative call.
///
/// The trailing `FF 60 18` is what makes this the *show* handler rather than
/// the hide one: `-showscores` is byte-identical up to that point and differs
/// only in the vftable displacement (`FF 60 2C`). Both occur exactly once in
/// the analysed image, and [`scan::find_unique`] refuses anything that does
/// not.
const PATTERN: &str = "68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? \
                       83 C4 04 85 C9 74 05 8B 01 FF 60 18";

/// Byte offset of the `je` opcode within [`PATTERN`].
const JE_AT: usize = 21;

/// `je rel8` -- what the stock build ships.
const JE: u8 = 0x74;
/// `jmp rel8` -- the same displacement, taken unconditionally.
const JMP: u8 = 0xEB;

/// The resolved address of the `je` opcode, or 0 before the first successful
/// scan. Cached because `find_unique` walks the whole `.text` section and the
/// cvar is polled every frame.
static JE_ADDRESS: AtomicUsize = AtomicUsize::new(0);

/// The module base [`JE_ADDRESS`] was resolved against, so a `client.dll`
/// reloaded at a different address is rescanned rather than patched through a
/// stale pointer. Same reason `deathmsg` tracks its own verified base.
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Whether the suppression is currently applied to the loaded module.
static SUPPRESSED: AtomicBool = AtomicBool::new(false);

/// Resolves the `je`'s address for the currently loaded `client.dll`.
fn je_address() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };

    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = JE_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }

    // Safety: `client_module_base` only returns a base for a module the loader
    // has mapped, and it stays mapped for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the +showscores handler -- {why}"))?;
    let address = found + JE_AT;

    // The scan already proved the byte is `74`; reading it back is what proves
    // `JE_AT` still indexes the byte the pattern matched, which is the part a
    // later edit to `PATTERN` could silently break.
    let present = unsafe { *(address as *const u8) };
    if present != JE {
        return Err(format!(
            "+{:#x} holds {present:#04x}, not the expected {JE:#04x} -- JE_AT does not line up with the pattern",
            address - base
        ));
    }

    JE_ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Applies or removes the suppression, returning whether a byte was actually
/// written. `hidden` is the cvar's own sense: `true` blocks `+showscores`,
/// `false` is the game's stock behaviour.
///
/// Idempotent and cheap to call every frame, which is how it is used. That
/// matters for more than tidiness: if `client.dll` is ever unloaded and
/// reloaded (measured *not* to happen for a plain demo change --
/// `docs/goldsrc_dod_quirks.md` -- but untested for a mod change or
/// returning to the menu), a reloaded module comes back with the stock byte.
/// A cached "already suppressed" belief would leave the scoreboard working
/// again from that point onward, silently. Deciding from the byte itself
/// rather than from a flag is what would make that self-heal.
pub fn set_hidden(hidden: bool) -> Result<bool, String> {
    let address = je_address()?;
    let want = if hidden { JMP } else { JE };

    let present = unsafe { *(address as *const u8) };
    if present == want {
        SUPPRESSED.store(hidden, Ordering::Release);
        return Ok(false);
    }
    // Anything other than the two bytes this module writes means something else
    // is patching the same site -- refuse rather than stamp over it.
    if present != JE && present != JMP {
        return Err(format!(
            "the +showscores handler holds {present:#04x}, which is neither {JE:#04x} nor {JMP:#04x} -- something else has patched it"
        ));
    }

    if !unsafe { crate::patch::write_code_bytes(address, &[want]) } {
        return Err("could not make the +showscores handler writable".to_string());
    }
    SUPPRESSED.store(hidden, Ordering::Release);
    Ok(true)
}

/// Whether `+showscores` is currently blocked in the loaded module.
pub fn suppressed() -> bool {
    SUPPRESSED.load(Ordering::Relaxed)
}

/// One line for `dodtools_status`.
pub fn status() -> String {
    if !suppressed() {
        return "the scoreboard behaves normally; a POV demo's recorded TAB presses will show it"
            .into();
    }
    "+showscores is blocked, so recorded TAB presses do not show the scoreboard \
     (round-end and spec_scoreboard are not covered)"
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `JE_AT` has to index the `74` in `PATTERN`, and nothing at runtime can
    /// check that before the patch is already resolved. Derive it from the
    /// pattern text itself.
    #[test]
    fn je_at_indexes_the_conditional_jump_in_the_pattern() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(
            tokens[JE_AT], "74",
            "JE_AT points at {:?}, not the conditional jump",
            tokens[JE_AT]
        );
        // The displacement has to survive unchanged -- that is the only reason
        // a one-byte edit works at all.
        assert_eq!(tokens[JE_AT + 1], "05");
    }

    /// The show and hide handlers are byte-identical up to the vftable
    /// displacement, so a pattern that stopped short of it would match both and
    /// `find_unique` would refuse -- a loud failure, but the reason is worth
    /// pinning.
    #[test]
    fn the_pattern_runs_past_the_vftable_displacement() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(&tokens[tokens.len() - 3..], &["FF", "60", "18"]);
    }

    /// `74` and `EB` take the same operand, which is why the displacement byte
    /// after them is left alone.
    #[test]
    fn the_two_opcodes_are_the_same_width() {
        assert_ne!(JE, JMP);
        assert_eq!(JE & 0xf0, 0x70, "74 is the short-conditional family");
        assert_eq!(JMP, 0xEB, "EB is short unconditional, also rel8");
    }

    /// The pattern must be parseable by the scanner and start on a real byte.
    #[test]
    fn the_pattern_is_well_formed() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(tokens[0], "68", "the handler starts with `push imm32`");
        assert!(tokens.len() > JE_AT + 1);
        for token in &tokens {
            assert!(
                *token == "??" || u8::from_str_radix(token, 16).is_ok(),
                "{token:?} is not a hex byte or a wildcard"
            );
        }
    }

    /// The two states this module writes are the only two it will accept back,
    /// so a third value has to be a foreign patch rather than a stale one.
    #[test]
    fn status_text_names_the_gap_it_does_not_cover() {
        SUPPRESSED.store(true, Ordering::Release);
        let text = status();
        assert!(text.contains("spec_scoreboard"), "{text}");
        assert!(text.contains("round-end"), "{text}");
        SUPPRESSED.store(false, Ordering::Release);
        assert!(status().contains("normally"));
    }
}
