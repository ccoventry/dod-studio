//! `ds_hud_hide_plugin_text`: hide what a server plugin puts on screen --
//! its coloured HUD messages (`set_hudmessage` / `show_hudmessage`: match,
//! stats and advert lines, #677) and its menus (AMX Mod X menus and votes,
//! #342) -- and nothing else. It was `ds_hud_hide_server_text` before #342
//! (never in a release).
//!
//! This file is the HUD-message half. The menus are `ShowMenu` text menus,
//! which `voice_menus.rs` already hooks for the voice menus: it hides every
//! other `ShowMenu` menu while [`wanted`] is true. `dod.dll` itself sends no
//! `ShowMenu` but the nine voice menus, so any other one comes from a plugin.
//!
//! ## What carries them
//!
//! A plugin's HUD message is a `TE_TEXTMESSAGE` temp entity (`svc_temp_entity`
//! 29), not a user message. `hw.dll`'s `CL_ParseTEnt` reads it into one of
//! four engine-owned `client_textmessage_t`s, named `__NETMESSAGE__1`..`4` by
//! channel, and then calls `CL_HudMessage(name)`, which hands the *name* to
//! `client.dll` as a `HudText` message. `CHudMessage` then asks the engine for
//! the text with `pfnTextMessageGet(name)` and draws it.
//!
//! So it does reach `HudText`, but not in a way a hook can stop.
//! `CL_HudMessage` goes through the engine's by-name dispatcher, which calls
//! **every** `HudText` record in the list, not just the newest
//! (see [`crate::engine::HookUserMsgFn`]): `client.dll`'s own handler runs
//! after any hook of ours, whatever the hook returns. That is also why
//! `ds_hud_hide_map_text` can't hide a map's `game_text`, which travels
//! the same way.
//!
//! ## The fix: one call in `hw.dll`
//!
//! The `TE_TEXTMESSAGE` case's call to `CL_HudMessage` is pointed at
//! [`filtered_hud_message`] while the cvar is on, and put back at 0:
//!
//! ```text
//!     call MSG_ReadString
//!     push eax                  ; the text
//!     push <channel buffer>
//!     call strncpy              ; 511 bytes into the channel's buffer
//!     push [msg.pName]          ; "__NETMESSAGE__<channel>"
//!     mov  byte [buffer+511], 0
//!     call CL_HudMessage        ; E8 rel32   <- call filtered_hud_message
//!     add  esp, 0x10            ; the caller pops all four arguments
//! ```
//!
//! | build | the call | `CL_HudMessage` |
//! | --- | --- | --- |
//! | pre-Anniversary | `hw.dll+0x23da4` | `+0x19ab0` |
//! | 25th Anniversary | `+0x1b0a0d` | `+0x1a30a0` |
//!
//! [`filtered_hud_message`] takes the same one argument, cdecl, and returns
//! nothing, like `CL_HudMessage`. It drops the message, or calls the real
//! `CL_HudMessage` with the same name. The message has been read whole by
//! then, so the stream stays in step either way.
//!
//! ## What stays
//!
//! - **The map's own text.** A message whose text is a `message` the loaded
//!   map declares on a text entity (`map_text`'s list, which in practice means
//!   a `game_text`) is passed on. Hiding that is `ds_hud_hide_map_text`'s
//!   job, not this one's.
//! - **Everything on the `HudText` user message itself**: map text
//!   (`env_message`, `dod_score_ent`) and DoD's own prompts come that way,
//!   never as a `TE_TEXTMESSAGE`, so they never reach this call.
//! - **HLTV director text** (`svc_director`), which `client.dll` draws
//!   straight from the director message, and the HLTV proxy's own text
//!   (`ds_spec_hide_hltv_messages`).
//!
//! A message already on screen when the cvar turns on stays until it fades.
//! `CHudMessage` draws it from the engine's channel buffer, which the engine
//! refills before the call, so until then it may show the newer text on the
//! same channel.
//!
//! `tools/verify_engine_text_offsets.py` checks both builds' signatures
//! against both installs' `hw.dll`: the channel-name table, that the call
//! goes to `CL_HudMessage` (which dispatches `"HudText"` by name), that the
//! caller pops the argument, and that nothing branches into the call.

