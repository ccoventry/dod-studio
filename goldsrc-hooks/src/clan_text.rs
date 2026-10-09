//! `dodstudio_hide_clan_text`: hide all of DoD's clan-match text -- the
//! warm-up and ready rules, "Warmup Mode", the countdown to the match and
//! "MATCH IS LIVE!" -- so clips from a match's warm-up and start don't carry
//! it (#608).
//!
//! ## Where each piece comes from
//!
//! `dod/resource/dod_english.txt` holds eight clan keys and nothing else about
//! clan matches. Measured offline against the shipped `client.dll` (one file,
//! byte-identical on both installs) and a match demo
//! (`bandits-ktps5w10-map2-anzio-over-axis-milo`); the reconstructed
//! `dod13-client` source agrees, except that it names the countdown key
//! `#Clan_match_start` where the binary has `#Clan_time_remaining`:
//!
//! - **`TextMsg`** from the server: `#clan_warmup_rules`, `#clan_ready_rules`
//!   and one `#Clan_time_remaining` centre-print as the countdown starts
//!   (`#Clan_awaiting_ready` would come the same way). Dropped whole when the
//!   key starts `#clan_`, in any case.
//! - **`ClanTimer`**, one byte of seconds: `client.dll` stores
//!   `seconds + time` (`+0x2cce0`) and `CHudDoDCrossHair::DrawClanTimer` then
//!   draws `#Clan_time_remaining` every frame until the last two seconds, and
//!   `#Clan_match_live` for those. So both the countdown and "MATCH IS LIVE!"
//!   are this one message, and it is swallowed.
//! - **`GameRules`**, two bytes of flags: bit 4 of the second is warm-up mode
//!   (`+0x2c650` stores it at `+0x108eb0`), which makes the objective icons'
//!   `Draw` put `clan_warmup_mode` on the left of the screen. Forwarded with
//!   that bit cleared; every other flag (paratroopers, infinite lives,
//!   British) still reaches the game.
//!
//! `#Clan_axis_ready` and `#Clan_allies_ready` arrive on `HudText`, which the
//! game never draws (checked live 2026-10-05), so there is nothing to hide.
//!
//! ## Turning it on or off mid-demo
//!
//! The countdown and warm-up mode are state the game already holds, so the
//! last `ClanTimer` and `GameRules` seen are kept and re-delivered to the
//! game's own handlers on a change: a `ClanTimer` of 0 (or of the seconds
//! still left) and the `GameRules` with or without the bit. Only on the level
//! they arrived on, so another map's flags are never replayed.
//!
//! Stopping the countdown is not enough on its own: `DrawClanTimer` posts its
//! line every frame to a centre-text object (`+0x175c6c`, through
//! `+0x3a830`), which keeps the last line it got on screen until 4 s after
//! the post (client time + 4.0 at the object's `+0x14`). So turning the cvar
//! on mid-countdown left the last second frozen there for 4 s (live test,
//! 2026-10-09). The re-delivery also ends that line's hold.
//!
//! ## Mechanism
//!
//! `map_text`'s: `pfnHookUserMsg` prepends, the game's handler is called
//! through `msglog`'s thunk table, nothing is patched. While
//! `dodstudio_debug_msglog` watches one of these messages its record sits in
//! front and asks [`filter`] instead, so the two don't take turns at the head
//! of the engine's list.

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;

pub const NAME: &str = console_name!("hide_clan_text");

const TEXT_MSG: &str = "TextMsg";
const CLAN_TIMER: &str = "ClanTimer";
const GAME_RULES: &str = "GameRules";
const MESSAGES: [&str; 3] = [TEXT_MSG, CLAN_TIMER, GAME_RULES];

/// Every clan key starts with this, in one case or the other.
const CLAN_KEY_PREFIX: &[u8] = b"#clan_";

/// `GameRules`' second byte: the warm-up mode flag.
const WARMUP_BIT: u8 = 4;

