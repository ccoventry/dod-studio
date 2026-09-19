//! `dodtools_mute_voice_commands`: silence DoD's spoken voice commands without
//! overwriting the game's own `.wav` files.
//!
//! ## The problem this exists for
//!
//! "Fire in the hole!", "Move up!", "Yes sir!" -- the radio/voice commands
//! players bind to keys. In a demo they replay exactly as they were pressed,
//! and they land on top of whatever the clip is actually about.
//!
//! The workaround before this was to overwrite every `player/us*.wav`,
//! `player/brit*.wav` and `player/ger*.wav` in the game folder with a blank
//! sound. That works, but it is destroying shipped game content to make a
//! per-take decision -- and it is the sort of change this pipeline otherwise
//! refuses to make on a user's install (`CLAUDE.md`'s rule about the game's own
//! files).
//!
//! ## Where the sound comes from
//!
//! DoD plays voice commands through exactly two client event callbacks,
//! registered in `HUD_Init`:
//!
//! ```text
//!     pfnHookEvent("events/misc/usvoice.sc",  client+0xb3f0)   ; US *and* British
//!     pfnHookEvent("events/misc/gervoice.sc", client+0xb5d0)   ; German
//! ```
//!
//! Behind them sit three contiguous 28-entry `const char*` tables -- exactly
//! the files the blank-`.wav` trick overwrites:
//!
//! ```text
//!     +0x1c55c8   player/usattack.wav,   player/ushold.wav,   ...
//!     +0x1c5638   player/britattack.wav, player/brithold.wav, ...
//!     +0x1c56a8   player/gerattack.wav,  player/gerhold.wav,  ...
//! ```
//!
//! Each callback ends up at `pEventAPI->EV_PlaySound`, reached by
//! `call dword ptr [ebx]` -- `FF 13`.
//!
//! ## What this patches
//!
//! Those two `call`s become `90 90`. Two bytes each.
//!
//! The stack stays balanced without any other change, because the argument
//! cleanup is caller-side and separate (`add esp, 0x20` in the US/British
//! callback, `add esp, 0x24` in the German one) -- removing the call does not
//! move it.
//!
//! **This is the reason to patch the call rather than stub the callback.**
//! Everything after the call still runs, and what it does is print the chat
//! line. So this behaves exactly like the blank-`.wav` approach -- audio gone,
//! everything else untouched -- where a `ret` at the top of the callback would
//! have silently taken the chat line with it.
//!
//! The tail, read rather than assumed: it gets the speaker and the local
//! player, **returns if they are not on the same team**, **returns if the
//! observer-mode global is non-zero -- i.e. whenever you are spectating**,
//! returns if the speaker is too far away, and only then formats
//! `(PlayerName): <text>` from the `#Voice_subtitle_*` table and prints it
//! behind the `#VOICE` prefix.
//!
//! The practical consequence: in an HLTV demo the chat line never appears in
//! the first place, so this setting only ever removes the sound. In a POV demo
//! the chat line stays, exactly as it did with blanked `.wav` files. Removing
//! that too would be a `ret` at the top of each callback -- a second mode, not
//! a different design.
//!
//! ## Scope, stated plainly
//!
//! Voice **commands** only. Pain, death and hurt sounds are different events
//! and are not touched. Nor is player voice chat (`voice_modenable`) or its
//! `sprites/voiceicon.spr` speaker icon, which belong to `CVoiceStatus` -- a
//! separate system with nothing to do with these callbacks.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The cvar name, for status and error text. Registered in `commands.rs`.
pub const NAME: &str = console_name!("mute_voice_commands");

/// `call dword ptr [ebx]` -- `pEventAPI->EV_PlaySound`.
const CALL: &[u8] = &[0xff, 0x13];
/// Two `nop`s, the same width.
const NOPS: &[u8] = &[0x90, 0x90];

/// One of the two voice event callbacks, identified by the span that ends in
/// its `EV_PlaySound` call.
struct Site {
    /// For error text: which callback this is.
    what: &'static str,
    /// The signature, ending on the `FF 13` this module rewrites.
    pattern: &'static str,
    /// Byte offset of that `FF 13` within the match.
    call_at: usize,
}

/// The US and British voices share a callback; which table it indexes is an
/// event parameter, so both are covered by this one site.
const ALLIED: Site = Site {
    what: "the US/British voice callback",
    pattern: "D9 5C 24 04 83 C4 04 8D 4C 24 ?? 50 6A 02 51 56 FF 13",
    call_at: 16,
};

const GERMAN: Site = Site {
    what: "the German voice callback",
    // The wildcarded dword is the German sound table's absolute address, which
    // carries a base relocation and so cannot be matched literally.
    pattern: "8B 04 BD ?? ?? ?? ?? 83 C4 04 8D 4C 24 ?? D9 1C 24 50 6A 02 51 56 FF 13",
    call_at: 22,
};

const SITES: &[&Site] = &[&ALLIED, &GERMAN];

/// Resolved addresses of the two `FF 13`s, or 0 before the first scan.
static CALL_ADDRESSES: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// The module base the addresses were resolved against. Measured (five game
/// sessions, five `LoadLibraryA("client.dll")` log lines, no reload between
/// demos inside a session, `docs/goldsrc_dod_quirks.md`): a plain demo change
/// does not reload `client.dll`. This guards a rescan for whatever *would*
/// reload it -- a mod change, returning to the menu -- neither of which has
/// been tested.
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Whether the calls are currently patched out in the loaded module.
static MUTED: AtomicBool = AtomicBool::new(false);

