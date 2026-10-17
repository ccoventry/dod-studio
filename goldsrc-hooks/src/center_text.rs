//! `dodstudio_hide_center_text`: drop every `TextMsg` the server sends to the
//! middle of the screen (`HUD_PRINTCENTER`, destination 4) -- match plugins'
//! `print_center` countdowns and notices among them (#677).
//!
//! ## Where it comes from
//!
//! `CHudTextMessage::MsgFunc_TextMsg` (`+0x4c250`) switches on the first
//! byte. Destination 4 goes to `+0x4c36a`, which posts the line to the
//! centre-print object (`+0x175c6c`) through `+0x3a830`; it then shows until
//! 4 s after the post. Destinations 1 (notify), 2 (console) and 3 (chat) go
//! elsewhere and are left alone, so joins, leaves and chat lines still show.
//!
//! The only narrower-than-this switch before was `dodstudio_hide_hudelement
//! vgui2print 1`, which also takes the machine gun's "Deploy to reload" hint.
//!
//! ## What it doesn't reach
//!
//! - The spectator camera mode's name: `CHudSpectator::SetMode` calls
//!   `MsgFunc_TextMsg` directly, not through the message dispatcher, so this
//!   never sees it. `dodstudio_hide_spectator_text` hides that one.
//! - The viewport's own way in: entry 13 of the function table at
//!   `+0xc1aa0` (`+0x20480`) also calls `MsgFunc_TextMsg` directly. What, if
//!   anything, uses it has not been looked into.
//! - The clan-match countdown: `ClanTimer` posts to the same object every
//!   frame without a `TextMsg`; `dodstudio_hide_clan_text` covers it.
//! - The engine's own `svc_centerprint`, which `client.dll` never sees.
//!
//! ## Mechanism
//!
//! `clan_text` already keeps a `TextMsg` handler at the front of the engine's
//! list on every level, so its filter asks [`hide`] rather than a second
//! handler taking turns with it. Turning the cvar on also ends the line on
//! screen at that moment (`clan_text::end_center_line`), so a notice isn't
//! left up for its last 4 s.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use crate::engine::CvarSPartial;
use crate::names::console_name;

pub const NAME: &str = console_name!("hide_center_text");

/// `TextMsg`'s destination byte for the middle of the screen.
const HUD_PRINTCENTER: u8 = 4;

/// Hidden messages logged one by one before only the count goes on.
const LOGGED_MAX: u32 = 20;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The cvar's value as last seen: what the message handler reads.
static WANTED: AtomicBool = AtomicBool::new(false);
/// Messages hidden this session.
static HIDDEN: AtomicU32 = AtomicU32::new(0);

/// Called by `commands.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// Whether a `TextMsg` payload (destination byte, then the text or key) goes
/// to the middle of the screen.
fn is_center_text(payload: &[u8]) -> bool {
    payload.first() == Some(&HUD_PRINTCENTER)
}

/// Whether `clan_text`'s `TextMsg` filter should drop this payload. Called
/// with the game's message handling waiting on the answer, so it costs an
/// atomic load and a byte compare unless it hides something.
pub(crate) fn hide(payload: &[u8]) -> bool {
    if !WANTED.load(Ordering::Relaxed) || !is_center_text(payload) {
        return false;
    }
    let count = HIDDEN.fetch_add(1, Ordering::Relaxed) + 1;
    if count <= LOGGED_MAX {
        let text = payload[1..].split(|&b| b == 0).next().unwrap_or_default();
        unsafe {
            crate::debug::report(&format!(
                "center_text: hid \"{}\"{}",
                String::from_utf8_lossy(text).trim(),
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

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if wanted == WANTED.swap(wanted, Ordering::Relaxed) {
        return;
    }
    let cleared = if wanted {
        match crate::clan_text::end_center_line() {
            Ok(()) => ", the line on screen ended".to_string(),
            Err(why) => format!(", a line on screen stays up to 4 s: {why}"),
        }
    } else {
        String::new()
    };
    unsafe {
        crate::debug::report(&format!(
            "center_text: {NAME} = {}{cleared}",
            if wanted {
                "1 (server centre text hidden)"
            } else {
                "0 (normal)"
            }
        ))
    };
}

/// A `dodstudio_debug_status` line, or `None` while off and never used.
pub fn status_line() -> Option<String> {
    let wanted = WANTED.load(Ordering::Relaxed);
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if !wanted && hidden == 0 {
        return None;
    }
    Some(format!(
        "{NAME} = {} -- {hidden} centre-text message(s) hidden this session",
        u8::from(wanted)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_msg(dest: u8, text: &str) -> Vec<u8> {
        let mut out = vec![dest];
        out.extend(text.as_bytes());
        out.push(0);
        out
    }

    #[test]
    fn only_destination_four_is_centre_text() {
        assert!(is_center_text(&text_msg(4, "Match starts in 10")));
        assert!(is_center_text(&text_msg(4, "#Clan_time_remaining")));
        for dest in [1, 2, 3, 5, 0] {
            assert!(
                !is_center_text(&text_msg(dest, "#Game_joined_game")),
                "{dest}"
            );
        }
        assert!(!is_center_text(&[]));
        assert!(is_center_text(&[4]), "an empty centre line is still one");
    }
}
