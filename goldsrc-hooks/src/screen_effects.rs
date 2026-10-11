//! `dodstudio_hide_headshot_red_screen` and `dodstudio_hide_screen_shake`: keep two
//! server effects out of a POV demo's picture (#606).
//!
//! ## What they are
//!
//! Both are user messages the server sent to the recording player, so they are
//! in the demo and replay with it (43 POV demos scanned, see #606):
//!
//! - **The red head-hit flash:** a `ScreenFade` of colour (128,0,0) at alpha
//!   200, flags 0, 3 s fade with a 2 s hold. It comes with every hit on the
//!   head hitbox, fatal or not, grenades included. The only other fades in
//!   the demos are black: the clear at respawn (alpha 0) and a rare end-of-map
//!   one, which this leaves alone.
//! - **The blast shake:** a `ScreenShake`, always amplitude 15, 1 s, from
//!   grenades going off near the player (no WSOD25 map has an `env_shake`).
//!
//! ## How
//!
//! `client.dll` registers neither message: `hw.dll` hooks both itself in
//! `ClientDLL_Init` (`V_ScreenFade` pre-Anniversary `hw.dll+0xc2580`, 25th
//! Anniversary `+0x232160`; `V_ScreenShake` `+0xc24e0` / `+0x2322c0`). The
//! engine's own handler is found at the registration site (`push handler;
//! push "ScreenFade"; call HookUserMsg`), and ours is hooked in front of it
//! the way `deathmsg.rs` does for `DeathMsg`: `pfnHookUserMsg` prepends, the
//! dispatcher stops at the first match, and re-hooking every frame survives a
//! reconnect. Everything that isn't dropped goes to the engine's handler
//! untouched. `tools/verify_screen_effects_offsets.py` checks the lookup on
//! both builds.

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial, UserMsgHookFn};
use crate::names::console_name;

pub const FLASH_NAME: &str = console_name!("hide_headshot_red_screen");
pub const SHAKE_NAME: &str = console_name!("hide_screen_shake");

static FLASH: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static SHAKE: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// The engine's handlers; 0 before a look, 1 when one couldn't be found.
static FADE_HANDLER: AtomicUsize = AtomicUsize::new(0);
static SHAKE_HANDLER: AtomicUsize = AtomicUsize::new(0);
/// Set once either cvar has been on: from then on our handlers stay in front
/// (forwarding everything while the cvars are 0).
static WANTED: AtomicBool = AtomicBool::new(false);

pub fn set_cvars(flash: *mut CvarSPartial, shake: *mut CvarSPartial) {
    FLASH.store(flash, Ordering::Release);
    SHAKE.store(shake, Ordering::Release);
}

fn on(cvar: &AtomicPtr<CvarSPartial>) -> bool {
    let cvar = cvar.load(Ordering::Acquire);
    // Safety: the engine owns the cvar for the session.
    !cvar.is_null() && unsafe { (*cvar).value } != 0.0
}

/// Whether a `ScreenFade` payload is the head-hit flash: (128,0,0) with flags
/// 0, the only red fade a DoD server sends.
fn is_head_hit_flash(payload: &[u8]) -> bool {
    // u16 duration, u16 hold time, i16 flags, u8 r, g, b, a.
    payload.len() >= 10 && payload[4..6] == [0, 0] && payload[6..9] == [128, 0, 0]
}

