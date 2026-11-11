//! Gives each snowflake a start time, so forced snow can't hang the game (#1369).
//!
//! ## What the game does
//!
//! A snowflake (`CSnowParticle`, 0x138 bytes from particleman's pool) moves
//! in its `Think(time)` (`client+0x52940`) by `velocity * (time - last)`,
//! where `last` is the float at `+0x130`, and then stores `time` there. The
//! flake's constructor (`client+0x52b80`) never sets `+0x130`, so the first
//! think moves the flake by whatever that memory held. A block last used by
//! a snowflake holds a time, and fresh pool memory holds 0 (a long but finite
//! move), so a map's own snow gets away with it. A rain drop is 0x12c bytes,
//! though: a flake that takes a block rain used reads bytes past the drop,
//! and on POST they held particleman's fill pattern `0xffefefeb`, a NaN.
//! The flake's origin turns NaN, and particleman's collision trace from its
//! last origin to that one never ends: `PM_RecursiveHullCheck`'s "back off
//! from solid" loop (`hw.dll+0x1e2819` calling `PM_HullPointContents` at
//! `+0x1e2100`) steps a NaN fraction that never goes below 0. The game
//! thread spins there, alive, taking no more commands. Only
//! `ds_map_weather` makes rain and snow fall on one map, so only forcing
//! snow after rain hit it (stack read and run counts in #1369).
//!
//! ## The change
//!
//! The constructor reads the client time (`call [GetClientTime]` at
//! `+0x52c74`) to work out the flake's die time. Right after that call, a
//! detour stores the same time at `+0x130` too (`fst`, so the x87 stack the
//! constructor goes on with is unchanged), and the first think then moves
//! the flake by the frame's time, as every later one does. It goes in the
//! first time any weather is forced, since forced rain followed by the
//! map's own snow (`ds_map_weather 1`, then `0` on a snowy map) mixes the
//! two as well, and stays for the session, like every detour here
//! (`detour.rs`). A game that never forces weather is never patched.
//!
//! `tools/verify_map_weather_offsets.py` checks the pattern is unique, the
//! span's bytes, and that nothing branches into it, in both installs'
//! `client.dll` (byte-identical).

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::detour::{self, Detour};
use crate::scan;

/// The constructor's die-time step: the store of the flake's z velocity
/// before it, the `GetClientTime` call, and the two moves the detour takes
/// over. The call's IAT slot is wildcarded.
///
/// ```text
/// client+0x52c6a  mov  dword ptr [esi+0x88], -75.0
/// client+0x52c74  call dword ptr [GetClientTime]     ; st0 = time
/// client+0x52c7a  mov  ecx, dword ptr [esi+4]         ; the detour
/// client+0x52c7d  mov  dword ptr [esp+8], 128.0
/// client+0x52c85  mov  eax, dword ptr [esp+8]
/// client+0x52c89  and  ecx, 0xffffff9a
/// ```
const PATTERN: &str = "C7 86 88 00 00 00 00 00 96 C2 FF 15 ?? ?? ?? ?? \
                       8B 4E 04 C7 44 24 08 00 00 00 43 8B 44 24 08 83 E1 9A";

/// From the match to the instruction after the call.
const DETOUR_AT: usize = 16;

/// `mov ecx, [esi+4]; mov dword ptr [esp+8], 0x43000000`: what the jump
/// overwrites, reproduced at the end of the stub. Eleven bytes, so six `nop`s
/// of padding. Neither reads the x87 stack, the call's return lands on the
/// first byte, and nothing branches into the rest (the verify tool checks).
const STOLEN: &[u8] = &[
    0x8b, 0x4e, 0x04, 0xc7, 0x44, 0x24, 0x08, 0x00, 0x00, 0x00, 0x43,
];

/// The flake's last-think time.
const LAST_THINK: u32 = 0x130;

/// Where the stub jumps back to, read by its `jmp dword [abs]`.
static RESUME: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: Mutex<Option<Detour>> = Mutex::new(None);

/// `fst dword ptr [esi+0x130]`, the stolen moves, `jmp dword [resume]`.
fn stub(resume_slot: usize) -> Vec<u8> {
    let mut code = vec![0xd9, 0x96];
    code.extend_from_slice(&LAST_THINK.to_le_bytes());
    code.extend_from_slice(STOLEN);
    code.extend_from_slice(&[0xff, 0x25]);
    code.extend_from_slice(&(resume_slot as u32).to_le_bytes());
    code
}

/// Installs the detour in the `client.dll` at `base`, once. Runs on the
/// engine thread (`map_weather::poll`); the lock only guards the slot.
pub fn ensure(base: usize) -> Result<(), String> {
    let mut slot = INSTALLED
        .lock()
        .map_err(|_| "the snow detour's lock is poisoned".to_string())?;
    if slot.is_some() {
        return Ok(());
    }
    // Safety: `base` is client.dll, mapped for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the snowflake's start time -- {why}"))?;
    let target = found + DETOUR_AT;
    // Safety: inside the pattern just matched.
    let present = unsafe { std::slice::from_raw_parts(target as *const u8, STOLEN.len()) };
    if present != STOLEN {
        return Err(format!(
            "expected {STOLEN:02x?} at client+{:#x}, found {present:02x?}",
            target - base
        ));
    }
    RESUME.store(target + STOLEN.len(), Ordering::Release);
    let code = stub(RESUME.as_ptr() as usize);
    // Safety: the span was checked byte for byte; the branch condition is
    // the verify tool's.
    let installed = unsafe { detour::install(target, STOLEN.len(), &code) }?;
    unsafe {
        crate::debug::report(&format!(
            "map_weather: snowflakes get a start time (detour at client+{:#x}, stub at {:#x})",
            target - base,
            installed.stub_address()
        ))
    };
    *slot = Some(installed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        #[rustfmt::skip]
        assert_eq!(
            stub(0x1234_5678),
            vec![
                0xd9, 0x96, 0x30, 0x01, 0x00, 0x00,       // fst dword [esi+0x130]
                0x8b, 0x4e, 0x04,                         // mov ecx, [esi+4]
                0xc7, 0x44, 0x24, 0x08, 0x00, 0x00, 0x00, 0x43, // mov dword [esp+8], 128.0
                0xff, 0x25, 0x78, 0x56, 0x34, 0x12,       // jmp dword [RESUME]
            ]
        );
    }

    #[test]
    fn the_detour_starts_after_the_call_and_takes_the_pattern_s_bytes() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        // ff 15 <slot>: the call ends right where the detour starts.
        assert_eq!(tokens[DETOUR_AT - 6..DETOUR_AT - 4], ["FF", "15"]);
        let stolen: Vec<u8> = tokens[DETOUR_AT..DETOUR_AT + STOLEN.len()]
            .iter()
            .map(|t| u8::from_str_radix(t, 16).expect("no wildcard in the stolen span"))
            .collect();
        assert_eq!(stolen, STOLEN);
        // The jump needs five bytes.
        assert!(STOLEN.len() >= 5);
    }

    #[test]
    fn the_store_is_the_flake_s_last_think_time() {
        // fst m32fp is D9 /2: modrm 0x96 = mod 10 (disp32), reg 2, rm esi.
        let code = stub(0);
        assert_eq!(&code[..2], &[0xd9, 0x96]);
        assert_eq!(u32::from_le_bytes(code[2..6].try_into().unwrap()), 0x130);
    }
}