/// The end of `DrawClanTimer` (`+0x2d3e3`): `push 1.0; push ebx; push -1;
/// push 0; push 0; push edx; push eax; push ecx; mov ecx, <centre-text
/// object>; call`. Unique in `client.dll`, the same on both installs.
const POST_PATTERN: &str = "68 00 00 80 3F 53 6A FF 6A 00 6A 00 52 50 51 B9";
/// Where the object's address sits in [`POST_PATTERN`]'s `mov ecx`.
const POST_OBJECT_AT: usize = 16;
/// The object's hold: the client time its line stops showing, an `f32`.
const HOLD_UNTIL: usize = 0x14;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The cvar's value as last seen: what the message handler reads.
static WANTED: AtomicBool = AtomicBool::new(false);
/// Messages hidden or changed this session.
static HIDDEN: AtomicU32 = AtomicU32::new(0);

/// The last `ClanTimer` and `GameRules` the server sent, for re-delivery when
/// the cvar changes.
static LAST: Mutex<Last> = Mutex::new(Last {
    level: String::new(),
    clan_timer: None,
    game_rules: None,
});

struct Last {
    /// The level both arrived on.
    level: String,
    /// Seconds, and the client time it arrived at.
    clan_timer: Option<(u8, f64)>,
    game_rules: Option<Vec<u8>>,
}

/// Called by `commands.rs` once `dodstudio_hide_clan_text` is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

// ── Pure decisions ───────────────────────────────────────────────────────────

/// Whether a `TextMsg` payload (destination byte, then the key) is clan text.
fn is_clan_text_msg(payload: &[u8]) -> bool {
    payload
        .get(1..)
        .and_then(|key| key.get(..CLAN_KEY_PREFIX.len()))
        .is_some_and(|start| start.eq_ignore_ascii_case(CLAN_KEY_PREFIX))
}

/// `GameRules` as the game should see it: unchanged, or with warm-up off.
fn game_rules_for_game(payload: &[u8], hide: bool) -> Vec<u8> {
    let mut out = payload.to_vec();
    if hide && let Some(flags) = out.get_mut(1) {
        *flags &= !WARMUP_BIT;
    }
    out
}

/// The whole seconds a countdown of `seconds` that started at `started` has
/// left at `now`, or `None` once it's over (or the clock went backwards: a
/// demo restarting).
fn seconds_left(seconds: u8, started: f64, now: f64) -> Option<u8> {
    let elapsed = now - started;
    if !(0.0..f64::from(seconds)).contains(&elapsed) {
        return None;
    }
    // Rounded up, so a countdown with any time left keeps running.
    Some((f64::from(seconds) - elapsed).ceil() as u8)
}

// ── Engine side ──────────────────────────────────────────────────────────────

fn level() -> String {
    crate::map_text::level_name().unwrap_or_default()
}

/// Hands `payload` to `client.dll`'s own handler for `name`.
fn deliver(name: &str, payload: &mut [u8]) -> i32 {
    let (Some(original), Ok(c_name)) = (crate::msglog::original_thunk(name), CString::new(name))
    else {
        return 1;
    };
    let size = i32::try_from(payload.len()).unwrap_or(0);
    // Safety: the game's own pfnUserMsgHook, with a buffer we own for the call.
    unsafe { original(c_name.as_ptr(), size, payload.as_mut_ptr().cast()) }
}

fn note_hidden(what: &str) {
    HIDDEN.fetch_add(1, Ordering::Relaxed);
    unsafe { crate::debug::report(&format!("clan_text: hid {what}")) };
}

/// Decides what happens to one of [`MESSAGES`]: `Some(result)` when it was
/// dropped or handed on changed, `None` to forward the original untouched.
/// Also called from `msglog`'s handler -- see the module doc.
pub(crate) fn filter(name: &str, size: i32, buf: *mut c_void) -> Option<i32> {
    let payload: &[u8] = if size > 0 && !buf.is_null() {
        unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) }
    } else {
        &[]
    };
    let hide = WANTED.load(Ordering::Relaxed);
    if name.eq_ignore_ascii_case(TEXT_MSG) {
        if hide && is_clan_text_msg(payload) {
            let key = payload[1..].split(|&b| b == 0).next().unwrap_or_default();
            note_hidden(&format!("TextMsg \"{}\"", String::from_utf8_lossy(key)));
            return Some(1);
        }
        return None;
    }
    if name.eq_ignore_ascii_case(CLAN_TIMER) {
        let seconds = payload.first().copied().unwrap_or(0);
        remember(|last| last.clan_timer = Some((seconds, engine::client_time())));
        if hide {
            note_hidden(&format!("ClanTimer {seconds} s"));
            return Some(1);
        }
        return None;
    }
    if name.eq_ignore_ascii_case(GAME_RULES) {
        remember(|last| last.game_rules = Some(payload.to_vec()));
        if hide && payload.get(1).is_some_and(|flags| flags & WARMUP_BIT != 0) {
            note_hidden("GameRules warm-up mode");
            return Some(deliver(GAME_RULES, &mut game_rules_for_game(payload, true)));
        }
        return None;
    }
    None
}