use std::ffi::c_char;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self};
use crate::hw_call_site::{Build, CALL_LEN, CallSite, call_bytes};
use crate::names::console_name;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("hud_hide_plugin_text");

/// From the channel-name table read to the call and the `add esp, 0x10`
/// after it; only the patched call's `rel32` is pinned.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        pattern: "8B 14 BD ?? ?? ?? ?? 68 FF 01 00 00 C1 E7 09 89 56 24 8D 9F ?? ?? ?? ?? \
                  89 5E 28 E8 ?? ?? ?? ?? 50 53 E8 ?? ?? ?? ?? 8B 46 24 C6 87 ?? ?? ?? ?? 00 \
                  50 E8 07 5D FF FF 83 C4 10",
        at: 50,
    },
    Build {
        name: "25th Anniversary",
        pattern: "8B 04 9D ?? ?? ?? ?? C1 E3 09 68 FF 01 00 00 89 47 24 8D B3 ?? ?? ?? ?? \
                  89 77 28 E8 ?? ?? ?? ?? 50 56 E8 ?? ?? ?? ?? FF 77 24 C6 83 ?? ?? ?? ?? 00 \
                  E8 8E 26 FF FF 83 C4 10",
        at: 49,
    },
];

/// The engine's text buffer per channel is 512 bytes, NUL at the last.
const MAX_TEXT: usize = 512;

/// Hidden messages logged one by one before only the count goes on.
const LOGGED_MAX: u32 = 20;

static SITE: CallSite = CallSite::new("TE_TEXTMESSAGE's call to CL_HudMessage", &BUILDS);

pub static CVAR: Cvar = Cvar::new();
/// The cvar's value as last seen: what [`filtered_hud_message`] reads.
static WANTED: AtomicBool = AtomicBool::new(false);
/// Set the first time the cvar is on, so a session that never asks for it
/// never scans `hw.dll`.
static TOUCHED: AtomicBool = AtomicBool::new(false);
/// Whether the call is ours right now.
static REDIRECTED: AtomicBool = AtomicBool::new(false);
/// Whether a failure has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);
/// The real `CL_HudMessage`, read from the stock call.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// Messages hidden this session.
static HIDDEN: AtomicU32 = AtomicU32::new(0);

type HudMessageFn = unsafe extern "C" fn(name: *const c_char);

/// Whether the cvar is on: `map_text` reads the map's strings for it then,
/// and `voice_menus` hides plugin menus.
pub(crate) fn wanted() -> bool {
    WANTED.load(Ordering::Relaxed)
}

/// The text up to its NUL, at most [`MAX_TEXT`] bytes.
///
/// Safety: `text` is null or points at a readable buffer of at least
/// [`MAX_TEXT`] bytes or a NUL before its end -- the engine's channel buffer.
unsafe fn read_text<'a>(text: *const c_char) -> &'a [u8] {
    if text.is_null() {
        return &[];
    }
    let bytes = text as *const u8;
    let mut len = 0;
    while len < MAX_TEXT && unsafe { *bytes.add(len) } != 0 {
        len += 1;
    }
    unsafe { std::slice::from_raw_parts(bytes, len) }
}

/// The text a `TE_TEXTMESSAGE` channel holds, from the engine.
fn channel_text(name: *const c_char) -> Vec<u8> {
    let Some(engfuncs) = engine::engfuncs() else {
        return Vec::new();
    };
    // Safety: the engine's own lookup, with the name it just passed us; it
    // returns its static channel message for a `__NETMESSAGE__` name.
    let message = unsafe { (engfuncs.pfn_text_message_get)(name) };
    if message.is_null() {
        return Vec::new();
    }
    // Safety: a live client_textmessage_t; its text is the channel buffer.
    unsafe { read_text((*message).p_message) }.to_vec()
}

