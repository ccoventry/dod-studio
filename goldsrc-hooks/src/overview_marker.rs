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
//!
//! ## Whose team (#907)
//!
//! `g_iTeamNumber` is the viewer's own team, not the watched player's: in
//! HLTV it is 0 whoever the camera follows, so the team branch fell through
//! to `allies_camera` every time. With the setting at `1`, the `cmp`'s
//! address operand points at [`ICON_TEAM`] instead, which [`poll`] fills each
//! frame with the watched player's team from `g_PlayerExtraInfo` (the
//! `teamnumber` the game fills from each player's `entity_state.team` while
//! spectating, `+0x31063`):
//!
//! ```text
//!     g_iUser1 (+0xe88d4)  observer mode: 1/2 chase, 4 in-eye, 6 map chase
//!     g_iUser2 (+0xe88d8)  the watched player's entity index, 1..32
//!     +0xe9120 + 48*index  that player's teamnumber (short)
//! ```
//!
//! When nobody is watched (roaming, the free overview, or a team that is
//! neither Allies nor Axis), HLTV keeps the game's own `spec_camera.spr`: the
//! sprite-choice `je` is left as it is for those frames. A POV demo never
//! takes that `je`, so there the team is the watched player's while the
//! recorder spectates after a death and `g_iTeamNumber` otherwise.
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

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use crate::engine::{self};
use crate::names::console_name;
use crate::scan;
use crate::setting::Cvar;

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
/// The address operand of `cmp [g_iTeamNumber], 2` in [`PATTERN`].
const TEAM_OPERAND_AT: usize = 20;
const JE: u8 = 0x74;
const JMP: u8 = 0xEB;

/// The game's own: `spec_camera.spr` in HLTV, the team's icon in POV.
pub const GAME: i32 = 0;
/// The team's camera icon in HLTV too.
pub const TEAM: i32 = 1;
/// No camera icon at all.
pub const NONE: i32 = 2;

pub static CVAR: Cvar = Cvar::new();
/// The pattern's start, 0 before a successful scan.
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The setting both bytes currently follow.
static MODE: AtomicI32 = AtomicI32::new(GAME);
static COMPLAINED: AtomicBool = AtomicBool::new(false);
/// The team the camera icon is chosen by while the setting is `1`: the
/// game's `cmp` reads it in place of `g_iTeamNumber`.
static ICON_TEAM: AtomicI32 = AtomicI32::new(0);
/// The `cmp`'s own operand, `g_iTeamNumber`'s address, read before it is
/// pointed at [`ICON_TEAM`]; 0 before a scan.
static GAME_TEAM_ADDRESS: AtomicUsize = AtomicUsize::new(0);

/// `g_iUser1`, `g_iUser2`, and `g_PlayerExtraInfo[0].teamnumber`.
const MODE_RVA: usize = 0xe8_8d4;
const TARGET_RVA: usize = 0xe8_8d8;
pub(crate) const EXTRA_TEAM_RVA: usize = 0xe9_120;
/// `sizeof(extra_player_info_t)`.
pub(crate) const EXTRA_STRIDE: usize = 48;
/// Allies (British maps included) and Axis.
const ALLIES: i32 = 1;
const AXIS: i32 = 2;

/// The pattern's start and the client.dll base it is in.
fn pattern_address() -> Result<(usize, usize), String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok((cached, base));
        }
    }
    // Safety: a mapped module, for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the overview map's camera icon -- {why}"))?;
    // Safety: inside the matched pattern; a fresh module holds the game's own
    // operand, since the patch went into the one before.
    let operand = unsafe { *((found + TEAM_OPERAND_AT) as *const u32) } as usize;
    if operand == icon_team_address() {
        return Err("the camera icon's team test already reads DoD Studio's team".to_string());
    }
    GAME_TEAM_ADDRESS.store(operand, Ordering::Release);
    ADDRESS.store(found, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok((found, base))
}

fn icon_team_address() -> usize {
    ICON_TEAM.as_ptr() as usize
}

/// The team of the player the spectator camera follows, if it follows one
/// who is on Allies or Axis. `team_of` reads a player's `teamnumber` by
/// entity index; it is only asked for 1..=32.
fn watched_team(observer_mode: i32, target: i32, team_of: impl Fn(i32) -> i32) -> Option<i32> {
    // 1 chase locked, 2 chase free, 4 in-eye, 6 map chase. 3 (roaming) and
    // 5 (free overview) follow nobody, whatever `g_iUser2` still holds.
    if !matches!(observer_mode, 1 | 2 | 4 | 6) || !(1..=32).contains(&target) {
        return None;
    }
    let team = team_of(target);
    matches!(team, ALLIES | AXIS).then_some(team)
}

/// [`watched_team`] from the live client.dll globals.
fn watched_team_now(base: usize) -> Option<i32> {
    // Safety: fixed `.data` offsets in a mapped client.dll, read only; the
    // index is bounded to 1..=32 before the array is read (it holds 65).
    let mode = unsafe { *((base + MODE_RVA) as *const i32) };
    let target = unsafe { *((base + TARGET_RVA) as *const i32) };
    watched_team(mode, target, |index| {
        let at = base + EXTRA_TEAM_RVA + EXTRA_STRIDE * index as usize;
        i32::from(unsafe { *(at as *const i16) })
    })
}