/// Updates [`LAST`], first forgetting what an earlier level left there.
fn remember(update: impl FnOnce(&mut Last)) {
    let level = level();
    if let Ok(mut last) = LAST.lock() {
        if last.level != level {
            last.level = level;
            last.clan_timer = None;
            last.game_rules = None;
        }
        update(&mut last);
    }
}

/// Ends the hold on the countdown's last line, so it goes the moment the
/// countdown stops instead of 4 s later. Only called while the countdown is
/// running, when that line is the one the object shows.
fn end_countdown_line() -> Result<(), String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session.
    let at = unsafe { crate::scan::find_unique(base, POST_PATTERN) }
        .map_err(|why| format!("could not find DrawClanTimer's post -- {why}"))?;
    // Safety: the pattern matched, so the `mov ecx, imm32` operand is there;
    // the loader has already relocated it to the object's live address.
    let object = unsafe { std::ptr::read_unaligned((at + POST_OBJECT_AT) as *const u32) } as usize;
    if !(base..base + 0x0100_0000).contains(&object) {
        return Err(format!(
            "the centre-text object {object:#x} is outside client.dll"
        ));
    }
    // Safety: a static object inside client.dll's image; the game writes this
    // `f32` from the same thread, in the same frame loop.
    unsafe { std::ptr::write_volatile((object + HOLD_UNTIL) as *mut f32, 0.0) };
    Ok(())
}

/// Re-delivers what the game already holds, as it should now look.
fn redeliver(hide: bool) {
    let level = level();
    let (clan_timer, game_rules) = match LAST.lock() {
        Ok(last) if last.level == level => (last.clan_timer, last.game_rules.clone()),
        _ => return,
    };
    if let Some((seconds, started)) = clan_timer {
        let now = engine::client_time();
        match seconds_left(seconds, started, now) {
            Some(left) => {
                // 0 makes `DrawClanTimer`'s "still running" test false at once.
                let sent = if hide { 0 } else { left };
                let result = deliver(CLAN_TIMER, &mut [sent]);
                let line = if hide {
                    match end_countdown_line() {
                        Ok(()) => ", its last line cleared".to_string(),
                        Err(why) => format!(", its last line stays up to 4 s: {why}"),
                    }
                } else {
                    String::new()
                };
                unsafe {
                    crate::debug::report(&format!(
                        "clan_text: re-delivered ClanTimer {sent} ({left} s of {seconds} left, handler returned {result}){line}"
                    ))
                };
            }
            None => unsafe {
                crate::debug::report(&format!(
                    "clan_text: ClanTimer {seconds} not re-delivered: started at {started:.2}, now {now:.2}"
                ))
            },
        }
    }
    if let Some(rules) = game_rules
        && rules.get(1).is_some_and(|flags| flags & WARMUP_BIT != 0)
    {
        deliver(GAME_RULES, &mut game_rules_for_game(&rules, hide));
    }
}

unsafe extern "C" fn hooked(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    let name_str = if name.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(name) }.to_string_lossy())
    };
    let Some(name_str) = name_str else {
        return 1;
    };
    if let Some(result) = filter(&name_str, size, buf) {
        return result;
    }
    match crate::msglog::original_thunk(&name_str) {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

fn install_hook(message: &str) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let Ok(name) = CString::new(message) else {
        return;
    };
    unsafe { (engfuncs.pfn_hook_user_msg)(name.as_ptr(), hooked) };
}

