//! The gun-lowering part of `ds_spec_match_pov`: the spectated
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
//! gait `dod_sprint`, body `jump` or a crouched take-off, the prone timer,
//! gait `prone_forward` while he moves, `movetype` 5). [`after_calc_refdef`]
//! runs after `client.dll`'s own `V_CalcRefdef` (`cldll_func_t` slot 19),
//! steps the offset and moves the viewmodel the way DoD does. Only while the
//! camera is in a player's eyes and the switch is on; anywhere else nothing
//! is touched.
//!
//! ## Stepped by time, not by frame
//!
//! A POV demo has the recorder's frame rate baked in: its gun took 55 of
//! *his* frames to go down. Across 19 POV demos from 19 players (#704) that
//! was 0.18-0.53 s down (median 0.28) and 0.07-0.19 s up (median 0.10), the
//! recorders running at about 104-299 fps. Stepping once per playback frame
//! instead tied the HLTV ramp to the capture rate: 0.18 s at 300 fps, 0.46 s
//! at OBS's 120, 0.92 s at 60. So the offset moves at [`STEPS_PER_SECOND`],
//! the rate of a 200 fps recorder: 0.275 s down and 0.09 s up, at any
//! capture or playback rate.
//!
//! Underwater is left out, as it is for the crosshair: the body's `swim`
//! covers the surface too.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::engine::{self, RefParamsPartial};

/// DoD's own numbers, per frame of the player's game.
const STEP_DOWN: f32 = 1.0;
const STEP_UP: f32 = 3.0;
const MOST: f32 = 55.0;
const SCALE: f32 = -0.7;
/// The player's frames per second the steps are taken at: the median POV
/// recorder's ramp (module docs).
const STEPS_PER_SECOND: f32 = 200.0;
/// The longest gap stepped in one go. Longer is a pause or a seek, not a
/// frame, and moves the gun no further than a 10 fps frame would.
const LONGEST_FRAME: f32 = 0.1;

/// Whether the spectated player's gun would be lowered this frame.
static LOWERED: AtomicBool = AtomicBool::new(false);
/// `v_modeloffscreen`, as f32 bits.
static OFFSCREEN: AtomicU32 = AtomicU32::new(0);
/// The `ref_params_s::time` the offset was last stepped for, as f64 bits, so
/// a second view in one frame (the picture-in-picture inset) doesn't step it
/// twice, and the next frame knows how much time has passed.
static STEPPED_AT: AtomicU64 = AtomicU64::new(0);

/// Says whether the gun would be lowered. Called every frame by
/// `anim_fix::apply()`, with `false` whenever there is no such player.
pub fn set_lowered(lowered: bool) {
    LOWERED.store(lowered, Ordering::Relaxed);
}

/// Whether the watched player's gun was last said to be lowered (for
/// `gun_log`).
pub fn lowered() -> bool {
    LOWERED.load(Ordering::Relaxed)
}

/// The camera moved to another player: his gun starts where the game's does,
/// up.
pub fn reset() {
    OFFSCREEN.store(0f32.to_bits(), Ordering::Relaxed);
}

/// DoD's step, scaled to `seconds` of the player's time: from one frame's
/// offset to the next.
fn step(offscreen: f32, lowered: bool, seconds: f32) -> f32 {
    let frames = seconds.clamp(0.0, LONGEST_FRAME) * STEPS_PER_SECOND;
    if lowered {
        (offscreen + STEP_DOWN * frames).min(MOST)
    } else {
        (offscreen - STEP_UP * frames).max(0.0)
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
    let stepped_at = f64::from_bits(STEPPED_AT.swap(time.to_bits(), Ordering::Relaxed));
    if stepped_at != time {
        // A clock that went backwards (a new demo, a seek back) moves it
        // nothing this frame; `step` clamps the negative gap to zero.
        let seconds = if stepped_at == 0.0 {
            0.0
        } else {
            (time - stepped_at) as f32
        };
        offscreen = step(offscreen, LOWERED.load(Ordering::Relaxed), seconds);
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

/// One line for `ds_debug_status`.
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

    /// Seconds the gun takes to go all the way down (or up) when played at
    /// `fps`.
    fn seconds_to(lowered: bool, fps: f32) -> f32 {
        let mut offscreen = if lowered { 0.0 } else { MOST };
        let target = if lowered { MOST } else { 0.0 };
        let mut frames = 0;
        while offscreen != target {
            offscreen = step(offscreen, lowered, 1.0 / fps);
            frames += 1;
            assert!(frames < 100_000, "never got there at {fps} fps");
        }
        frames as f32 / fps
    }

    #[test]
    fn it_takes_the_same_time_at_any_frame_rate() {
        // A 200 fps recorder's 55 frames down and 19 up, whatever the
        // capture runs at.
        for fps in [60.0, 120.0, 200.0, 300.0, 1000.0] {
            let down = seconds_to(true, fps);
            let up = seconds_to(false, fps);
            assert!(
                (down - 0.275).abs() <= 1.0 / fps + 1e-4,
                "{fps} fps: {down} s down"
            );
            assert!(
                (up - 0.0917).abs() <= 1.0 / fps + 1e-4,
                "{fps} fps: {up} s up"
            );
        }
    }

    #[test]
    fn it_stops_at_55_and_at_0() {
        assert_eq!(step(54.9, true, 0.05), MOST);
        assert_eq!(step(MOST, true, 0.05), MOST);
        assert_eq!(step(1.0, false, 0.05), 0.0);
    }

    #[test]
    fn a_pause_or_a_clock_going_back_moves_it_little_or_not_at_all() {
        // A seek forward moves it no further than one long frame.
        assert_eq!(
            step(0.0, true, 30.0),
            STEP_DOWN * LONGEST_FRAME * STEPS_PER_SECOND
        );
        // A new demo's clock, behind the old one, moves it nothing.
        assert_eq!(step(20.0, true, -500.0), 20.0);
        assert_eq!(step(20.0, false, -500.0), 20.0);
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