/// Resolves both call sites for the currently loaded `client.dll`.
fn call_addresses() -> Result<[usize; 2], String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };

    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = [
            CALL_ADDRESSES[0].load(Ordering::Acquire),
            CALL_ADDRESSES[1].load(Ordering::Acquire),
        ];
        if cached[0] != 0 && cached[1] != 0 {
            return Ok(cached);
        }
    }

    let mut found = [0usize; 2];
    for (slot, site) in SITES.iter().enumerate() {
        // Safety: `client_module_base` only returns a base for a mapped module,
        // and it stays mapped for the session.
        let at = unsafe { scan::find_unique(base, site.pattern) }
            .map_err(|why| format!("could not find {} -- {why}", site.what))?;
        let address = at + site.call_at;

        // The scan proved the bytes are there; reading them back proves
        // `call_at` still indexes the ones the pattern matched, which is what a
        // later edit to the pattern could silently break.
        let present = unsafe { std::slice::from_raw_parts(address as *const u8, CALL.len()) };
        if present != CALL {
            return Err(format!(
                "+{:#x} holds {present:02x?}, not the expected {CALL:02x?} -- call_at does not line up with {}'s pattern",
                address - base,
                site.what
            ));
        }
        found[slot] = address;
    }

    for (slot, address) in found.iter().enumerate() {
        CALL_ADDRESSES[slot].store(*address, Ordering::Release);
    }
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(found)
}

/// Applies or removes the mute, returning whether anything was written.
///
/// Idempotent and cheap to call every frame, which is how it is used --
/// deciding from the bytes rather than a flag is what would make this
/// self-heal if `client.dll` is ever unloaded and reloaded (see
/// [`SCANNED_BASE`]), coming back with the original `call` in place.
pub fn set_muted(muted: bool) -> Result<bool, String> {
    let addresses = call_addresses()?;
    let want: &[u8] = if muted { NOPS } else { CALL };

    let mut wrote = false;
    for address in addresses {
        let present = unsafe { std::slice::from_raw_parts(address as *const u8, CALL.len()) };
        if present == want {
            continue;
        }
        // Anything but the two encodings this module writes means something
        // else is patching the same site -- refuse rather than stamp over it.
        if present != CALL && present != NOPS {
            return Err(format!(
                "a voice callback holds {present:02x?}, which is neither {CALL:02x?} nor {NOPS:02x?} -- something else has patched it"
            ));
        }
        if !unsafe { crate::patch::write_code_bytes(address, want) } {
            return Err("could not make a voice callback writable".to_string());
        }
        wrote = true;
    }

    MUTED.store(muted, Ordering::Release);
    Ok(wrote)
}

/// Whether voice commands are currently silenced in the loaded module.
pub fn muted() -> bool {
    MUTED.load(Ordering::Relaxed)
}

/// One line for `dodtools_status`.
pub fn status() -> String {
    if !muted() {
        return "voice commands play normally".into();
    }
    "voice commands are silent; the POV chat line still shows (there is none \
     while spectating), and pain and death sounds are not affected"
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `call_at` has to index the `FF 13` in each pattern, and nothing at
    /// runtime can check that before the patch is already resolved. Derive it
    /// from the pattern text itself.
    #[test]
    fn call_at_indexes_the_play_sound_call_in_every_pattern() {
        for site in SITES {
            let tokens: Vec<&str> = site.pattern.split_whitespace().collect();
            assert_eq!(
                (tokens[site.call_at], tokens[site.call_at + 1]),
                ("FF", "13"),
                "{}'s call_at points at {:?}, not `call dword ptr [ebx]`",
                site.what,
                &tokens[site.call_at..site.call_at + 2]
            );
        }
    }

    /// The call is the last thing each pattern covers, so the signature cannot
    /// accidentally describe a longer span than it rewrites.
    #[test]
    fn every_pattern_ends_on_the_call_it_rewrites() {
        for site in SITES {
            let tokens: Vec<&str> = site.pattern.split_whitespace().collect();
            assert_eq!(site.call_at + 2, tokens.len(), "{}", site.what);
        }
    }

    /// The replacement has to be exactly as wide as what it replaces -- the
    /// caller-side `add esp, N` that follows assumes the arguments are still
    /// on the stack and that nothing after the call has moved.
    #[test]
    fn the_replacement_is_the_same_width_as_the_call() {
        assert_eq!(CALL.len(), NOPS.len());
        assert!(NOPS.iter().all(|&b| b == 0x90));
    }

    #[test]
    fn the_patterns_are_well_formed_and_distinct() {
        assert_ne!(ALLIED.pattern, GERMAN.pattern);
        for site in SITES {
            let tokens: Vec<&str> = site.pattern.split_whitespace().collect();
            assert!(tokens.len() > site.call_at + 1);
            for token in &tokens {
                assert!(
                    *token == "??" || u8::from_str_radix(token, 16).is_ok(),
                    "{token:?} in {} is not a hex byte or a wildcard",
                    site.what
                );
            }
        }
        assert_eq!(SITES.len(), CALL_ADDRESSES.len());
    }

    #[test]
    fn status_says_what_is_left_alone() {
        MUTED.store(true, Ordering::Release);
        let text = status();
        assert!(text.contains("chat line"), "{text}");
        assert!(text.contains("pain and death"), "{text}");
        MUTED.store(false, Ordering::Release);
        assert!(status().contains("normally"));
    }
}
