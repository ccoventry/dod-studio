//! `dodstudio_hud_map_players`: which players get an icon on DoD's overview
//! map (issue #304).
//!
//! ## What the game does
//!
//! `CHudDoDMap::DrawOverviewEntities` walks `CHudSpectator`'s overview list
//! and draws each entry's sprite. Players reach that list from
//! `AddOverviewEntity` (`client+0x39e20`, called at `+0x30c9a`) whatever
//! their team, so the positions are all there -- an HLTV stream carries
//! everybody. One test at draw time throws the other side away
//! (`whamemer/dod13-client`, `cl_dll/dod_map.cpp`):
//!
//! ```cpp
//! if( ent->player && ent->curstate.team != g_iTeamNumber )
//!     continue;
//! ```
//!
//! In `client.dll`, identical in the stock, pre- and post-Anniversary
//! installs:
//!
//! ```text
//!     +0x2f02f  mov  esi, [edi]             ; the entry's cl_entity_t
//!     +0x2f031  mov  eax, [esi+4]           ; ent->player
//!     +0x2f034  test eax, eax
//!     +0x2f036  je   +0x2f050               ; not a player: always drawn
//!     +0x2f038  mov  ecx, [esi+0x354]       ; ent->curstate.team    <- detour
//!     +0x2f03e  mov  edx, [g_iTeamNumber]
//!     +0x2f044  cmp  ecx, edx
//!     +0x2f046  jne  +0x2f322               ; the next entry
//!     +0x2f04c  test eax, eax               ; <- the detour returns here
//! ```
//!
//! ## The detour
//!
//! The twenty bytes from `+0x2f038` become a jump to a stub that asks
//! [`keep_icon`] and then takes the game's own two exits: on to `+0x2f04c` to
//! draw it, or to the `jne`'s target to skip it. The stub puts `ecx` and `edx`
//! back to what the stolen instructions left in them, and keeps `eax`
//! (`ent->player`), which `+0x2f04c` tests. Nothing in the image branches into
//! the stolen span (checked over the whole `.text`).
//!
//! The detour is installed the first time the cvar leaves 0, and stays: with
//! the cvar back at 0, [`keep_icon`] makes the game's own test. A detour cannot
//! be taken out safely while the game may be running the stub.
//!
//! ## What a POV demo can show
//!
//! A POV recording holds only the players the recorder's client was sent, so
//! enemies come and go with what the recorder could see. HLTV demos, and live
//! HLTV, carry everybody.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("hud_map_players");

/// From `mov esi, [edi]` to the `jne` after the team test, and the first
/// instruction after it, so the span's end is checked too.
const PATTERN: &str = "8B 37 8B 46 04 85 C0 74 ?? 8B 8E 54 03 00 00 8B 15 ?? ?? ?? ?? \
                       3B CA 0F 85 ?? ?? ?? ?? 85 C0";
/// The detour's start in [`PATTERN`]: `mov ecx, [esi+0x354]`.
const SPAN_AT: usize = 9;
/// The stolen bytes: the two loads, the `cmp` and the `jne`.
const SPAN_LEN: usize = 20;
/// `g_iTeamNumber`'s address, the operand of `mov edx, [imm32]`.
const TEAM_AT: usize = 17;
/// The `jne rel32`'s opcode.
const JNE_AT: usize = 23;

/// `cl_entity_t.curstate.team`.
const TEAM_OFFSET: usize = 0x354;

/// `g_iUser1` and `g_iUser2`: the spectator mode, and the player being
/// watched. See `spectator_target.rs`.
const MODE_RVA: usize = 0xe8_8d4;
const TARGET_RVA: usize = 0xe8_8d8;

pub const GAME: i32 = 0;
pub const EVERYONE: i32 = 1;
pub const ENEMIES: i32 = 2;
pub const WATCHED: i32 = 3;
pub const NOBODY: i32 = 4;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The setting [`keep_icon`] follows.
static MODE: AtomicI32 = AtomicI32::new(GAME);
/// The `client.dll` base the detour went into, 0 before.
static INSTALLED_BASE: AtomicUsize = AtomicUsize::new(0);
/// `g_iTeamNumber`'s address, read out of the instruction.
static TEAM_ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// Whether a failure to install has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

