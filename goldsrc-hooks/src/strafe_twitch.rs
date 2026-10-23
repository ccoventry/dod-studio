//! Stops strafing players' models twitching in an HLTV demo (#927): on the
//! frame the demo clock steps back, each player model is drawn where it was
//! drawn the frame before. Part of `ds_spec_match_pov`: a POV demo and live
//! play don't twitch.
//!
//! ## What the twitch is
//!
//! While an HLTV demo plays in `viewdemo`, the client clock (`cl.time`) steps
//! back by about a millisecond on roughly one frame in ten whenever the game
//! draws faster than the demo's packets arrive. Measured at the user's 165 fps
//! on two HLTV demos from different matches (100 Hz and 28 Hz player updates),
//! on PRE and on POST: about one frame in eleven. At `fps_max 100` it never
//! stepped back, under `playdemo` at a fixed `host_framerate` neither, and in
//! a POV demo (m00cat's, same teams) 0-3 times in 20 s. On that frame every
//! interpolated player is drawn a fraction of a unit behind where he was the
//! frame before. HLAE-only runs (no DoD Studio DLL) twitch the same way.
//!
//! DoD's player renderer (`CGameStudioModelRenderer::StudioEstimateGait` and
//! `CalculateYawBlend`) points the legs along the model's movement since the
//! last frame, and twists the torso (the aim sequences' 3x3 blend,
//! `blending[0]`) by the angle between the view and the legs. A step back
//! points the legs the opposite way. Running forward, that's more than 120
//! degrees off the view, and the renderer's own rule turns the legs back
//! round, so nothing shows. Strafing, it's only about 90 degrees off, so the
//! model is drawn for that one frame turned 180 degrees with the torso
//! twisted to the other extreme: the upper body flickers, several times a
//! second (about 7 a second on a player strafing left and right), while he
//! strafes. A paused frame never shows it, since it lasts one frame.
//!
//! ## The hold
//!
//! This wraps the renderer's `StudioDrawPlayer`. On a frame whose clock is
//! earlier than the last frame's, a player drawn on the last frame is drawn
//! at that frame's origin again: the renderer sees no movement, keeps the legs
//! where they were, and the torso blend stays put. The entity's own origin is
//! put back after the call, so nothing else sees a change. The model is drawn
//! at most a fraction of a unit from where it would have been, for one frame.
//!
//! It has to be the renderer's call: `HUD_AddEntity` sees the same entity,
//! but the engine sets the origin it draws from after that (measured: an
//! origin written there never reached the gait estimate).

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::engine::{self, ClEntityS, Vec3};

/// GoldSrc's player slots, entity indices 1..=32.
const MAX_PLAYERS: usize = 32;

/// A step back larger than this is a seek or a new demo, not jitter.
const MAX_STEP_BACK: f64 = 0.05;

/// `int StudioDrawPlayer(int flags, entity_state_t *pplayer)`.
type DrawPlayerFn = unsafe extern "C" fn(i32, *mut c_void) -> i32;
/// `cl_entity_t *GetCurrentEntity(void)`.
type CurrentEntityFn = unsafe extern "C" fn() -> *mut ClEntityS;
/// `void GetTimes(int *framecount, double *current, double *old)`.
type GetTimesFn = unsafe extern "C" fn(*mut i32, *mut f64, *mut f64);

static REAL_DRAW_PLAYER: AtomicUsize = AtomicUsize::new(0);

/// The clock of this frame and of the frame before it, as f64 bits. A frame
/// is told apart by its clock, not the renderer's frame count: a frame that
/// draws two views (the spectator inset) counts twice.
static FRAME_CLOCK: AtomicU64 = AtomicU64::new(0);
static PREVIOUS_CLOCK: AtomicU64 = AtomicU64::new(0);

/// No clock yet.
const NEVER: u64 = u64::MAX;

/// Per player: the origin he was last drawn at, the clock of that frame, and
/// the clock of the frame he was last held on.
static LAST_ORIGIN: [[AtomicU32; 3]; MAX_PLAYERS + 1] =
    [const { [const { AtomicU32::new(0) }; 3] }; MAX_PLAYERS + 1];
static LAST_CLOCK: [AtomicU64; MAX_PLAYERS + 1] =
    [const { AtomicU64::new(NEVER) }; MAX_PLAYERS + 1];
static HELD_CLOCK: [AtomicU64; MAX_PLAYERS + 1] =
    [const { AtomicU64::new(NEVER) }; MAX_PLAYERS + 1];

/// Player frames held so far, for `ds_debug_status`.
static HELD: AtomicU32 = AtomicU32::new(0);

/// Whether a player is held this frame. `now` is this frame's clock,
/// `previous` the last frame's, `last_drawn` the clock of the frame the player
/// was last drawn on, `last_held` the clock of the frame he was last held on
/// (f64 bits, [`NEVER`] for none).
fn should_hold(now: f64, previous: f64, last_drawn: u64, last_held: u64) -> bool {
    if last_held == now.to_bits() {
        // Drawn twice this frame: the same answer both times.
        return true;
    }
    let stepped_back = now < previous && previous - now < MAX_STEP_BACK;
    // Only a player drawn on the frame right before has an origin to hold.
    stepped_back && last_drawn == previous.to_bits()
}

