//! Spectator clicks and keys work again as soon as the demo clock goes back
//! (#1296), after a click made while a `viewdemo` demo was still loading.
//!
//! ## The gate
//!
//! `CHudSpectator::HandleButtonsDown` (`client+0x386a0`, see
//! `spectator_buttons`), where `+attack` steps to the next player and `+jump`
//! changes the camera mode, slows clicks down with a timestamp on the client
//! clock:
//!
//! ```text
//!     +0x38708  fld   [esi+0x1b18]        ; m_flNextObserverInput  <- GATE_PATTERN
//!     +0x3870e  fcomp [esp+0xc]           ; GetClientTime(), read at the top
//!               ...  je return            ; still ahead of the clock: ignored
//!     ...the button runs...
//!     +0x387f6  fstp  [esi+0x1b18]        ; = the clock + 0.2
//! ```
//!
//! Only `Init` and `InitHUDData` clear it; nothing does when the clock goes
//! back. And in a demo it does: while `viewdemo` loads, the client runs ahead
//! through the demo (the console prints its kills and chat), then playback
//! starts again from the beginning (`docs/goldsrc_viewdemo.md` §5). A click
//! or a Space pressed during that load stamps the gate with a time from
//! further on in the demo, so once playback restarts every spectator button
//! is ignored until the demo catches up with it, which can be most of the
//! demo. (A seek back in an HLTV demo restarts it from its start, which
//! clears the gate: measured, not a way in.) It fits what #1296 reports on
//! POST (mouse and Space did nothing in an HLTV demo until a fast-forward),
//! and is reproduced on both builds by
//! pressing `+attack` during the load: the gate stayed at 61.49 while the
//! clock started again from 1, and Space, `+attack` and `+jump` did nothing.
//!
//! ## The fix
//!
//! Every frame, when the gate is more than [`SLACK`] ahead of the client
//! clock, it is set back to 0, which the game's own `InitHUDData` writes. The
//! gate is never legitimately more than 0.2 s ahead, so a press made since the
//! clock went back keeps its 0.2 s. A plain store into `client.dll`'s data,
//! not code: no page protection changes.
//!
//! The spectator HUD's address comes from `+attack`'s down handler
//! (`mov ecx, <gHUD.m_Spectator>; push 1; call HandleButtonsDown`, the only
//! match of [`OBJECT_PATTERN`]), checked to call the function
//! `spectator_buttons` found, and the gate's offset from the `fld` in that
//! function. `client.dll` is the same file in both engine builds;
//! `tools/verify_spectator_input_gate_offsets.py` checks both patterns.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// `IN_AttackDown`'s call: `mov ecx, imm32; push 1 (IN_ATTACK); call rel32`.
const OBJECT_PATTERN: &str = "B9 ?? ?? ?? ?? 6A 01 E8";
/// Where the spectator HUD's address sits in [`OBJECT_PATTERN`].
const OBJECT_AT: usize = 1;
/// Where the call's `rel32` sits in [`OBJECT_PATTERN`].
const CALL_REL_AT: usize = 8;
/// The gate's test in `HandleButtonsDown`: `fld dword [esi+disp32]`, `fcomp
/// qword [esp+0xc]`, `fnstsw ax`, `and eax, 0x4100`, `je`.
const GATE_PATTERN: &str = "D9 86 ?? ?? ?? ?? DC 5C 24 0C DF E0 25 00 41 00 00 0F 84";
/// Where the gate's offset in the spectator HUD sits in [`GATE_PATTERN`].
const GATE_OFFSET_AT: usize = 2;
/// How far into `HandleButtonsDown` the gate's test may sit (it is at +0x68).
const GATE_WITHIN: usize = 0x100;
/// How far ahead of the clock the gate may be before it is stale: the game
/// sets it 0.2 s ahead.
const SLACK: f32 = 1.0;

/// The gate's address, resolved against [`SCANNED_BASE`]; 0 until then.
static GATE: AtomicUsize = AtomicUsize::new(0);
/// The `client.dll` base the gate was looked for in (found or not).
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// Whether a failure to find it has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