/// Called once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// What one setting shows.
pub fn describe(mode: i32) -> &'static str {
    match mode {
        EVERYONE => "everyone",
        ENEMIES => "only the other team",
        WATCHED => "only the player being watched",
        NOBODY => "nobody",
        _ => "the game's own: the team of the player being watched",
    }
}

/// The player whose icon [`WATCHED`] keeps: the one the spectator camera is
/// on, or in a POV demo the recorder.
fn watched_player(base: usize) -> i32 {
    // Safety: fixed `.data` offsets in a mapped client.dll, read only.
    let mode = unsafe { *((base + MODE_RVA) as *const i32) };
    if mode != 0 {
        return unsafe { *((base + TARGET_RVA) as *const i32) };
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return 0;
    };
    // Safety: the game's own map code calls this every frame it draws; the
    // result is the local player's `cl_entity_t`, `index` first.
    let local = unsafe { (engfuncs.get_local_player)() } as *const i32;
    if local.is_null() {
        0
    } else {
        unsafe { *local }
    }
}

/// Whether one player's icon is drawn. Called by the stub, for players only,
/// with the game's map code waiting on the answer.
extern "C" fn keep_icon(entity: *const c_void) -> u32 {
    let entity = entity as *const u8;
    let team_address = TEAM_ADDRESS.load(Ordering::Relaxed);
    if entity.is_null() || team_address == 0 {
        return 1;
    }
    // Safety: `entity` is the `cl_entity_t` the game is about to draw, and
    // `team_address` was read out of the game's own instruction.
    let team = unsafe { *(entity.add(TEAM_OFFSET) as *const i32) };
    let ours = unsafe { *(team_address as *const i32) };
    let keep = match MODE.load(Ordering::Relaxed) {
        EVERYONE => true,
        ENEMIES => team != ours,
        WATCHED => match engine::client_module_base() {
            Some(base) => {
                let index = unsafe { *(entity as *const i32) };
                index != 0 && index == watched_player(base)
            }
            None => false,
        },
        NOBODY => false,
        _ => team == ours,
    };
    keep as u32
}

/// The stub: ask [`keep_icon`], then leave by the game's own exits.
fn stub_code(team_address: usize, draw: usize, skip: usize) -> Vec<u8> {
    let mut code = Vec::new();
    code.push(0x50); // push eax          (ent->player, tested after the span)
    code.push(0x56); // push esi          (the entity)
    code.push(0xB8); // mov  eax, keep_icon
    code.extend_from_slice(
        &(keep_icon as extern "C" fn(*const c_void) -> u32 as usize as u32).to_le_bytes(),
    );
    code.extend_from_slice(&[0xFF, 0xD0]); // call eax
    code.extend_from_slice(&[0x83, 0xC4, 0x04]); // add esp, 4
    code.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
    code.push(0x58); // pop  eax          (flags untouched from here on)
    code.extend_from_slice(&[0x8B, 0x8E, 0x54, 0x03, 0x00, 0x00]); // mov ecx, [esi+0x354]
    code.extend_from_slice(&[0x8B, 0x15]); // mov edx, [g_iTeamNumber]
    code.extend_from_slice(&(team_address as u32).to_le_bytes());
    code.extend_from_slice(&[0x75, 0x06]); // jnz draw
    code.push(0x68); // push skip; ret
    code.extend_from_slice(&(skip as u32).to_le_bytes());
    code.push(0xC3);
    code.push(0x68); // draw: push draw; ret
    code.extend_from_slice(&(draw as u32).to_le_bytes());
    code.push(0xC3);
    code
}

