//! The zoom an HLTV demo shows each player with, and putting it back (#830).
//!
//! ## Where an HLTV demo's zoom lives
//!
//! A POV demo records the recorder's FOV in every frame. An HLTV demo has no
//! such payload: the server sends an `HLTV` user message, `[player, value]`,
//! each time a player's zoom changes (a sniper sends 20 as he scopes, 0 as he
//! unscopes). `CHud::MsgFunc_HLTV` (`client+0x22270`, reached through the
//! `__MsgFunc_HLTV` thunk at `+0x20c50`) keeps the value per player in an
//! `int[64]` at `gHUD+0x70`, with 0 stored as 90, or fills all 64 when
//! `player` is 0. Bit 7 set means health instead, kept elsewhere.
//! `CHud::Think` (`+0x36d8e`) reads `table[g_iUser2]` while in-eye, and a 0
//! entry means 90: no zoom known.
//!
//! Nothing else in `client.dll` writes the table: no new map, `ResetHUD`,
//! `InitHUD` or new demo clears it, and `client.dll` stays loaded between
//! demos. So:
//!
//! - **A backward seek** sends only the landing frame, so the table keeps
//!   what the later position left in it: a player scoped in at the landing
//!   shows unzoomed when he had unscoped by the old position, and the other
//!   way round. `demo_seek.rs` rewinds such a seek to the start and steps
//!   forward, which replays every `HLTV` message in order; it clears the table
//!   first ([`clear`]), for players with no message before the landing.
//! - **A new demo** would start with the last demo's zooms until each player's
//!   first message. `demo_seek::demo_started` clears the table whenever a demo
//!   is loaded, before any of its messages arrive.
//!
//! Clearing writes the 64 slots back to 0, the state a fresh game starts in.
//! It is plain data in `client.dll`'s `.data`, so no page protection changes.
//!
//! ## Finding the table
//!
//! The thunk pushes `gHUD` as an absolute address (`push imm32`, relocated by
//! the loader) before calling the handler, so the table's address is read out
//! of the thunk's own bytes, after checking those bytes, the call's target,
//! and the handler's and reader's `+0x70` against the analysed build.
//! `client.dll` is byte-identical in the pre-Anniversary and 25th Anniversary
//! installs, so one set covers both; `tools/verify_hltv_fov_offsets.py`
//! checks them against both installs' DLLs.
//!
//! ## A POV demo played as HLTV leaves the HUD's FOV at 0 (#1462)
//!
//! `Think` turns the table into the HUD's FOV, `gHUD.m_iFOV` (`gHUD+0x234`):
//! in-eye the watched player's zoom, otherwise 90. DoD's `StudioDrawModel`
//! skips the viewmodel while that FOV is under 90 (`client+0x49ed5`, the
//! scoped-in case). `Think` runs from `HUD_UpdateClientData`, every frame of
//! an HLTV demo, but not for a POV demo played with `dem_forcehltv 1`, where
//! the recorder's own HUD messages still arrive: `ResetHUD` at his spawn and
//! `YouDied` set the FOV to 0, `SetFOV` to his zoom. Measured on `tr2.dem`
//! (PRE): the FOV sat at 0 from the recorder's spawn to his death and at 90
//! from his `SetFOV 90` to his next spawn, and the watched player's gun was
//! drawn exactly in the 90 stretches, whatever `ds_spec_match_pov` said.
//! [`settle_hud_fov`] does `Think`'s job before the view is drawn.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine;