/// Swaps `StudioDrawPlayer` in the renderer's interface for [`draw_player`].
/// `interface` is the `r_studio_interface_t` `HUD_GetStudioModelInterface`
/// handed back: `{ version, StudioDrawModel, StudioDrawPlayer }`.
///
/// Safety: `interface` must be that struct, alive for the session.
pub unsafe fn install(interface: *mut *mut c_void) {
    if interface.is_null() || REAL_DRAW_PLAYER.load(Ordering::Acquire) != 0 {
        return;
    }
    // Safety: the struct is three pointer-sized fields; the third is the slot.
    let slot = unsafe { interface.add(2) };
    if unsafe { *slot }.is_null() {
        return;
    }
    match unsafe {
        crate::patch::swap_vftable_slot(
            slot as usize,
            draw_player as *const () as usize,
            &REAL_DRAW_PLAYER,
        )
    } {
        Ok(()) => unsafe {
            crate::debug::report(&format!(
                "strafe twitch: wrapped StudioDrawPlayer (real at {:#x})",
                REAL_DRAW_PLAYER.load(Ordering::Acquire)
            ))
        },
        Err(why) => {
            REAL_DRAW_PLAYER.store(0, Ordering::Release);
            unsafe { crate::debug::report(&format!("strafe twitch: not installed: {why}")) };
        }
    }
}

unsafe extern "C" fn draw_player(flags: i32, pplayer: *mut c_void) -> i32 {
    let real = REAL_DRAW_PLAYER.load(Ordering::Acquire);
    if real == 0 {
        return 0;
    }
    // Safety: the renderer's own function, with its own arguments.
    let real: DrawPlayerFn = unsafe { std::mem::transmute(real) };
    if !crate::anim_fix::active() {
        return unsafe { real(flags, pplayer) };
    }
    let Some(studio) = engine::engine_studio() else {
        return unsafe { real(flags, pplayer) };
    };
    let current_slot = studio.slots_before_view_info()[0];
    let times_slot = studio.slots_before_view_info()[4];
    if current_slot.is_null() || times_slot.is_null() {
        return unsafe { real(flags, pplayer) };
    }
    // Safety: the engine's own accessors, called on its render thread.
    let current: CurrentEntityFn = unsafe { std::mem::transmute(current_slot) };
    let get_times: GetTimesFn = unsafe { std::mem::transmute(times_slot) };
    let ent = unsafe { current() };
    if ent.is_null() {
        return unsafe { real(flags, pplayer) };
    }
    let (mut frame, mut now, mut old) = (0i32, 0f64, 0f64);
    unsafe { get_times(&mut frame, &mut now, &mut old) };
    let now_bits = now.to_bits();
    let frame_clock = FRAME_CLOCK.swap(now_bits, Ordering::Relaxed);
    if frame_clock != now_bits {
        PREVIOUS_CLOCK.store(frame_clock, Ordering::Relaxed);
    }
    let previous = f64::from_bits(PREVIOUS_CLOCK.load(Ordering::Relaxed));
    // Safety: the entity being drawn, valid for this call.
    let e = unsafe { &mut *ent };
    let index = e.index;
    if !(1..=MAX_PLAYERS as i32).contains(&index) {
        return unsafe { real(flags, pplayer) };
    }
    let slot = index as usize;
    let hold = should_hold(
        now,
        previous,
        LAST_CLOCK[slot].load(Ordering::Relaxed),
        HELD_CLOCK[slot].load(Ordering::Relaxed),
    );
    if !hold {
        let result = unsafe { real(flags, pplayer) };
        let store = &LAST_ORIGIN[slot];
        store[0].store(e.origin.x.to_bits(), Ordering::Relaxed);
        store[1].store(e.origin.y.to_bits(), Ordering::Relaxed);
        store[2].store(e.origin.z.to_bits(), Ordering::Relaxed);
        LAST_CLOCK[slot].store(now_bits, Ordering::Relaxed);
        return result;
    }
    let held = &LAST_ORIGIN[slot];
    let saved = Vec3 {
        x: e.origin.x,
        y: e.origin.y,
        z: e.origin.z,
    };
    e.origin = Vec3 {
        x: f32::from_bits(held[0].load(Ordering::Relaxed)),
        y: f32::from_bits(held[1].load(Ordering::Relaxed)),
        z: f32::from_bits(held[2].load(Ordering::Relaxed)),
    };
    if HELD_CLOCK[slot].swap(now_bits, Ordering::Relaxed) != now_bits {
        HELD.fetch_add(1, Ordering::Relaxed);
    }
    let result = unsafe { real(flags, pplayer) };
    // Safety: the same entity; still this call's.
    let e = unsafe { &mut *ent };
    e.origin = saved;
    LAST_CLOCK[slot].store(now_bits, Ordering::Relaxed);
    result
}

/// For `ds_debug_status`: whether it's installed, and how many player frames
/// it has held.
pub fn status() -> String {
    if REAL_DRAW_PLAYER.load(Ordering::Acquire) == 0 {
        return "not installed (the renderer interface was never handed over)".to_string();
    }
    format!(
        "{} player frame(s) held on a clock step back",
        HELD.load(Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(t: f64) -> u64 {
        t.to_bits()
    }

    #[test]
    fn a_frame_after_a_step_back_holds() {
        assert!(should_hold(17.9281, 17.9295, bits(17.9295), NEVER));
    }

    #[test]
    fn a_frame_moving_on_draws() {
        assert!(!should_hold(17.9342, 17.9281, bits(17.9281), bits(17.9281)));
    }

    #[test]
    fn a_player_not_drawn_last_frame_draws() {
        assert!(!should_hold(17.9281, 17.9295, bits(17.5), NEVER));
    }

    #[test]
    fn a_seek_back_draws() {
        assert!(!should_hold(10.0, 17.9295, bits(17.9295), NEVER));
    }

    #[test]
    fn a_second_view_of_a_held_frame_holds_too() {
        assert!(should_hold(17.9281, 17.9295, bits(17.9281), bits(17.9281)));
    }

    #[test]
    fn two_steps_back_in_a_row_both_hold() {
        assert!(should_hold(17.9270, 17.9281, bits(17.9281), bits(17.9281)));
    }
}
