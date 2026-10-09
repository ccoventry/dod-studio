//! The gun-lowering part of `dodstudio_spec_match_pov`: the spectated
//! first-person gun drops off the bottom of the screen when the player's own
//! would (issue #559).
//!
//! ## What the player sees
//!
//! While playing, DoD lowers the gun during a sprint, a jump (until landing),
//! going prone or getting up, crawling, on a ladder, and fully underwater --
//! the same states that hide the crosshair through `g_ihidexhair` (#310). It
//! does it in `DoDGunGoOnOffScreen` (dod13-client `cl_dll/dod_common.cpp`),
//! called from `V_CalcRefdef` (`cl_dll/view.cpp`) once a frame:
//!
//! ```text
//!     lowered:     if ((int)offscreen < 55) offscreen += 1;
//!     otherwise:   if (offscreen >= 3)  offscreen -= 3;
//!                  else                 offscreen = 0;
//!     delta = up + forward + up;
//!     viewmodel.origin += delta * offscreen * -0.7;
//! ```
//!
//! So it takes 55 frames to go down and 19 to come back, at whatever frame
//! rate the game runs. A deployed machine gun never lowers. Those numbers
//! are read from `client.dll` (`+0x2c99e`..`+0x2ca0a`), not the
//! reconstruction: dod13-client leaves out the `else offscreen = 0`, and a
//! gun left 1 unit down and back for good looks held at another angle.
//!
//! The function returns 0 at once while spectating, which is why an HLTV
//! view keeps the gun up. #204 had concluded that sprint does nothing to the
//! viewmodel; it had looked at the model and its animation, which really
//! don't change, and not at its position.
//!
//! ## What this does
//!
//! `anim_fix` decides each frame whether the spectated player's gun would be
//! lowered (`crosshair_rule::gun_lowered`, the same signals as the crosshair:
//! gait `dod_sprint`, body `jump`, the prone timer, gait `prone_forward`,
//! `movetype` 5). [`after_calc_refdef`] runs after `client.dll`'s own
//! `V_CalcRefdef` (`cldll_func_t` slot 19), steps the offset exactly as DoD
//! does, and moves the viewmodel the same way. Only while the camera is in a
//! player's eyes and the switch is on; anywhere else nothing is touched.
//!
//! Underwater is left out, as it is for the crosshair: the body's `swim`
//! covers the surface too.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::engine::{self, RefParamsPartial};

/// DoD's own numbers.
const STEP_DOWN: f32 = 1.0;
const STEP_UP: f32 = 3.0;
const MOST: f32 = 54.0;
const SCALE: f32 = -0.7;

/// Whether the spectated player's gun would be lowered this frame.
static LOWERED: AtomicBool = AtomicBool::new(false);
/// `v_modeloffscreen`, as f32 bits.
static OFFSCREEN: AtomicU32 = AtomicU32::new(0);
/// The `ref_params_s::time` the offset was last stepped for, as f64 bits, so
/// a second view in one frame (the picture-in-picture inset) doesn't step it
/// twice.
static STEPPED_AT: AtomicU64 = AtomicU64::new(0);

/// Says whether the gun would be lowered. Called every frame by
/// `anim_fix::apply()`, with `false` whenever there is no such player.
pub fn set_lowered(lowered: bool) {
    LOWERED.store(lowered, Ordering::Relaxed);
}

/// The camera moved to another player: his gun starts where the game's does,
/// up.
pub fn reset() {
    OFFSCREEN.store(0f32.to_bits(), Ordering::Relaxed);
}

/// DoD's step, from one frame's offset to the next.
fn step(offscreen: f32, lowered: bool) -> f32 {
    if lowered {
        if offscreen <= MOST {
            offscreen + STEP_DOWN
        } else {
            offscreen
        }
    } else if offscreen >= STEP_UP {
        offscreen - STEP_UP
    } else {
        0.0
    }
}

/// How far to move the viewmodel for this offset, given the view's forward
/// and up vectors.
fn offset(offscreen: f32, forward: [f32; 3], up: [f32; 3]) -> [f32; 3] {
    let scale = offscreen * SCALE;
    [0, 1, 2].map(|i| (up[i] + forward[i] + up[i]) * scale)
}

/// Runs after `client.dll`'s own `V_CalcRefdef`, with the same `pparams`.
///
/// Safety: `pparams` is the engine's `ref_params_s` for this view, valid for
/// the call.
pub unsafe fn after_calc_refdef(pparams: *mut RefParamsPartial) {
    if pparams.is_null() {
        return;
    }
    if !crate::anim_fix::active() || crate::spectator_target::in_eye_target().is_none() {
        reset();
        return;
    }
    let params = unsafe { &*pparams };
    let time = f64::from(params.time);
    let mut offscreen = f32::from_bits(OFFSCREEN.load(Ordering::Relaxed));
    if STEPPED_AT.swap(time.to_bits(), Ordering::Relaxed) != time.to_bits() {
        offscreen = step(offscreen, LOWERED.load(Ordering::Relaxed));
        OFFSCREEN.store(offscreen.to_bits(), Ordering::Relaxed);
    }
    if offscreen == 0.0 {
        return;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let viewmodel = unsafe { (engfuncs.get_view_model)() };
    if viewmodel.is_null() {
        return;
    }
    let [x, y, z] = offset(offscreen, params.forward, params.up);
    // Safety: the engine's own viewmodel entity, which V_CalcRefdef has just
    // placed for this frame and the renderer reads next.
    let origin = unsafe { &mut (*viewmodel).origin };
    origin.x += x;
    origin.y += y;
    origin.z += z;
}

/// One line for `dodstudio_debug_status`.
pub fn status() -> String {
    let offscreen = f32::from_bits(OFFSCREEN.load(Ordering::Relaxed));
    format!(
        "the gun {} (lowered {offscreen:.0} of 55)",
        if LOWERED.load(Ordering::Relaxed) {
            "is going down, as the player's own would"
        } else {
            "is up"
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_takes_55_frames_down_and_19_back_up() {
        let mut offscreen = 0.0;
        for _ in 0..55 {
            offscreen = step(offscreen, true);
        }
        assert_eq!(offscreen, 55.0);
        // And no further.
        assert_eq!(step(offscreen, true), 55.0);
        for _ in 0..18 {
            offscreen = step(offscreen, false);
        }
        assert_eq!(offscreen, 1.0);
        // Under 3, the binary snaps it home: no unit is left over.
        assert_eq!(step(offscreen, false), 0.0);
    }

    #[test]
    fn the_gun_moves_down_and_back_along_the_view() {
        // Looking straight ahead along +x: down 2 units and back 1 per unit
        // of offset, times 0.7.
        let [x, y, z] = offset(10.0, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert!((x + 7.0).abs() < 1e-5, "{x}");
        assert_eq!(y, 0.0);
        assert!((z + 14.0).abs() < 1e-5, "{z}");
    }
}