/// The handler `hw.dll` registers for `message` (with its trailing NUL), from
/// the `push handler; push "<message>"; call` at its registration.
fn engine_handler(message: &[u8]) -> Option<usize> {
    let base = engine::engine_module_base()?;
    // Safety: a module handle the loader gave us, mapped up to its image size.
    let image = unsafe {
        let size = crate::pe::image_size(base as *mut u8)?;
        std::slice::from_raw_parts(base as *const u8, size)
    };
    // The string itself: `message` right after a NUL, so "XScreenFade" can't
    // match.
    let at = image
        .windows(message.len() + 1)
        .position(|w| w[0] == 0 && &w[1..] == message)?
        + 1;
    let string = (base + at) as u32;
    let s = string.to_le_bytes();
    let pattern = format!(
        "68 ?? ?? ?? ?? 68 {:02X} {:02X} {:02X} {:02X} E8",
        s[0], s[1], s[2], s[3]
    );
    // Safety: as above.
    let push = unsafe { crate::scan::find_unique(base, &pattern) }.ok()?;
    let handler = unsafe { std::ptr::read_unaligned((push + 1) as *const u32) } as usize;
    (base..base + image.len())
        .contains(&handler)
        .then_some(handler)
}

fn handler(slot: &AtomicUsize, message: &[u8]) -> Option<UserMsgHookFn> {
    let found = match slot.load(Ordering::Acquire) {
        0 => {
            let found = engine_handler(message).unwrap_or(1);
            slot.store(found, Ordering::Release);
            if found == 1 {
                let name = String::from_utf8_lossy(&message[..message.len() - 1]);
                unsafe {
                    crate::debug::report(&format!(
                        "screen_effects: hw.dll's {name} handler wasn't found, so it can't be filtered"
                    ))
                };
            }
            found
        }
        found => found,
    };
    // Safety: the engine's own pfnUserMsgHook for this message.
    (found != 1).then(|| unsafe { std::mem::transmute::<usize, UserMsgHookFn>(found) })
}

unsafe extern "C" fn hooked_fade(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if on(&FLASH) && size >= 10 && !buf.is_null() {
        // Safety: the engine hands over `size` bytes of payload.
        let payload = unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) };
        if is_head_hit_flash(payload) {
            // 1 is what the dispatcher treats as handled.
            return 1;
        }
    }
    match handler(&FADE_HANDLER, b"ScreenFade\0") {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

unsafe extern "C" fn hooked_shake(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if on(&SHAKE) {
        return 1;
    }
    match handler(&SHAKE_HANDLER, b"ScreenShake\0") {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

/// Every frame: hooks both messages once either setting has been on.
pub fn poll() {
    if !WANTED.load(Ordering::Relaxed) {
        if !on(&FLASH) && !on(&SHAKE) {
            return;
        }
        WANTED.store(true, Ordering::Relaxed);
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Found before hooking, while the engine's own record is still the one
    // the registration site names.
    if handler(&FADE_HANDLER, b"ScreenFade\0").is_none()
        || handler(&SHAKE_HANDLER, b"ScreenShake\0").is_none()
    {
        return;
    }
    for (message, ours) in [
        ("ScreenFade", hooked_fade as UserMsgHookFn),
        ("ScreenShake", hooked_shake as UserMsgHookFn),
    ] {
        if let Ok(name) = CString::new(message) {
            // Safety: the engine's own hook function; it returns early when
            // the first record for the name already has this handler.
            unsafe { (engfuncs.pfn_hook_user_msg)(name.as_ptr(), ours) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fade(duration: u16, hold: u16, flags: i16, rgba: [u8; 4]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&duration.to_le_bytes());
        v.extend_from_slice(&hold.to_le_bytes());
        v.extend_from_slice(&flags.to_le_bytes());
        v.extend_from_slice(&rgba);
        v
    }

    #[test]
    fn the_red_flash_is_recognised() {
        assert!(is_head_hit_flash(&fade(12288, 8192, 0, [128, 0, 0, 200])));
    }

    #[test]
    fn the_black_fades_pass() {
        // The respawn clear and the end-of-map fade.
        assert!(!is_head_hit_flash(&fade(4, 0, 0, [0, 0, 0, 0])));
        assert!(!is_head_hit_flash(&fade(4096, 61440, 5, [0, 0, 0, 0])));
        // Red but with flags set: not the server's flash.
        assert!(!is_head_hit_flash(&fade(12288, 8192, 1, [128, 0, 0, 200])));
        assert!(!is_head_hit_flash(&[128, 0, 0]));
    }
}