/// `__MsgFunc_HLTV`: forwards its three arguments with `gHUD` pushed as
/// `this`, then calls `CHud::MsgFunc_HLTV`.
pub const THUNK_RVA: usize = 0x2_0c50;
/// `mov eax/ecx/edx, [esp+..]; push eax; push ecx; push edx; push gHUD`;
/// then `call MsgFunc_HLTV` ([`CALL`] at [`CALL_AT`]). The two absolute
/// operands are read, not compared.
const THUNK_PREFIX: [u8; 16] = [
    0x8b, 0x44, 0x24, 0x0c, 0x8b, 0x4c, 0x24, 0x08, 0x8b, 0x54, 0x24, 0x04, 0x50, 0x51, 0x52, 0x68,
];
const CALL: u8 = 0xe8;
const CALL_AT: usize = 20;
/// Where `gHUD`'s imm32 sits in the thunk.
const GHUD_AT: usize = 16;
/// The call's rel32, and the end of the call it is measured from.
const CALL_REL_AT: usize = 21;
const CALL_END: usize = 25;
/// `CHud::MsgFunc_HLTV`.
pub const HANDLER_RVA: usize = 0x2_2270;
/// The handler's one-player store, `mov eax, [esp+0xc]; mov [eax+esi*4+0x70],
/// ebx`, and `CHud::Think`'s read, `mov eax, [esi+eax*4+0x70]`.
pub const STORE_RVA: usize = 0x2_22f7;
pub const STORE_BYTES: [u8; 8] = [0x8b, 0x44, 0x24, 0x0c, 0x89, 0x5c, 0xb0, 0x70];
pub const READ_RVA: usize = 0x3_6d8e;
pub const READ_BYTES: [u8; 4] = [0x8b, 0x44, 0x86, 0x70];
/// The table's offset in `CHud`, and its length (the handler's `rep stosd`
/// count).
pub const TABLE_OFFSET: usize = 0x70;
pub const SLOTS: usize = 64;
/// `gHUD.m_iFOV`'s offset in `CHud`, and `Think`'s in-eye store of the
/// table's value there, `mov [esi+0x234], eax`.
pub const HUD_FOV_OFFSET: usize = 0x234;
pub const FOV_STORE_RVA: usize = 0x3_6d96;
pub const FOV_STORE_BYTES: [u8; 6] = [0x89, 0x86, 0x34, 0x02, 0x00, 0x00];
/// `Think`'s FOV for no zoom known.
const NO_ZOOM: i32 = 90;
/// The FOV `CHudDoDCommon::Draw` sets while a viewport panel is open.
const MENU_FOV: i32 = 115;

/// The table's address, from the thunk's bytes as loaded at `base`, or why
/// they are not the analysed build's. `handler_ok` and `read_ok` say whether
/// the handler's store and `Think`'s read hold their stock bytes, and
/// `image_size` bounds where `gHUD` may be.
fn table_address(
    base: usize,
    image_size: usize,
    thunk: &[u8],
    handler_ok: bool,
    read_ok: bool,
) -> Result<usize, String> {
    let matches = thunk.len() >= CALL_END && thunk[..16] == THUNK_PREFIX && thunk[CALL_AT] == CALL;
    if !matches {
        return Err(format!(
            "the HLTV message thunk at +{THUNK_RVA:#x} is not the analysed build's"
        ));
    }
    let word =
        |at: usize| u32::from_le_bytes([thunk[at], thunk[at + 1], thunk[at + 2], thunk[at + 3]]);
    let rel = word(CALL_REL_AT) as i32;
    let target = (base + THUNK_RVA + CALL_END).wrapping_add_signed(rel as isize);
    if target != base + HANDLER_RVA {
        return Err(format!(
            "the HLTV message thunk calls +{:#x}, not +{HANDLER_RVA:#x}",
            target.wrapping_sub(base)
        ));
    }
    if !handler_ok || !read_ok {
        return Err(format!(
            "the HLTV zoom table's store (+{STORE_RVA:#x}) or read (+{READ_RVA:#x}) is not the analysed build's"
        ));
    }
    let ghud = word(GHUD_AT) as usize;
    // Everything read or written through `gHUD`: the table, and the HUD's
    // FOV past it.
    let end = (SLOTS * 4 + TABLE_OFFSET).max(HUD_FOV_OFFSET + 4);
    if ghud < base || ghud + end > base + image_size {
        return Err(format!(
            "the HLTV message thunk pushes {ghud:#x}, outside client.dll"
        ));
    }
    Ok(ghud + TABLE_OFFSET)
}

/// The table's address in the loaded `client.dll`, once the thunk, the
/// handler's store and `Think`'s read check out; with `fov_store`, also
/// `Think`'s spectator FOV store ([`FOV_STORE_RVA`]).
fn located_table(fov_store: bool) -> Result<usize, String> {
    let base =
        engine::client_module_base().ok_or_else(|| "client.dll is not loaded".to_string())?;
    // Safety: `base` is client.dll's mapped image, and every read is inside
    // the module (its own code, at the analysed build's RVAs).
    unsafe {
        let image_size = crate::pe::image_size(base as *mut u8)
            .ok_or_else(|| "client.dll has no readable PE header".to_string())?;
        if HANDLER_RVA.max(READ_RVA).max(FOV_STORE_RVA) + 16 > image_size {
            return Err("client.dll is smaller than the analysed build".into());
        }
        let bytes =
            |rva: usize, len: usize| std::slice::from_raw_parts((base + rva) as *const u8, len);
        if fov_store && bytes(FOV_STORE_RVA, FOV_STORE_BYTES.len()) != FOV_STORE_BYTES {
            return Err(format!(
                "CHud::Think's FOV store (+{FOV_STORE_RVA:#x}) is not the analysed build's"
            ));
        }
        table_address(
            base,
            image_size,
            bytes(THUNK_RVA, CALL_END),
            bytes(STORE_RVA, STORE_BYTES.len()) == STORE_BYTES,
            bytes(READ_RVA, READ_BYTES.len()) == READ_BYTES,
        )
    }
}

