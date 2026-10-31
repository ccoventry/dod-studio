//! A POV demo's red crosshair (`cl_xhair_style 0`) opens while its recorder
//! runs, as it did when he played (follows #1187, #308).
//!
//! The game's gap function (`crosshair_gap` has it) widens the gap above
//! 105 u/s from the velocity at `client+0x1080f8`, which only
//! `HUD_WeaponsPostThink` writes, from the client's own prediction. A demo
//! plays without prediction, so in a POV demo that velocity is whatever was
//! there before it (zero in a fresh game) and the crosshair keeps its resting
//! gap however fast the recorder runs. Its other inputs, the weapon and
//! whether a machine gun is deployed, come from recorded messages and entity
//! state and already follow the demo.
//!
//! While a POV demo plays, the style-0 draw's gap call comes here
//! (`crosshair_gap`); [`gap`] puts the view's velocity in that slot for the
//! one call to the game's own function and puts the old value back after
//! it. Everything else -- `cl_dynamic_xhair`, the fixed grenade and rocket
//! gap, the weapon table, the 3 px a frame easing, the x87's precision -- is
//! the game's own code. Nothing else that reads the slot sees the change.
//!
//! The velocity is `ref_params_s`'s `simvel` when the engine calls
//! `V_CalcRefdef` for the main view, which in a POV demo is the recorder's:
//! the same predicted velocity his crosshair read live, the one his view bob
//! moves with (`pov_live_bob`). Measured in `wsod25_grp2_h1_warchyld` (PRE):
//! on 1,290 of the 1,371 frames whose `simorg` was exactly a recorded
//! frame's, `simvel` was that frame's recorded one to 0.02 u/s; the other
//! frames fall between recorded ones, where the engine interpolates the
//! view. It is the full 3D velocity, as live: a fall widens it too.
//!
//! Live play and HLTV demos are untouched: the call is pointed here only
//! while a POV demo plays, and [`gap`] only runs in the demo's own view
//! (`g_iUser1` 0). No setting: the player's choice is the stock
//! `cl_dynamic_xhair`, which the game's function reads as before.

// The hook's half is 32-bit only; the host build (clippy, tests) compiles
// the rest.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use crate::engine::RefParamsPartial;

/// Whether a POV demo is playing, as of this frame's [`poll`].
static PLAYING: AtomicBool = AtomicBool::new(false);
/// The main view's `simvel`, as f32 bits.
static VELOCITY: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
/// For `ds_debug_status`: the last gap drawn, -1 before one.
static SHOWN: AtomicI32 = AtomicI32::new(-1);
/// Whether setting the call up has failed and said so, so it is said once.
static FAILED: AtomicBool = AtomicBool::new(false);

/// Points the red crosshair's gap call here while a POV demo plays, and back
/// at the game's own otherwise. Called once a frame from `commands::poll`.
pub fn poll() {
    let playing = crate::anim_fix::pov_demo_playing();
    if PLAYING.swap(playing, Ordering::AcqRel) && !playing {
        forget_velocity();
    }
    match crate::crosshair_gap::set_pov_demo_hook(playing) {
        Ok(_) => FAILED.store(false, Ordering::Relaxed),
        Err(why) => {
            if !FAILED.swap(true, Ordering::Relaxed) {
                // Safety: plain file append.
                unsafe {
                    crate::debug::report(&format!(
                        "pov_crosshair_gap: a POV demo's red crosshair keeps the game's own gap -- {why}"
                    ))
                };
            }
        }
    }
}

/// Whether a POV demo is playing: the gap hook's test for sending the call
/// to [`gap`].
pub fn playing() -> bool {
    PLAYING.load(Ordering::Relaxed)
}

/// Keeps the main view's velocity for this frame's crosshair. Called from
/// the `V_CalcRefdef` hook for the main view, which runs before the HUD
/// draws.
pub fn note_view(view: &RefParamsPartial) {
    if !playing() {
        return;
    }
    for (slot, value) in VELOCITY.iter().zip(view.simvel) {
        slot.store(value.to_bits(), Ordering::Relaxed);
    }
}

fn forget_velocity() {
    for slot in &VELOCITY {
        slot.store(0, Ordering::Relaxed);
    }
    SHOWN.store(-1, Ordering::Relaxed);
}

fn velocity() -> [f32; 3] {
    VELOCITY
        .each_ref()
        .map(|slot| f32::from_bits(slot.load(Ordering::Relaxed)))
}

/// The game's own gap for the demo's view, with the view's velocity in the
/// slot the game reads for the length of the call. Without the slot's
/// address (not scanned yet), the game's own gap as it is.
///
/// # Safety
///
/// `this` is the `CHudDoDCrossHair` the style-0 draw passed, and this runs
/// on the HUD's thread, inside that draw.
#[cfg(target_arch = "x86")]
pub unsafe fn gap(this: *mut u8) -> i32 {
    let gap = match crate::crosshair_gap::velocity_slot() {
        // Safety: the slot the spread function reads, 12 bytes checked
        // readable at the scan, in client.dll's writable data.
        Some(slot) => unsafe {
            with_velocity(slot as *mut [f32; 3], velocity(), || {
                crate::crosshair_gap::original_gap(this)
            })
        },
        None => unsafe { crate::crosshair_gap::original_gap(this) },
    };
    SHOWN.store(gap, Ordering::Relaxed);
    gap
}

/// Runs `call` with `velocity` in `slot`, then puts the slot's old value
/// back, so only `call` sees it.
///
/// # Safety
///
/// `slot` is valid for reads and writes for the whole call.
unsafe fn with_velocity<R>(slot: *mut [f32; 3], velocity: [f32; 3], call: impl FnOnce() -> R) -> R {
    // Volatile: the game reads the slot through its own code, which the
    // compiler cannot see.
    let kept = unsafe { slot.read_volatile() };
    unsafe { slot.write_volatile(velocity) };
    let result = call();
    unsafe { slot.write_volatile(kept) };
    result
}

/// The 3D speed the game compares with 105 u/s.
fn speed(velocity: [f32; 3]) -> f32 {
    velocity.iter().map(|v| v * v).sum::<f32>().sqrt()
}

/// One line for `ds_debug_status` while a POV demo plays.
pub fn status_line() -> Option<String> {
    if !playing() {
        return None;
    }
    let shown = match SHOWN.load(Ordering::Relaxed) {
        gap if gap < 0 => "not drawn yet".to_string(),
        gap => format!("{gap} px"),
    };
    Some(format!(
        "POV demo red crosshair: gap {shown}, the recorder at {:.0} u/s (the game widens it above 105)",
        speed(velocity())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_sees_the_view_velocity_and_the_slot_gets_its_own_back() {
        let mut slot = [1.0f32, 2.0, 3.0];
        let at = &raw mut slot;
        let seen = unsafe { with_velocity(at, [250.0, 0.0, -10.0], || at.read()) };
        assert_eq!(seen, [250.0, 0.0, -10.0]);
        assert_eq!(slot, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn the_speed_is_three_dimensional_as_the_games() {
        assert_eq!(speed([3.0, 4.0, 0.0]), 5.0);
        assert_eq!(speed([0.0, 0.0, -200.0]), 200.0);
    }
}
