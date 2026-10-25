//! The kill feed's 31-character name cap, lifted to 63 for
//! `ds_player_change_name` (#1021).
//!
//! `DeathNoticeItem`'s `szKiller` and `szVictim` are 64 bytes each, but
//! `MsgFunc_DeathMsg` copies a name into them with `strncpy(dst, name, 32)`
//! and then writes a terminator at `[31]`:
//!
//! ```text
//! +0x2b2c2  6a 20                 push 0x20                 ; szKiller
//! +0x2b2ca  e8 ..                 call strncpy
//! +0x2b2d2  c6 86 <&[0].szKiller[31]> 00   mov byte ptr [esi + ...], 0
//! +0x2b2fc  6a 20                 push 0x20                 ; szVictim
//! +0x2b306  e8 ..                 call strncpy
//! +0x2b30e  c6 86 <&[0].szVictim[31]> 00   mov byte ptr [esi + ...], 0
//! ```
//!
//! So the engine's 31-character names always fit, and a longer one from the
//! rename detour was cut. The lift makes both lengths 0x40 and moves both
//! terminators to `[63]`, inside the same 64-byte fields. Names the engine
//! hands out are at most 31 bytes, so nothing changes for them.
//!
//! The terminators are two of `deathmsg`'s [`super::ARRAY_REFS`]: `max`
//! rewrites them for its relocated array, so `apply_max` asks
//! [`field_for`] which field to point them at. The stock check runs before
//! the lift ([`super::lift_name_cap`]), so it never sees the lifted bytes.
//! `tools/verify_killfeed_name_cap_offsets.py` checks every site against the
//! binary (byte-identical on both installs).

use std::sync::atomic::{AtomicUsize, Ordering};

/// `push 0x20` before each `strncpy`: the RVA of the imm8.
const LEN_SITES: [usize; 2] = [0x2_b2c3, 0x2_b2fd];
/// `mov byte ptr [esi + disp32], 0` after each copy: the RVA of the disp32,
/// and the field it points at in item 0 as shipped.
const TERMINATOR_SITES: [(usize, usize); 2] = [(0x2_b2d4, 0x1f), (0x2_b310, 0x5f)];
const PUSH_IMM8: u8 = 0x6a;
/// `c6 86`: `mov byte ptr [esi + disp32], imm8`.
const MOV_BYTE_ESI_DISP32: [u8; 2] = [0xc6, 0x86];

const STOCK_LEN: u8 = 0x20;
const LIFTED_LEN: u8 = 0x40;
/// How far each terminator moves: `[31]` to `[63]`.
const SHIFT: usize = (LIFTED_LEN - STOCK_LEN) as usize;

/// The client.dll base the cap is lifted for, or 0.
static LIFTED_AT: AtomicUsize = AtomicUsize::new(0);

pub(super) fn lifted(base: usize) -> bool {
    base != 0 && LIFTED_AT.load(Ordering::Acquire) == base
}

/// The field `apply_max` points the reference at `rva` to: the terminators
/// one name further on while the cap is lifted.
pub(super) fn field_for(base: usize, rva: usize, field: usize) -> usize {
    if lifted(base) && TERMINATOR_SITES.iter().any(|&(at, _)| at == rva) {
        field + SHIFT
    } else {
        field
    }
}

/// Lifts the cap in the module at `base`, whose death-notice array is at
/// `array` (client.dll's own or `max`'s relocated one).
///
/// # Safety
/// `base` is client.dll's loaded base, and the stock check has passed for it.
pub(super) unsafe fn lift(base: usize, array: usize) -> Result<(), String> {
    if lifted(base) {
        return Ok(());
    }
    for rva in LEN_SITES {
        // Safety: code offsets inside the loaded module.
        let (op, len) = unsafe {
            (
                *((base + rva - 1) as *const u8),
                *((base + rva) as *const u8),
            )
        };
        if op != PUSH_IMM8 || len != STOCK_LEN {
            return Err(format!(
                "+{:#x} is not `push 0x20` ({op:02x} {len:02x})",
                rva - 1
            ));
        }
    }
    for (rva, field) in TERMINATOR_SITES {
        let (op, disp, imm) = unsafe {
            (
                std::slice::from_raw_parts((base + rva - 2) as *const u8, 2),
                ((base + rva) as *const u32).read_unaligned(),
                *((base + rva + 4) as *const u8),
            )
        };
        let want = (array + field) as u32;
        if op != MOV_BYTE_ESI_DISP32 || disp != want || imm != 0 {
            return Err(format!(
                "+{:#x} is not the terminator write expected (disp {disp:#x}, want {want:#x})",
                rva - 2
            ));
        }
    }
    for (rva, field) in TERMINATOR_SITES {
        let value = (array + field + SHIFT) as u32;
        if !unsafe { crate::patch::write_code_bytes(base + rva, &value.to_le_bytes()) } {
            return Err(format!("could not make +{rva:#x} writable"));
        }
    }
    for rva in LEN_SITES {
        if !unsafe { crate::patch::write_code_bytes(base + rva, &[LIFTED_LEN]) } {
            return Err(format!("could not make +{rva:#x} writable"));
        }
    }
    LIFTED_AT.store(base, Ordering::Release);
    unsafe {
        crate::debug::report(
            "deathmsg: the kill feed now copies names of up to 63 characters (was 31, #1021)",
        )
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminators_are_deathmsg_array_references() {
        for (rva, field) in TERMINATOR_SITES {
            assert!(
                super::super::ARRAY_REFS.contains(&(rva, field)),
                "+{rva:#x} field {field:#x} is not in ARRAY_REFS"
            );
        }
    }

    #[test]
    fn the_lifted_terminators_stay_inside_their_64_byte_names() {
        // szKiller is 0x00..0x40, szVictim 0x40..0x80.
        assert_eq!(TERMINATOR_SITES[0].1 + SHIFT, 0x3f);
        assert_eq!(TERMINATOR_SITES[1].1 + SHIFT, 0x7f);
        assert_eq!(usize::from(LIFTED_LEN), 0x40);
    }

    #[test]
    fn only_the_terminators_move_and_only_once_lifted() {
        assert_eq!(field_for(0x1900000, 0x2_b2d4, 0x1f), 0x1f, "not lifted");
        LIFTED_AT.store(0x1234_0000, Ordering::Release);
        assert_eq!(field_for(0x1234_0000, 0x2_b2d4, 0x1f), 0x3f);
        assert_eq!(field_for(0x1234_0000, 0x2_b310, 0x5f), 0x7f);
        assert_eq!(
            field_for(0x1234_0000, 0x2_b2e7, 0x40),
            0x40,
            "another reference"
        );
        assert_eq!(
            field_for(0x5555_0000, 0x2_b2d4, 0x1f),
            0x1f,
            "another module"
        );
        LIFTED_AT.store(0, Ordering::Release);
    }
}
