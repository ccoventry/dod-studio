//! `ds_hud_overview_team_marker`: in HLTV, mark the player being watched
//! on the overview map with their team's camera icon, not `spec_camera.spr`
//! (`1`), or leave the camera icon off the map altogether (`2`, #677).
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
//! player icons, so the watched player stands out over their own icon.
//!
//! ## No camera icon (`2`)
//!
//! Whichever sprite was chosen, the draw is skipped when it is null:
//!
//! ```text
//!     +0x2f43b  test esi, esi
//!     +0x2f43d  je   +0x2f4a5               ; <- 74 66, made EB 66
//!     +0x2f43f  fld  [...]                  ; the draw
//!     ...
//!     +0x2f4a5  pop  edi                    ; the function's epilogue
//! ```
//!
//! With that `je` made a `jmp`, the game takes its own "no sprite" exit, the
//! one it takes when a camera sprite failed to load, so nothing is left half
//! done (the x87 stack is balanced there: the `fld` is after the branch).
//! It works in POV and HLTV alike, since both reach this draw. Only the
//! camera icon goes; the player icons were drawn in the loop before it.
//! Without it the only way to hide the icon is blanking
//! `sprites/spec_camera.spr`, which only HLTV draws: POV draws its team's
//! `*_camera.spr` (#677). `tools/verify_hide_text_offsets.py` checks both
//! branches, the `fld` after the skip and the epilogue it lands on.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("hud_overview_team_marker");

/// From the `IsSpectateOnly` call, through the team sprite choice, to the
/// `je` that skips the draw when no sprite was chosen.
const PATTERN: &str = "FF 15 ?? ?? ?? ?? 85 C0 74 08 8B 35 ?? ?? ?? ?? EB 26 \
                       83 3D ?? ?? ?? ?? 02 75 08 8B 35 ?? ?? ?? ?? EB 15 \
                       A0 ?? ?? ?? ?? 8B 35 ?? ?? ?? ?? 84 C0 75 06 8B 35 ?? ?? ?? ?? \
                       85 F6 74 66";
/// The sprite choice's `je` in [`PATTERN`].
const JE_AT: usize = 8;
/// The draw's "no sprite" `je` in [`PATTERN`].
const SKIP_AT: usize = 58;
const JE: u8 = 0x74;
const JMP: u8 = 0xEB;

/// The game's own: `spec_camera.spr` in HLTV, the team's icon in POV.
pub const GAME: i32 = 0;
/// The team's camera icon in HLTV too.
pub const TEAM: i32 = 1;
/// No camera icon at all.
pub const NONE: i32 = 2;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The pattern's start, 0 before a successful scan.
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The setting both bytes currently follow.
static MODE: AtomicI32 = AtomicI32::new(GAME);
static COMPLAINED: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn pattern_address() -> Result<usize, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: a mapped module, for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the overview map's camera icon -- {why}"))?;
    ADDRESS.store(found, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(found)
}

/// Puts one short `je` in the wanted state, returning whether it was written.
fn set_branch(address: usize, jump: bool, what: &str) -> Result<bool, String> {
    let want = if jump { JMP } else { JE };
    // Safety: the scan proved this byte is mapped code.
    let present = unsafe { *(address as *const u8) };
    if present == want {
        return Ok(false);
    }
    if present != JE && present != JMP {
        return Err(format!(
            "{what} holds {present:#04x}, neither {JE:#04x} nor {JMP:#04x} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &[want]) } {
        return Err(format!("could not make {what} writable"));
    }
    Ok(true)
}

/// Applies one setting, returning whether a byte was written. Decides from
/// the bytes, so it is cheap to call every frame.
pub fn set_mode(mode: i32) -> Result<bool, String> {
    let found = pattern_address()?;
    let chose = set_branch(
        found + JE_AT,
        mode == TEAM,
        "the camera icon's sprite choice",
    )?;
    let skipped = set_branch(found + SKIP_AT, mode == NONE, "the camera icon's draw")?;
    MODE.store(mode, Ordering::Release);
    Ok(chose || skipped)
}

fn describe(mode: i32) -> &'static str {
    match mode {
        TEAM => "1: in HLTV the watched player is marked with their team's camera icon",
        NONE => "2: no camera icon on the overview map",
        _ => "0: in HLTV the watched player is marked with spec_camera.spr",
    }
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let raw = unsafe { (*cvar).value };
    let wanted = if raw.is_finite() { raw as i32 } else { GAME };
    let wanted = if (GAME..=NONE).contains(&wanted) {
        wanted
    } else {
        GAME
    };
    match set_mode(wanted) {
        Ok(false) => COMPLAINED.store(false, Ordering::Relaxed),
        Ok(true) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!("overview_marker: {NAME} = {}", describe(wanted)))
            };
        }
        Err(why) => {
            if wanted != GAME && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "overview_marker: {NAME} not applied yet -- {why}"
                    ))
                };
            }
        }
    }
}

/// One line for `ds_debug_status`, while it is not the game's own:
/// the setting, and the team the game is choosing the icon by right now.
pub fn status_line() -> Option<String> {
    let mode = MODE.load(Ordering::Relaxed);
    if mode == GAME {
        return None;
    }
    let team = engine::client_module_base()
        // Safety: `g_iTeamNumber`, a fixed .data offset in client.dll.
        .map(|base| unsafe { *((base + 0xe8_8d0) as *const i32) });
    Some(format!(
        "overview map marker: {NAME} = {}; g_iTeamNumber = {team:?}",
        describe(mode)
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

    #[test]
    fn skip_at_indexes_the_no_sprite_je() {
        let t: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(&t[SKIP_AT - 2..SKIP_AT], &["85", "F6"], "test esi, esi");
        assert_eq!(&t[SKIP_AT..], &["74", "66"], "je, the pattern's last bytes");
    }

    #[test]
    fn every_setting_has_words_and_out_of_range_is_the_games() {
        for mode in GAME..=NONE {
            assert!(describe(mode).starts_with(&mode.to_string()));
        }
        assert_eq!(describe(GAME), describe(7));
    }
}