/// Sets every player's HLTV zoom back to "none known", as when the game
/// starts. Called on the engine's thread (a console command, or `poll`).
pub fn clear() -> Result<(), String> {
    let table = located_table(false)?;
    // Safety: `located_table` checked the address lies inside client.dll's
    // image, where the table is plain `.data`.
    unsafe { std::ptr::write_bytes(table as *mut i32, 0, SLOTS) };
    Ok(())
}

/// [`clear`], with any refusal written to the debug log.
pub fn clear_logged(why: &str) {
    if let Err(e) = clear() {
        unsafe { crate::debug::report(&format!("hltv_fov: could not clear ({why}): {e}")) };
    }
}

/// The FOV `Think` gives a spectator: the watched player's zoom from the
/// table while in his eyes (`in_eye_target`, his entity index), else 90.
/// `zoom_of` reads one table slot.
fn spectator_fov(in_eye_target: Option<i32>, zoom_of: impl Fn(usize) -> i32) -> i32 {
    in_eye_target
        .and_then(|target| usize::try_from(target).ok())
        .filter(|slot| (1..SLOTS).contains(slot))
        .map(zoom_of)
        .filter(|&zoom| zoom != 0)
        .unwrap_or(NO_ZOOM)
}

/// Whether the HUD's FOV `current` is one only a message to the recorder can
/// have left, which `Think` would have replaced with `spectator`. DoD's own
/// menu FOV (`CHudDoDCommon::Draw`, while a viewport panel is open) is left
/// alone: `Think` meets it in an HLTV demo too.
fn needs_settling(current: i32, spectator: i32) -> bool {
    current != spectator && current != MENU_FOV
}

/// Where `gHUD.m_iFOV` is ([`located_table`]), and the `client.dll` base it
/// was found at: 0 until found, found again if the base ever changes.
/// [`HUD_FOV_REFUSED`] stops the search after a refusal.
static HUD_FOV_AT: AtomicUsize = AtomicUsize::new(0);
static HUD_FOV_BASE: AtomicUsize = AtomicUsize::new(0);
static HUD_FOV_REFUSED: AtomicBool = AtomicBool::new(false);
/// How many times [`settle_hud_fov`] has put the FOV right this session.
static SETTLED: AtomicU32 = AtomicU32::new(0);
/// Lines [`settle_hud_fov`] writes to the log before it only counts.
const SETTLED_LOG_LINES: u32 = 8;

/// `gHUD.m_iFOV`'s address, or `None` when this isn't the analysed
/// `client.dll` (logged once).
fn hud_fov_address() -> Option<usize> {
    let base = engine::client_module_base()?;
    let known = HUD_FOV_AT.load(Ordering::Relaxed);
    if known != 0 && HUD_FOV_BASE.load(Ordering::Relaxed) == base {
        return Some(known);
    }
    if HUD_FOV_REFUSED.load(Ordering::Relaxed) {
        return None;
    }
    match located_table(true) {
        Ok(table) => {
            let at = table - TABLE_OFFSET + HUD_FOV_OFFSET;
            HUD_FOV_AT.store(at, Ordering::Relaxed);
            HUD_FOV_BASE.store(base, Ordering::Relaxed);
            Some(at)
        }
        Err(why) => {
            HUD_FOV_REFUSED.store(true, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "hltv_fov: the in-eye HUD FOV fix is off this session: {why}"
                ))
            };
            None
        }
    }
}

/// Puts the HUD's FOV back to what `CHud::Think` gives a spectator, when a
/// demo played as HLTV leaves it at a value `Think` would have replaced
/// (#1462). Runs on the engine's thread once a frame, before the view is
/// drawn; writes `gHUD.m_iFOV` and nothing else.
///
/// `Think` runs from `HUD_UpdateClientData`, which the engine does not call
/// for a POV demo played with `dem_forcehltv 1`, while the recorder's own
/// `ResetHUD`, `YouDied` and `SetFOV` messages still arrive and set the FOV to
/// 0 or his zoom. DoD's `StudioDrawModel` (`client+0x49ed5`) skips the
/// viewmodel while the FOV is under 90, so from the recorder's spawn to his
/// death no gun is drawn in anyone's eyes. In an HLTV demo `Think` writes
/// the same value every frame and this finds nothing to do.
pub fn settle_hud_fov() {
    let spectating = engine::engfuncs().is_some_and(|e| unsafe { (e.is_spectate_only)() } != 0);
    if !spectating {
        return;
    }
    let Some(fov_at) = hud_fov_address() else {
        return;
    };
    let table = fov_at - HUD_FOV_OFFSET + TABLE_OFFSET;
    // Safety: both inside client.dll's image (`table_address`'s bounds), plain
    // `.data` the engine's thread owns; this runs on that thread.
    unsafe {
        let spectator = spectator_fov(crate::spectator_target::in_eye_target(), |slot| {
            *(table as *const i32).add(slot)
        });
        let current = *(fov_at as *const i32);
        if !needs_settling(current, spectator) {
            return;
        }
        *(fov_at as *mut i32) = spectator;
        let settled = SETTLED.fetch_add(1, Ordering::Relaxed) + 1;
        if settled <= SETTLED_LOG_LINES {
            crate::debug::report(&format!(
                "hltv_fov: the HUD's FOV was {current} with no CHud::Think to reset it (a POV demo played as HLTV); set to {spectator}, so the in-eye gun draws ({settled} so far{})",
                if settled == SETTLED_LOG_LINES {
                    ", counting quietly from here"
                } else {
                    ""
                }
            ));
        }
    }
}

