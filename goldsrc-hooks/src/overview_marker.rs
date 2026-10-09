//! `dodstudio_hud_map_team_marker`: in HLTV, mark the player being watched
//! on the overview map with his team's camera icon, not `spec_camera.spr`.
//!
//! ## What the game does
//!
//! After the player icons, `CHudDoDMap::DrawOverviewEntities` draws one more
//! icon at the camera, on top of the rest. Which sprite depends on whether
//! the viewer is HLTV (`gEngfuncs.IsSpectateOnly`):
//!
//! ```text
//!     +0x2f403  call [IsSpectateOnly]
//!     +0x2f409  test eax, eax
//!     +0x2f40b  je   +0x2f415               ; <- 74 08, made EB 08
//!     +0x2f40d  mov  esi, [spec_camera]     ; HLTV: one sprite for everyone
//!     +0x2f413  jmp  draw
//!     +0x2f415  cmp  [g_iTeamNumber], 2     ; otherwise by team:
//!     ...                                   ; axis_camera, brit_camera
//!                                           ; (British maps) or allies_camera
//! ```
//!
//! With the `je` made a `jmp`, HLTV takes the team branch too.
//! `g_iTeamNumber` is the watched player's team while spectating: the map's
//! own team filter (`overview_players`) compares against it, and that is
//! what keeps the watched player's side on the map.
//!
//! The stock `*_camera.spr` files are byte-for-byte the player icons, so on
//! their own this changes nothing visible. DoD Studio puts bordered, themed
//! ones in `dod_addon/sprites`; the game draws the camera icon about 1.5x the
//! player icons, so the watched player stands out over his own icon.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("hud_map_team_marker");

/// From the `IsSpectateOnly` call to the team test's `jne`.
const PATTERN: &str = "FF 15 ?? ?? ?? ?? 85 C0 74 08 8B 35 ?? ?? ?? ?? EB 26 \
                       83 3D ?? ?? ?? ?? 02 75 08";
/// The `je` in [`PATTERN`].
const JE_AT: usize = 8;
const JE: u8 = 0x74;
const JMP: u8 = 0xEB;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
static ON: AtomicBool = AtomicBool::new(false);
static COMPLAINED: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn je_address() -> Result<usize, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: a mapped module, for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the camera icon's sprite choice -- {why}"))?;
    let address = found + JE_AT;
    ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Applies or removes the change, returning whether a byte was written.
/// Decides from the byte, so it is cheap to call every frame.
pub fn set_on(on: bool) -> Result<bool, String> {
    let address = je_address()?;
    let want = if on { JMP } else { JE };
    // Safety: the scan proved this byte is mapped code.
    let present = unsafe { *(address as *const u8) };
    if present == want {
        ON.store(on, Ordering::Release);
        return Ok(false);
    }
    if present != JE && present != JMP {
        return Err(format!(
            "the camera icon's sprite choice holds {present:#04x}, neither {JE:#04x} nor {JMP:#04x} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &[want]) } {
        return Err("could not make the camera icon's sprite choice writable".to_string());
    }
    ON.store(on, Ordering::Release);
    Ok(true)
}

fn describe(on: bool) -> &'static str {
    if on {
        "1: in HLTV the watched player is marked with his team's camera icon"
    } else {
        "0: in HLTV the watched player is marked with spec_camera.spr"
    }
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    match set_on(wanted) {
        Ok(false) => COMPLAINED.store(false, Ordering::Relaxed),
        Ok(true) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!("overview_marker: {NAME} = {}", describe(wanted)))
            };
        }
        Err(why) => {
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "overview_marker: {NAME} not applied yet -- {why}"
                    ))
                };
            }
        }
    }
}

/// One line for `dodstudio_debug_status`, once it has been on: the setting,
/// and the team the game is choosing the icon by right now.
pub fn status_line() -> Option<String> {
    if !ON.load(Ordering::Relaxed) {
        return None;
    }
    let team = engine::client_module_base()
        // Safety: `g_iTeamNumber`, a fixed .data offset in client.dll.
        .map(|base| unsafe { *((base + 0xe8_8d0) as *const i32) });
    Some(format!(
        "overview map marker: {NAME} = {}; g_iTeamNumber = {team:?}",
        describe(true)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn je_at_indexes_the_short_je() {
        let t: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(&t[JE_AT..JE_AT + 2], &["74", "08"]);
        // The two branches the jump chooses between are what follow it.
        assert_eq!(
            &t[JE_AT + 2..JE_AT + 4],
            &["8B", "35"],
            "mov esi, [spec_camera]"
        );
        assert_eq!(
            &t[JE_AT + 10..JE_AT + 12],
            &["83", "3D"],
            "cmp [g_iTeamNumber], 2"
        );
    }
}
