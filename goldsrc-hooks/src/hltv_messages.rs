//! `dodstudio_hide_hltv_messages`: hide the text an HLTV proxy puts on screen
//! during playback -- "You're watching HLTV. Visit www.valvesoftware.com",
//! and any line a proxy operator sends with `msg` -- without patching the
//! demo (issue #30).
//!
//! ## Where the text comes from
//!
//! The proxy sends it as `svc_director` (51) with the sub-command
//! `DRC_CMD_MESSAGE` (6). The engine strips the opcode and length byte and
//! hands the rest to `HUD_DirectorMessage(int iSize, void *pbuf)` -- slot 38
//! of the `cldll_func_t` table -- which forwards to
//! `CHudSpectator::DirectorMessage` (`client.dll+0x38030`). That reads the
//! first byte as the command and switches on it; case 6 (`+0x381bc`) puts the
//! text up as a HUD message. See `docs/goldsrc_client_dll_internals.md` §7.
//!
//! Measured on two HLTV demos: 39 and 29 of these per demo, about one a
//! minute, every one the same Valve line.
//!
//! ## What this does
//!
//! `engine.rs` swaps slot 38 for a trampoline that asks [`should_drop`]
//! first. With the cvar at 1, a message whose command byte is 6 never reaches
//! `client.dll`; every other command goes through untouched. That includes
//! `DRC_CMD_STUFFTEXT` (10), which is what the pipeline's own highlight
//! labels are, and the camera, mode and event commands.
//!
//! Nothing is patched and no address is involved: the slot is found the same
//! way as the other four `engine.rs` swaps, and `client.dll` is byte-identical
//! in the pre-Anniversary and 25th Anniversary installs, so this works on
//! both.
//!
//! ## What it doesn't hide
//!
//! Text that arrives any other way -- chat, a map's `game_text`, centre-print.
//! And a message already on screen when the cvar is turned on stays until it
//! fades on its own; only the ones after that are dropped.
//!
//! Default 0, like every other `dodstudio_` switch.

use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::engine::CvarSPartial;
use crate::names::console_name;

pub const NAME: &str = console_name!("hide_hltv_messages");

/// `hltv.h`'s `DRC_CMD_MESSAGE`.
const DRC_CMD_MESSAGE: u8 = 6;

/// Where a `DRC_CMD_MESSAGE`'s text starts, counted from the command byte:
/// command (1), effect (1), colour (4), then x, y, fade-in, fade-out, hold
/// and fx time as six floats (24). The same layout
/// `native::patch::build_director_message` writes.
const TEXT_OFFSET: usize = 30;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// How many messages have been dropped this session.
static DROPPED: AtomicU32 = AtomicU32::new(0);

/// Called by `commands.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn enabled() -> bool {
    let cvar = CVAR.load(Ordering::Acquire);
    // Safety: a cvar the engine registered for us, alive for the session.
    !cvar.is_null() && unsafe { (*cvar).value } != 0.0
}

/// Whether a director message buffer, as `HUD_DirectorMessage` receives it,
/// is a `DRC_CMD_MESSAGE`.
fn is_text_message(buf: &[u8]) -> bool {
    buf.first() == Some(&DRC_CMD_MESSAGE)
}

/// The text of a `DRC_CMD_MESSAGE`, for the log. `None` for any other
/// command, or one too short to have reached its text.
fn message_text(buf: &[u8]) -> Option<String> {
    if !is_text_message(buf) {
        return None;
    }
    let text = buf.get(TEXT_OFFSET..)?;
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    Some(String::from_utf8_lossy(&text[..end]).into_owned())
}

/// The decision itself, separate from the cvar so it can be tested.
fn decide(enabled: bool, buf: &[u8]) -> bool {
    enabled && is_text_message(buf)
}

/// Called by `engine.rs`'s `HUD_DirectorMessage` trampoline for every director
/// message. True means "don't pass this one on".
pub fn should_drop(buf: &[u8]) -> bool {
    if !decide(enabled(), buf) {
        return false;
    }
    let n = DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
    // About once a minute of playback, so one line each is no flood.
    unsafe {
        crate::debug::report(&format!(
            "hltv_messages: hid {:?} ({n} so far)",
            message_text(buf).unwrap_or_default()
        ))
    };
    true
}

/// A `dodstudio_debug_status` line, or `None` while off and never used.
pub fn status_line() -> Option<String> {
    let on = enabled();
    let dropped = DROPPED.load(Ordering::Relaxed);
    if !on && dropped == 0 {
        return None;
    }
    Some(format!(
        "{NAME} = {} -- hid {dropped} HLTV text message(s) this session",
        if on { "1" } else { "0" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `DRC_CMD_MESSAGE` buffer as the engine hands it over: no opcode or
    /// length byte, command first.
    fn message(text: &str) -> Vec<u8> {
        let mut buf = vec![DRC_CMD_MESSAGE, 0];
        buf.extend_from_slice(&[0xFF, 0xA0, 0x00, 0x00]);
        for value in [-1.0f32, 0.85, 0.5, 0.5, 3.0, 0.0] {
            buf.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(buf.len(), TEXT_OFFSET);
        buf.extend_from_slice(text.as_bytes());
        buf.push(0);
        buf
    }

    #[test]
    fn drops_a_text_message_only_while_on() {
        let buf = message("You're watching HLTV. Visit www.valvesoftware.com");
        assert!(decide(true, &buf));
        assert!(!decide(false, &buf));
    }

    /// Everything else a proxy sends has to reach the client: the pipeline's
    /// own highlight labels are `DRC_CMD_STUFFTEXT` (10), and the rest drive
    /// the spectator camera.
    #[test]
    fn passes_every_other_command_through() {
        for command in [1u8, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12] {
            let mut buf = message("x");
            buf[0] = command;
            assert!(!decide(true, &buf), "command {command}");
        }
        let stufftext = b"\x0aecho [dod-studio] MATCH_START\0";
        assert!(!decide(true, stufftext));
    }

    #[test]
    fn an_empty_buffer_is_passed_through() {
        assert!(!decide(true, &[]));
        assert_eq!(message_text(&[]), None);
    }

    #[test]
    fn reads_the_text_for_the_log() {
        let buf = message("You're watching HLTV. Visit www.valvesoftware.com");
        assert_eq!(
            message_text(&buf).as_deref(),
            Some("You're watching HLTV. Visit www.valvesoftware.com")
        );
    }

    /// A truncated message is still dropped -- the command byte is what counts
    /// -- but has no text to log, and must not read past the end.
    #[test]
    fn a_truncated_message_is_dropped_without_reading_past_it() {
        let buf = &message("hello")[..TEXT_OFFSET - 4];
        assert!(decide(true, buf));
        assert_eq!(message_text(buf), None);

        let mut unterminated = message("hello");
        unterminated.pop();
        assert_eq!(message_text(&unterminated).as_deref(), Some("hello"));
    }

    #[test]
    fn other_commands_have_no_text() {
        assert_eq!(message_text(b"\x0aecho hi\0"), None);
    }
}