/// One line for `ds_debug_status`.
pub fn status() -> String {
    format!(
        "the HUD's FOV put back for a POV demo played as HLTV {} time(s) this session",
        SETTLED.load(Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: usize = 0x1900_0000;
    const SIZE: usize = 0x18_0000;

    /// The analysed build's thunk, as loaded at `BASE`: `gHUD` at `+0x1080b0`.
    fn thunk() -> Vec<u8> {
        let mut t = THUNK_PREFIX.to_vec();
        t.extend_from_slice(&((BASE + 0x10_80b0) as u32).to_le_bytes());
        t.push(CALL);
        let rel = (HANDLER_RVA as i64 - (THUNK_RVA + CALL_END) as i64) as i32;
        t.extend_from_slice(&rel.to_le_bytes());
        t
    }

    #[test]
    fn the_table_is_ghud_plus_0x70() {
        assert_eq!(
            table_address(BASE, SIZE, &thunk(), true, true),
            Ok(BASE + 0x10_8120)
        );
        // The bytes the analysis read: `e8 07 16 00 00`.
        assert_eq!(&thunk()[CALL_REL_AT..CALL_END], &[0x07, 0x16, 0x00, 0x00]);
    }

    /// `Think`'s spectator rule: the watched player's zoom in his eyes, 90
    /// for no zoom known, out of his eyes, or a slot past the table.
    #[test]
    fn the_spectator_fov_is_thinks() {
        let table = |slot: usize| if slot == 7 { 20 } else { 0 };
        assert_eq!(spectator_fov(Some(7), table), 20);
        assert_eq!(spectator_fov(Some(3), table), NO_ZOOM);
        assert_eq!(spectator_fov(None, table), NO_ZOOM);
        assert_eq!(spectator_fov(Some(0), |_| 20), NO_ZOOM);
        assert_eq!(spectator_fov(Some(-1), |_| 20), NO_ZOOM);
        assert_eq!(spectator_fov(Some(SLOTS as i32), |_| 20), NO_ZOOM);
    }

    /// The recorder's `ResetHUD`/`YouDied` 0 and his `SetFOV` zoom are put
    /// right; `Think`'s own value and DoD's menu FOV are not touched.
    #[test]
    fn only_a_value_think_would_replace_is_settled() {
        assert!(needs_settling(0, 90));
        assert!(needs_settling(20, 90));
        assert!(needs_settling(0, 20));
        assert!(!needs_settling(90, 90));
        assert!(!needs_settling(20, 20));
        assert!(!needs_settling(MENU_FOV, 90));
    }

    /// The HUD's FOV sits past the table, so `gHUD` must leave room for it.
    #[test]
    fn the_hud_fov_must_be_inside_the_image() {
        let ghud_end = 0x10_80b0 + HUD_FOV_OFFSET + 4;
        assert!(table_address(BASE, ghud_end, &thunk(), true, true).is_ok());
        assert!(table_address(BASE, ghud_end - 1, &thunk(), true, true).is_err());
    }

    #[test]
    fn another_build_is_refused() {
        let mut changed = thunk();
        changed[0] = 0x55;
        assert!(table_address(BASE, SIZE, &changed, true, true).is_err());
        let mut elsewhere = thunk();
        elsewhere[CALL_REL_AT] = 0x08;
        assert!(table_address(BASE, SIZE, &elsewhere, true, true).is_err());
        assert!(table_address(BASE, SIZE, &thunk(), false, true).is_err());
        assert!(table_address(BASE, SIZE, &thunk(), true, false).is_err());
        assert!(table_address(BASE, SIZE, &thunk()[..20], true, true).is_err());
        // gHUD outside the image.
        assert!(table_address(BASE, 0x10_8000, &thunk(), true, true).is_err());
    }
}
