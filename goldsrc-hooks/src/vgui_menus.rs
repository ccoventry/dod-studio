//! `dodstudio_hide_menus`: keep the team menu, the class menus and the MOTD
//! window a POV demo replays off the screen (#677), while a demo plays.
//!
//! ## What opens them
//!
//! The server opens each one with a user message, and a POV demo records it:
//! `VGUIMenu` (one byte of menu number first: 2 is the team menu, and the
//! demos also carry 10 and 13 as the player picks a class) and `MOTD` (the
//! message of the day, in chunks). Both go through `client.dll` thunks
//! (`+0x20dd0`, `+0x20d10`, `msglog.rs`'s table) that only forward to
//! `DoDViewport`'s own methods (vtable `+0x10`, `+0x24`), and nothing else
//! calls those thunks. So dropping the two messages keeps every one of these
//! windows shut, and touches nothing else.
//!
//! Measured offline across the pre-Anniversary install's demo library: the
//! POV demos recorded from joining the server carry the `MOTD` chunks and a
//! team-menu `VGUIMenu` at the same moment -- often in the demo's loading
//! segment, before playback starts -- and a class-menu `VGUIMenu` when the
//! player picks a team or changes class.
//!
//! `dodstudio_hide_hudelement menu` doesn't reach them: that is the old text
//! menu (`ShowMenu`). HLAE's `mirv_movie_hidepanels 1` keeps VGUI panels out
//! of its recordings, but not off the screen, so OBS capture shows them.
//!
//! ## Only during a demo
//!
//! Dropped only while `pDemoAPI->IsPlayingback()` says a demo is playing.
//! In live play the menus are how a player joins a team, so they always
//! reach the game, cvar or not.
//!
//! A window already open when the cvar turns on stays open: there is no
//! message to drop for it. So the cvar has to be on before the demo loads
//! (the launch line or a config), not only from a command the demo runs.
//!
//! ## Mechanism
//!
//! `map_text`'s: `pfnHookUserMsg` prepends, the game's handler is called
//! through `msglog`'s thunk table, nothing is patched. Both messages arrive
//! from the network, so the engine's by-number dispatcher stops at our
//! record. While `dodstudio_debug_msglog` watches one of them, its record sits
//! in front and asks [`hide`] instead.
//!
//! The hooks go in as soon as the engine's own `HUD_Frame` drives us, not
//! only once a level is loaded like `map_text`'s: the MOTD and team menu sit
//! in the loading segment, which may well be read before the first frame
//! that sees the level, and a missed menu stays open. Read offline in the
//! pre-Anniversary `hw.dll`, hooking that early is safe: the engine frees its
//! message list only where it unloads `client.dll` (`hw.dll+0xbda0`, which
//! calls `HUD_Shutdown` and `FreeLibrary`), and `svc_newusermsg`
//! (`+0x1a550`) numbers **every** record with the message's name, so a record
//! prepended after `client.dll`'s `HUD_Init` (which `HUD_Frame` comes after)
//! stays in front and gets each connect's message number.

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;

pub const NAME: &str = console_name!("hide_menus");

const VGUI_MENU: &str = "VGUIMenu";
const MOTD: &str = "MOTD";
const MESSAGES: [&str; 2] = [VGUI_MENU, MOTD];

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

/// Whether `name` is one of the two messages this drops.
fn is_menu_message(name: &str) -> bool {
    MESSAGES.iter().any(|m| m.eq_ignore_ascii_case(name))
}

/// What to call one in the log.
fn describe(name: &str, payload: &[u8]) -> String {
    if name.eq_ignore_ascii_case(VGUI_MENU) {
        match payload.first() {
            Some(2) => "the team menu (VGUIMenu 2)".to_string(),
            Some(menu) => format!("VGUIMenu {menu}"),
            None => "an empty VGUIMenu".to_string(),
        }
    } else {
        format!("a MOTD chunk ({} bytes)", payload.len())
    }
}

/// Whether to drop this message: the cvar is on, it is one of [`MESSAGES`],
/// and a demo is playing. Also called from `msglog`'s handler.
pub(crate) fn hide(name: &str, payload: &[u8]) -> bool {
    if !WANTED.load(Ordering::Relaxed) || !is_menu_message(name) || !engine::playing_demo() {
        return false;
    }
    let count = HIDDEN.fetch_add(1, Ordering::Relaxed) + 1;
    if count <= LOGGED_MAX {
        unsafe {
            crate::debug::report(&format!(
                "vgui_menus: hid {}{}",
                describe(name, payload),
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

unsafe extern "C" fn hooked(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if name.is_null() {
        return 1;
    }
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let payload: &[u8] = if size > 0 && !buf.is_null() {
        unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) }
    } else {
        &[]
    };
    if hide(&name_str, payload) {
        // 1 is what the engine's own dispatcher treats as handled.
        return 1;
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

/// Called every frame from `commands::poll`. Follows the cvar and, while it
/// is on, keeps the hooks in the engine's list: from the first real
/// `HUD_Frame` (see the module doc), or once a level is loaded if that hook
/// never lands. Re-hooking is idempotent: the engine returns early when a
/// record with this handler is already there.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if wanted != WANTED.swap(wanted, Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "vgui_menus: {NAME} = {}",
                if wanted {
                    "1 (team/class menus and the MOTD hidden in demos)"
                } else {
                    "0 (normal)"
                }
            ))
        };
    }
    // Off leaves any installed record in place: it forwards everything while
    // WANTED is false, and there is no unhook to ask the engine for.
    if !wanted || !(engine::hud_frame_driving() || crate::map_text::level_name().is_some()) {
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
    let wanted = WANTED.load(Ordering::Relaxed);
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if !wanted && hidden == 0 {
        return None;
    }
    Some(format!(
        "{NAME} = {} -- {hidden} menu/MOTD message(s) hidden this session (demos only)",
        u8::from(wanted)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_window_messages_are_menu_messages() {
        assert!(is_menu_message("VGUIMenu"));
        assert!(is_menu_message("vguimenu"));
        assert!(is_menu_message("MOTD"));
        for other in ["ShowMenu", "TextMsg", "HudText", "ServerName", "", "MOTDx"] {
            assert!(!is_menu_message(other), "{other}");
        }
    }

    /// Both have a thunk in `msglog`'s table, which is what the hook
    /// forwards to.
    #[test]
    fn both_messages_have_a_thunk_to_forward_to() {
        for message in MESSAGES {
            assert!(crate::msglog::find_thunk(message).is_some(), "{message}");
        }
    }

    #[test]
    fn the_log_names_the_team_menu() {
        assert_eq!(
            describe("VGUIMenu", &[2, 0, 0, 0, 0]),
            "the team menu (VGUIMenu 2)"
        );
        assert_eq!(describe("VGUIMenu", &[10, 0xff]), "VGUIMenu 10");
        assert_eq!(describe("VGUIMenu", &[]), "an empty VGUIMenu");
        assert_eq!(describe("MOTD", &[0; 62]), "a MOTD chunk (62 bytes)");
    }

    /// Off, or outside a demo (no engine in a test), nothing is dropped.
    #[test]
    fn nothing_is_dropped_outside_a_demo() {
        assert!(!hide("VGUIMenu", &[2]));
        assert!(!hide("MOTD", &[]));
    }
}