/// Called every frame from `commands::poll`. Follows the cvar, puts the
/// game's countdown and warm-up flag right on a change, and keeps the hooks in
/// place.
///
/// The hooks go in on the first frame a level is loaded, on or off, so the
/// last `ClanTimer` and `GameRules` are already known when the cvar is turned
/// on mid-demo. Not before: hooks prepended at the main menu never saw a
/// message (live test, 2026-10-08), while the same handler hooked once a demo
/// was playing (through `dodstudio_debug_msglog`) saw every one -- the same
/// timing `map_text`, `deathmsg` and `msglog` use.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if wanted != WANTED.swap(wanted, Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "clan_text: {NAME} = {}",
                if wanted {
                    "1 (clan-match text hidden)"
                } else {
                    "0 (normal)"
                }
            ))
        };
        redeliver(wanted);
    }
    if crate::map_text::level_name().is_none() {
        return;
    }
    for message in MESSAGES {
        if !crate::msglog::watching(message) {
            install_hook(message);
        }
    }
}

/// A `dodstudio_debug_status` line, or `None` while off and never used.
pub fn status_line() -> Option<String> {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return None;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if !wanted && hidden == 0 {
        return None;
    }
    Some(format!(
        "{NAME} = {} -- {hidden} clan-match message(s) hidden this session",
        if wanted { "1" } else { "0" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `TextMsg` as the server writes it: destination, key, arguments.
    fn text_msg(dest: u8, parts: &[&str]) -> Vec<u8> {
        let mut out = vec![dest];
        for part in parts {
            out.extend(part.as_bytes());
            out.push(0);
        }
        out
    }

    /// Every clan key the game has that arrives as a `TextMsg`, in the case
    /// the game spells it.
    #[test]
    fn every_clan_text_msg_is_clan_text() {
        for key in [
            "#clan_warmup_rules",
            "#clan_ready_rules",
            "#Clan_time_remaining",
            "#Clan_awaiting_ready",
            "#Clan_match_live",
        ] {
            assert!(is_clan_text_msg(&text_msg(4, &[key])), "{key}");
        }
        // As the test demo carries it, with its two arguments.
        assert!(is_clan_text_msg(&text_msg(
            4,
            &["#Clan_time_remaining", "0", "10"]
        )));
        assert!(is_clan_text_msg(&text_msg(
            4,
            &["#clan_ready_rules", "ready"]
        )));
    }

    #[test]
    fn other_text_msgs_pass() {
        for key in [
            "#game_roundstart_allie1",
            "#Game_connected",
            "clan",
            "#clan",
            "#Clan",
        ] {
            assert!(!is_clan_text_msg(&text_msg(4, &[key])), "{key}");
        }
        assert!(!is_clan_text_msg(&[]));
        assert!(!is_clan_text_msg(&[4]));
    }

    /// The key starts after the destination byte, not at the payload's start.
    #[test]
    fn the_destination_byte_is_not_part_of_the_key() {
        let mut shifted = b"#clan_ready_rules\0".to_vec();
        shifted.insert(0, b'x');
        assert!(is_clan_text_msg(&shifted));
        assert!(!is_clan_text_msg(b"#clan_ready_rules\0"));
    }

    #[test]
    fn only_the_warmup_flag_is_cleared() {
        assert_eq!(game_rules_for_game(&[0x07, 0x07], true), vec![0x07, 0x03]);
        assert_eq!(game_rules_for_game(&[0x07, 0x07], false), vec![0x07, 0x07]);
        assert_eq!(game_rules_for_game(&[0x00, 0x04], true), vec![0x00, 0x00]);
        // Bit 4 of the *first* byte is British, not warm-up.
        assert_eq!(game_rules_for_game(&[0x04, 0x00], true), vec![0x04, 0x00]);
        assert_eq!(game_rules_for_game(&[0x04], true), vec![0x04]);
        assert!(game_rules_for_game(&[], true).is_empty());
    }

    #[test]
    fn a_countdown_has_its_seconds_left_rounded_up() {
        assert_eq!(seconds_left(10, 100.0, 100.0), Some(10));
        assert_eq!(seconds_left(10, 100.0, 103.2), Some(7));
        assert_eq!(seconds_left(10, 100.0, 109.9), Some(1));
    }

    #[test]
    fn an_over_or_rewound_countdown_has_nothing_left() {
        assert_eq!(seconds_left(10, 100.0, 110.0), None);
        assert_eq!(seconds_left(10, 100.0, 250.0), None);
        assert_eq!(seconds_left(10, 100.0, 50.0), None, "demo restarted");
        assert_eq!(seconds_left(0, 100.0, 100.0), None);
    }
}