/// Points the `cmp`'s address operand at `want`, returning whether it was
/// written.
fn set_operand(address: usize, want: usize, game: usize) -> Result<bool, String> {
    // Safety: the scan proved these bytes are mapped code.
    let present = unsafe { *(address as *const u32) } as usize;
    if present == want {
        return Ok(false);
    }
    if present != game && present != icon_team_address() {
        return Err(format!(
            "the camera icon's team test reads {present:#x}, neither g_iTeamNumber nor DoD Studio's -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &(want as u32).to_le_bytes()) } {
        return Err("could not make the camera icon's team test writable".to_string());
    }
    Ok(true)
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

/// Applies one setting, and at `1` the watched player's team, returning
/// whether the setting was newly applied (worth a log line; the sprite
/// choice following the camera on and off a player is not). Decides from
/// the bytes, so it is cheap to call every frame.
fn set_mode(mode: i32) -> Result<bool, String> {
    let (found, base) = pattern_address()?;
    let game_team = GAME_TEAM_ADDRESS.load(Ordering::Acquire);
    let watched = if mode == TEAM {
        let watched = watched_team_now(base);
        // Safety: `g_iTeamNumber`, the address the game's own `cmp` reads.
        let own = unsafe { *(game_team as *const i32) };
        ICON_TEAM.store(watched.unwrap_or(own), Ordering::Release);
        watched
    } else {
        None
    };
    let pointed = set_operand(
        found + TEAM_OPERAND_AT,
        if mode == TEAM {
            icon_team_address()
        } else {
            game_team
        },
        game_team,
    )?;
    // HLTV takes the team branch only while someone is watched; otherwise
    // its own spec_camera.spr.
    set_branch(
        found + JE_AT,
        watched.is_some(),
        "the camera icon's sprite choice",
    )?;
    let skipped = set_branch(found + SKIP_AT, mode == NONE, "the camera icon's draw")?;
    let before = MODE.swap(mode, Ordering::AcqRel);
    Ok(before != mode || pointed || skipped)
}

fn describe(mode: i32) -> &'static str {
    match mode {
        TEAM => {
            "1: in HLTV the watched player is marked with their team's camera icon (spec_camera.spr while nobody is watched)"
        }
        NONE => "2: no camera icon on the overview map",
        _ => "0: in HLTV the watched player is marked with spec_camera.spr",
    }
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let Some(raw) = CVAR.value() else {
        return;
    };
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
/// the setting, the watched player's team, and `g_iTeamNumber`.
pub fn status_line() -> Option<String> {
    let mode = MODE.load(Ordering::Relaxed);
    if mode == GAME {
        return None;
    }
    let base = engine::client_module_base();
    let watched = base.and_then(watched_team_now);
    // Safety: `g_iTeamNumber`, a fixed .data offset in client.dll.
    let own = base.map(|base| unsafe { *((base + 0xe8_8d0) as *const i32) });
    Some(format!(
        "overview map marker: {NAME} = {}; watched player's team = {watched:?}, g_iTeamNumber = {own:?}, icon team = {}",
        describe(mode),
        ICON_TEAM.load(Ordering::Relaxed)
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
    fn team_operand_at_indexes_the_cmp_address() {
        let t: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(
            &t[TEAM_OPERAND_AT - 2..TEAM_OPERAND_AT],
            &["83", "3D"],
            "cmp [imm32], imm8"
        );
        assert_eq!(
            &t[TEAM_OPERAND_AT..TEAM_OPERAND_AT + 4],
            &["??"; 4],
            "the address is wildcarded, so a patched one still matches"
        );
        assert_eq!(t[TEAM_OPERAND_AT + 4], "02", "compared with Axis");
    }

    #[test]
    fn the_watched_players_team_picks_the_icon() {
        let teams = |index: i32| match index {
            3 => AXIS,
            5 => ALLIES,
            7 => 3, // spectators
            _ => 0,
        };
        for mode in [1, 2, 4, 6] {
            assert_eq!(watched_team(mode, 3, teams), Some(AXIS), "mode {mode}");
            assert_eq!(watched_team(mode, 5, teams), Some(ALLIES), "mode {mode}");
        }
        // Roaming, the free overview and no spectating follow nobody.
        for mode in [0, 3, 5, 7] {
            assert_eq!(watched_team(mode, 3, teams), None, "mode {mode}");
        }
        // No target, a target past the player slots, or one on neither team.
        assert_eq!(watched_team(4, 0, teams), None);
        assert_eq!(watched_team(4, 33, |_| AXIS), None);
        assert_eq!(watched_team(4, -1, |_| AXIS), None);
        assert_eq!(watched_team(4, 7, teams), None);
        assert_eq!(watched_team(4, 9, teams), None);
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