/// The gate's address, or why there is none.
fn find_gate(base: usize) -> Result<usize, String> {
    let function = crate::spectator_buttons::address()?;
    // Safety: `base` is client.dll's, mapped for the session; each match lies
    // in its code, and the reads stay inside it (the call's rel32 is the four
    // bytes after the pattern's E8).
    unsafe {
        let call = scan::find_unique(base, OBJECT_PATTERN)
            .map_err(|why| format!("+attack's call -- {why}"))?;
        let object = std::ptr::read_unaligned((call + OBJECT_AT) as *const u32) as usize;
        let rel = std::ptr::read_unaligned((call + CALL_REL_AT) as *const i32);
        let target = (call + CALL_REL_AT + 4).wrapping_add_signed(rel as isize);
        if target != function {
            return Err(format!(
                "+attack calls +{:#x}, not the spectator buttons at +{:#x}",
                target.wrapping_sub(base),
                function - base
            ));
        }
        let test = scan::find_unique(base, GATE_PATTERN)
            .map_err(|why| format!("the click gate's test -- {why}"))?;
        if !(function..function + GATE_WITHIN).contains(&test) {
            return Err(format!(
                "the click gate's test at +{:#x} is not in the spectator buttons",
                test - base
            ));
        }
        let offset = std::ptr::read_unaligned((test + GATE_OFFSET_AT) as *const u32) as usize;
        Ok(object + offset)
    }
}

/// Whether a gate at `gate` is stale at client time `now`.
fn is_stale(gate: f32, now: f32) -> bool {
    gate - now > SLACK
}

/// Clears a gate left ahead of the clock. Called every frame from
/// `commands::poll`, on the engine's thread (the only one that writes it).
pub fn poll() {
    let Some(base) = engine::client_module_base() else {
        return;
    };
    let Some(now) = engine::engine_client_time() else {
        return;
    };
    if SCANNED_BASE.swap(base, Ordering::AcqRel) != base {
        match find_gate(base) {
            Ok(gate) => GATE.store(gate, Ordering::Release),
            Err(why) => {
                GATE.store(0, Ordering::Release);
                if !COMPLAINED.swap(true, Ordering::Relaxed) {
                    unsafe {
                        crate::debug::report(&format!(
                            "spectator_input_gate: not installed -- after a click during a demo's load, spectator clicks may do nothing for a while: {why}"
                        ))
                    };
                }
            }
        }
    }
    let gate = GATE.load(Ordering::Acquire);
    if gate == 0 {
        return;
    }
    // Safety: a field of client.dll's static spectator HUD, which lives as
    // long as the module; only the engine's thread, this one, touches it.
    let ahead = unsafe { std::ptr::read_volatile(gate as *const f32) };
    if is_stale(ahead, now) {
        unsafe { std::ptr::write_volatile(gate as *mut f32, 0.0) };
        unsafe {
            crate::debug::report(&format!(
                "spectator_input_gate: the demo clock went back to {now:.2} s past a click's {ahead:.2} s -- spectator clicks and keys work again (#1296)"
            ))
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_press_is_kept() {
        // HandleButtonsDown's own 0.2 s.
        assert!(!is_stale(10.2, 10.0));
        assert!(!is_stale(0.0, 10.0));
    }

    #[test]
    fn a_press_from_later_in_the_demo_is_stale() {
        // #1296's probe: pressed at 61.29 during the load, clock back at 1.
        assert!(is_stale(61.49, 1.0));
    }

    #[test]
    fn the_offsets_sit_on_the_wildcards() {
        for (pattern, at, len) in [
            (OBJECT_PATTERN, OBJECT_AT, 4),
            (OBJECT_PATTERN, CALL_REL_AT, 0),
            (GATE_PATTERN, GATE_OFFSET_AT, 4),
        ] {
            let tokens: Vec<&str> = pattern.split_whitespace().collect();
            assert!(tokens[at..at + len].iter().all(|t| *t == "??"), "{pattern}");
        }
        // The call's rel32 follows the pattern's E8.
        assert_eq!(OBJECT_PATTERN.split_whitespace().count(), CALL_REL_AT);
    }
}
