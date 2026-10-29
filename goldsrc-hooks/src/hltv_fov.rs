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
    let end = SLOTS * 4 + TABLE_OFFSET;
    if ghud < base || ghud + end > base + image_size {
        return Err(format!(
            "the HLTV message thunk pushes {ghud:#x}, outside client.dll"
        ));
    }
    Ok(ghud + TABLE_OFFSET)
}

/// Sets every player's HLTV zoom back to "none known", as when the game
/// starts. Called on the engine's thread (a console command, or `poll`).
pub fn clear() -> Result<(), String> {
    let base =
        engine::client_module_base().ok_or_else(|| "client.dll is not loaded".to_string())?;
    // Safety: `base` is client.dll's mapped image; every read is inside the
    // module (its own code, at the analysed build's RVAs), and the table is
    // written only once its address checks out, inside the image.
    unsafe {
        let image_size = crate::pe::image_size(base as *mut u8)
            .ok_or_else(|| "client.dll has no readable PE header".to_string())?;
        if HANDLER_RVA.max(READ_RVA) + 16 > image_size {
            return Err("client.dll is smaller than the analysed build".into());
        }
        let bytes =
            |rva: usize, len: usize| std::slice::from_raw_parts((base + rva) as *const u8, len);
        let table = table_address(
            base,
            image_size,
            bytes(THUNK_RVA, CALL_END),
            bytes(STORE_RVA, STORE_BYTES.len()) == STORE_BYTES,
            bytes(READ_RVA, READ_BYTES.len()) == READ_BYTES,
        )?;
        std::ptr::write_bytes(table as *mut i32, 0, SLOTS);
    }
    Ok(())
}

/// [`clear`], with any refusal written to the debug log.
pub fn clear_logged(why: &str) {
    if let Err(e) = clear() {
        unsafe { crate::debug::report(&format!("hltv_fov: could not clear ({why}): {e}")) };
    }
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