/// Puts the detour into the loaded `client.dll`, once per module.
fn install() -> Result<bool, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };
    if INSTALLED_BASE.load(Ordering::Acquire) == base {
        return Ok(false);
    }
    // Safety: a mapped module; the scan checks every byte the stub relies on.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the overview map's team test -- {why}"))?;
    let team_address = unsafe { *((found + TEAM_AT) as *const u32) } as usize;
    let jne = found + JNE_AT;
    let rel = unsafe { *((jne + 2) as *const i32) };
    let skip = (jne + 6).wrapping_add(rel as usize);
    let span = found + SPAN_AT;
    let draw = span + SPAN_LEN;

    TEAM_ADDRESS.store(team_address, Ordering::Release);
    let code = stub_code(team_address, draw, skip);
    // Safety: the pattern pins the span to whole instructions, and nothing
    // branches into it (module doc).
    let detour = unsafe { crate::detour::install(span, SPAN_LEN, &code) }?;
    unsafe {
        crate::debug::report(&format!(
            "overview_players: team test at client+{:#x} now asks {NAME} (stub at {:#x})",
            span - base,
            detour.stub_address()
        ))
    };
    // The handle going out of scope leaves the stub in place for the
    // process; see `detour::Detour`.
    INSTALLED_BASE.store(base, Ordering::Release);
    Ok(true)
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let raw = unsafe { (*cvar).value };
    let wanted = if raw.is_finite() { raw as i32 } else { GAME };
    let wanted = if (GAME..=NOBODY).contains(&wanted) {
        wanted
    } else {
        GAME
    };
    let before = MODE.swap(wanted, Ordering::Relaxed);
    if wanted == GAME && INSTALLED_BASE.load(Ordering::Relaxed) == 0 {
        return;
    }
    match install() {
        Ok(_) => COMPLAINED.store(false, Ordering::Relaxed),
        Err(why) => {
            if !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "overview_players: {NAME} not applied yet -- {why}"
                    ))
                };
            }
            return;
        }
    }
    if before != wanted {
        unsafe {
            crate::debug::report(&format!(
                "overview_players: {NAME} = {wanted} ({})",
                describe(wanted)
            ))
        };
    }
}

/// One line for `dodstudio_debug_status`, once the setting has left 0.
pub fn status_line() -> Option<String> {
    let mode = MODE.load(Ordering::Relaxed);
    (INSTALLED_BASE.load(Ordering::Relaxed) != 0 || mode != GAME)
        .then(|| format!("overview map icons: {NAME} = {mode} ({})", describe(mode)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Vec<&'static str> {
        PATTERN.split_whitespace().collect()
    }

    #[test]
    fn the_offsets_index_the_instructions_they_name() {
        let t = tokens();
        assert_eq!(
            &t[SPAN_AT..SPAN_AT + 2],
            &["8B", "8E"],
            "mov ecx, [esi+...]"
        );
        assert_eq!(&t[SPAN_AT + 2..SPAN_AT + 4], &["54", "03"], "curstate.team");
        assert_eq!(&t[TEAM_AT - 2..TEAM_AT], &["8B", "15"], "mov edx, [imm32]");
        assert_eq!(&t[JNE_AT..JNE_AT + 2], &["0F", "85"], "jne rel32");
        assert_eq!(
            &t[SPAN_AT + SPAN_LEN..SPAN_AT + SPAN_LEN + 2],
            &["85", "C0"],
            "the span ends where `test eax, eax` starts"
        );
        assert_eq!(
            JNE_AT + 6,
            SPAN_AT + SPAN_LEN,
            "the jne is the span's last instruction"
        );
    }

    #[test]
    fn the_stub_restores_what_the_span_left_and_leaves_twice() {
        let code = stub_code(0x1111_1111, 0x2222_2222, 0x3333_3333);
        // ecx and edx as the stolen loads left them.
        assert!(
            code.windows(6)
                .any(|w| w == [0x8B, 0x8E, 0x54, 0x03, 0x00, 0x00])
        );
        assert!(
            code.windows(6)
                .any(|w| w == [0x8B, 0x15, 0x11, 0x11, 0x11, 0x11])
        );
        // `jnz` hops exactly the skip exit (`push imm32; ret`).
        let jnz = code.windows(2).position(|w| w == [0x75, 0x06]).unwrap();
        assert_eq!(
            &code[jnz + 2..jnz + 8],
            &[0x68, 0x33, 0x33, 0x33, 0x33, 0xC3]
        );
        assert_eq!(&code[jnz + 8..], &[0x68, 0x22, 0x22, 0x22, 0x22, 0xC3]);
        // Pushes and pops balance: eax and esi in, the argument and eax out.
        assert_eq!(&code[..2], &[0x50, 0x56]);
    }

    #[test]
    fn every_setting_has_words() {
        for mode in GAME..=NOBODY {
            assert!(!describe(mode).is_empty());
        }
        assert_eq!(
            describe(GAME),
            describe(99),
            "out of range reads as the game's own"
        );
    }
}