/// Whether to drop this message, counting and logging it if so.
fn hide(name: *const c_char) -> bool {
    if !WANTED.load(Ordering::Relaxed) {
        return false;
    }
    let text = channel_text(name);
    if text.iter().all(u8::is_ascii_whitespace) {
        // An empty message clears its channel (KTP sends one before each
        // update); dropped like the rest, but not worth a count or a line.
        return true;
    }
    if crate::map_text::declares_sent_text(&text) {
        return false;
    }
    let count = HIDDEN.fetch_add(1, Ordering::Relaxed) + 1;
    if count <= LOGGED_MAX {
        unsafe {
            crate::debug::report(&format!(
                "plugin_text: hid \"{}\"{}",
                String::from_utf8_lossy(&text).trim().replace('\n', " / "),
                if count == LOGGED_MAX {
                    " (the rest are only counted)"
                } else {
                    ""
                }
            ))
        };
    }
    true
}

/// What the `TE_TEXTMESSAGE` case calls instead of `CL_HudMessage` while the
/// cvar is on: same argument, same convention, nothing returned.
unsafe extern "C" fn filtered_hud_message(name: *const c_char) {
    if hide(name) {
        return;
    }
    let original = ORIGINAL.load(Ordering::Acquire);
    if original != 0 {
        // Safety: the address the stock call went to, CL_HudMessage itself.
        let original = unsafe { std::mem::transmute::<usize, HudMessageFn>(original) };
        unsafe { original(name) };
    }
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let Some(wanted) = CVAR.try_on() else {
        return;
    };
    if wanted != WANTED.swap(wanted, Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "plugin_text: {NAME} = {}",
                if wanted {
                    "1 (server plugin HUD messages and menus hidden)"
                } else {
                    "0 (normal)"
                }
            ))
        };
    }
    if wanted {
        TOUCHED.store(true, Ordering::Relaxed);
    } else if !TOUCHED.load(Ordering::Relaxed) {
        return;
    }
    let result = SITE.resolve().and_then(|address| {
        if let Some(original) = SITE.original_target() {
            ORIGINAL.store(original, Ordering::Release);
        }
        let ours: [u8; CALL_LEN] =
            call_bytes(address, filtered_hud_message as HudMessageFn as usize);
        SITE.put(wanted, ours)
    });
    match result {
        Ok(wrote) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            REDIRECTED.store(wanted, Ordering::Relaxed);
            if wrote {
                unsafe {
                    crate::debug::report(&format!(
                        "plugin_text: TE_TEXTMESSAGE {} ({} hw.dll)",
                        if wanted {
                            "now goes through the filter"
                        } else {
                            "goes straight to the game again"
                        },
                        SITE.build_name().unwrap_or("unknown")
                    ))
                };
            }
        }
        Err(why) => {
            REDIRECTED.store(false, Ordering::Relaxed);
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("plugin_text: {NAME} not applied -- {why}"))
                };
            }
        }
    }
}

/// A `ds_debug_status` line, or `None` while off and never used.
pub fn status_line() -> Option<String> {
    let wanted = WANTED.load(Ordering::Relaxed);
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if !wanted && hidden == 0 {
        return None;
    }
    let state = if wanted && !REDIRECTED.load(Ordering::Relaxed) {
        " (NOT applied -- see the log)"
    } else {
        ""
    };
    Some(format!(
        "{NAME} = {}{state} -- {hidden} plugin HUD message(s) hidden this session",
        u8::from(wanted)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_build_pins_the_call_and_the_callers_cleanup() {
        crate::hw_call_site::check_builds(&BUILDS);
        for build in &BUILDS {
            let t: Vec<&str> = build.pattern.split_whitespace().collect();
            // `add esp, 0x10` right after the call: the caller pops.
            assert_eq!(
                &t[build.at + CALL_LEN..build.at + CALL_LEN + 3],
                &["83", "C4", "10"],
                "{}",
                build.name
            );
            // The strncpy of 511 bytes into the channel buffer.
            assert!(build.pattern.contains("68 FF 01 00 00"), "{}", build.name);
        }
    }

    #[test]
    fn text_stops_at_its_nul_or_the_buffers_end() {
        let short = b"Round 2 starts in 10\0garbage";
        assert_eq!(
            unsafe { read_text(short.as_ptr().cast()) },
            b"Round 2 starts in 10"
        );
        let long = vec![b'x'; MAX_TEXT + 64];
        assert_eq!(unsafe { read_text(long.as_ptr().cast()) }.len(), MAX_TEXT);
        assert!(unsafe { read_text(std::ptr::null()) }.is_empty());
    }
}
