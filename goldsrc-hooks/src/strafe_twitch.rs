//! Stops strafing players' models twitching in an HLTV demo (#927): on the
//! frame the demo clock steps back, each player model is drawn where it was
//! drawn the frame before. Its own switch, `ds_player_fix_model_flicker`
//! (on by default), not part of `ds_spec_match_pov`: it is a bug in how the
//! game draws a model, which HLTV playback only shows more often, not a
//! difference in what an HLTV demo records, so it is wanted with or without
//! the match-POV changes. It stands down in a POV demo, where the clock
//! (almost) never steps back and the hold is untested.
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
//! This wraps the renderer's `StudioDrawPlayer`. Each player has a mark: the
//! clock and origin of the last frame he was drawn at unheld. On a frame whose
//! clock is behind his mark, a player drawn on the frame before (held or not)
//! is drawn at the mark's origin again: the renderer sees no movement, keeps
//! the legs where they were, and the torso blend stays put. That is every
//! frame until the clock passes the mark again, not just the first: at 1/4
//! speed and 239.5 fps a frame moves the clock about 1 ms, the size of the
//! step back, so the few frames after one are behind the mark too. The
//! entity's own origin is put back after the call, so nothing else sees a
//! change. The model is drawn at most a fraction of a unit from where it would
//! have been, for those few frames.
//!
//! It has to be the renderer's call: `HUD_AddEntity` sees the same entity,
//! but the engine sets the origin it draws from after that (measured: an
//! origin written there never reached the gait estimate).

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::engine::{self, ClEntityS, Vec3};
use crate::names::console_name;
use crate::setting::Setting;

/// The switch: 1 holds the models on a step-back frame (#927).
pub const NAME: &str = console_name!("player_fix_model_flicker");

/// The cvar, or the fallback toggle when it could not be registered.
pub static FIX_MODEL_FLICKER: Setting = Setting::new(NAME, true);

/// Whether the switch is on: the cvar, or the fallback toggle without one.
pub fn enabled() -> bool {
    FIX_MODEL_FLICKER.on()
}

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

/// Per player, the mark: the origin he was last drawn at unheld and the clock
/// of that frame. Held frames leave it alone, so it is the furthest point he
/// has been shown at.
static MARK_ORIGIN: [[AtomicU32; 3]; MAX_PLAYERS + 1] =
    [const { [const { AtomicU32::new(0) }; 3] }; MAX_PLAYERS + 1];
static MARK_CLOCK: [AtomicU64; MAX_PLAYERS + 1] =
    [const { AtomicU64::new(NEVER) }; MAX_PLAYERS + 1];
/// Per player: the clock of the frame he was last drawn on, held or not, and
/// of the frame he was last held on.
static LAST_CLOCK: [AtomicU64; MAX_PLAYERS + 1] =
    [const { AtomicU64::new(NEVER) }; MAX_PLAYERS + 1];
static HELD_CLOCK: [AtomicU64; MAX_PLAYERS + 1] =
    [const { AtomicU64::new(NEVER) }; MAX_PLAYERS + 1];

/// Player frames held so far, for `ds_debug_status`.
static HELD: AtomicU32 = AtomicU32::new(0);

/// Whether a player is held this frame. `now` is this frame's clock,
/// `previous` the last frame's, `mark` the clock of the frame the player was
/// last drawn on unheld, `last_drawn` the clock of the frame he was last drawn
/// on held or not, `last_held` the clock of the frame he was last held on (f64
/// bits, [`NEVER`] for none).
///
/// Every frame below the mark is held, not just the first: at 1/4 speed a
/// frame moves the clock about as far as the step back, so the frames after
/// one are still behind the mark, and drawing him there points his legs
/// backwards just the same (#927, seen at `fps_max 239.5`).
fn should_hold(now: f64, previous: f64, mark: u64, last_drawn: u64, last_held: u64) -> bool {
    if last_held == now.to_bits() {
        // Drawn twice this frame: the same answer both times.
        return true;
    }
    if mark == NEVER {
        return false;
    }
    let mark = f64::from_bits(mark);
    let behind = now < mark && mark - now < MAX_STEP_BACK;
    // A player missing from the frame right before has gone out of view; his
    // mark may be anywhere by now.
    behind && last_drawn == previous.to_bits()
}

/// One player's clocks, f64 bits: the mark's, the last frame he was drawn on,
/// and the last frame he was held on. The statics hold them between frames.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Track {
    mark: u64,
    last_drawn: u64,
    last_held: u64,
}

impl Track {
    /// Decides this draw and moves the clocks on: true when he is drawn at
    /// the mark's origin, false when he is drawn where he is and that becomes
    /// the mark.
    fn draw(&mut self, now: f64, previous: f64) -> bool {
        let hold = should_hold(now, previous, self.mark, self.last_drawn, self.last_held);
        if hold {
            self.last_held = now.to_bits();
        } else {
            self.mark = now.to_bits();
        }
        self.last_drawn = now.to_bits();
        hold
    }
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
    if !enabled() || crate::anim_fix::pov_demo_playing() {
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
    let mut track = Track {
        mark: MARK_CLOCK[slot].load(Ordering::Relaxed),
        last_drawn: LAST_CLOCK[slot].load(Ordering::Relaxed),
        last_held: HELD_CLOCK[slot].load(Ordering::Relaxed),
    };
    let held_before = track.last_held;
    let hold = track.draw(now, previous);
    MARK_CLOCK[slot].store(track.mark, Ordering::Relaxed);
    LAST_CLOCK[slot].store(track.last_drawn, Ordering::Relaxed);
    HELD_CLOCK[slot].store(track.last_held, Ordering::Relaxed);
    if !hold {
        let result = unsafe { real(flags, pplayer) };
        let store = &MARK_ORIGIN[slot];
        store[0].store(e.origin.x.to_bits(), Ordering::Relaxed);
        store[1].store(e.origin.y.to_bits(), Ordering::Relaxed);
        store[2].store(e.origin.z.to_bits(), Ordering::Relaxed);
        return result;
    }
    if held_before != track.last_held {
        HELD.fetch_add(1, Ordering::Relaxed);
    }
    let held = &MARK_ORIGIN[slot];
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
    let result = unsafe { real(flags, pplayer) };
    // Safety: the same entity; still this call's.
    let e = unsafe { &mut *ent };
    e.origin = saved;
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

    /// Plays one player through `draws` the way [`draw_player`] does: each is
    /// a frame's clock and whether he is drawn on it, and the same clock twice
    /// in a row is one frame's two views.
    /// Returns, per draw, whether he was held.
    fn play(draws: &[(f64, bool)]) -> Vec<bool> {
        let mut track = Track {
            mark: NEVER,
            last_drawn: NEVER,
            last_held: NEVER,
        };
        let (mut frame, mut previous) = (NEVER, 0u64);
        let mut held = Vec::new();
        for &(now, drawn) in draws {
            if frame != now.to_bits() {
                previous = frame;
                frame = now.to_bits();
            }
            if drawn {
                held.push(track.draw(now, f64::from_bits(previous)));
            }
        }
        held
    }

    fn drawn(clocks: &[f64]) -> Vec<(f64, bool)> {
        clocks.iter().map(|&t| (t, true)).collect()
    }

    #[test]
    fn a_frame_after_a_step_back_holds_and_the_next_one_on_draws() {
        let held = play(&drawn(&[17.9270, 17.9295, 17.9281, 17.9342]));
        assert_eq!(held, [false, false, true, false]);
    }

    /// The 1/4-speed case (#927, `fps_max 239.5`): after the step back the
    /// clock moves on about 1 ms a frame, so the next frames are still behind
    /// the last one he was drawn on unheld. Each holds at that frame's origin,
    /// until the clock passes it.
    #[test]
    fn every_frame_behind_the_mark_holds_at_quarter_speed() {
        let held = play(&drawn(&[
            17.9285, 17.9295, 17.9281, 17.9284, 17.9288, 17.9292, 17.9296, 17.9300,
        ]));
        assert_eq!(held, [false, false, true, true, true, true, false, false]);
    }

    #[test]
    fn two_steps_back_in_a_row_both_hold() {
        let held = play(&drawn(&[17.9295, 17.9281, 17.9270, 17.9300]));
        assert_eq!(held, [false, true, true, false]);
    }

    #[test]
    fn a_seek_back_draws_at_once() {
        let held = play(&drawn(&[17.9295, 10.0, 10.0042, 10.0084]));
        assert_eq!(held, [false, false, false, false]);
    }

    #[test]
    fn a_player_not_drawn_last_frame_draws() {
        let held = play(&[(17.9295, true), (17.9300, false), (17.9281, true)]);
        assert_eq!(held, [false, false]);
    }

    /// The spectator inset draws a frame twice: both views get one answer.
    #[test]
    fn a_frame_drawn_twice_gives_the_same_answer_both_times() {
        let held = play(&drawn(&[
            17.9295, 17.9295, 17.9281, 17.9281, 17.9288, 17.9288, 17.9300, 17.9300,
        ]));
        assert_eq!(held, [false, false, true, true, true, true, false, false]);
    }

    #[test]
    fn nothing_holds_before_a_first_draw() {
        assert!(!should_hold(
            17.9281,
            17.9295,
            NEVER,
            17.9295f64.to_bits(),
            NEVER
        ));
    }

    /// #927: the fix has its own switch, on by default, and
    /// `ds_spec_match_pov` turns it neither on nor off.
    #[test]
    fn the_switch_is_its_own_and_on_by_default() {
        // anim_fix's tests mutate LEVEL too; cargo runs tests in parallel.
        let _statics = crate::anim_fix::tests::lock_statics();
        let level = crate::anim_fix::LEVEL.load(Ordering::Relaxed);
        assert!(FIX_MODEL_FLICKER.value().is_none());
        assert!(enabled(), "on by default");
        for match_pov in [0, 1] {
            crate::anim_fix::LEVEL.store(match_pov, Ordering::Relaxed);
            FIX_MODEL_FLICKER.fallback.store(true, Ordering::Relaxed);
            assert!(enabled(), "match_pov {match_pov}");
            FIX_MODEL_FLICKER.fallback.store(false, Ordering::Relaxed);
            assert!(!enabled(), "match_pov {match_pov}");
        }
        FIX_MODEL_FLICKER.fallback.store(true, Ordering::Relaxed);
        crate::anim_fix::LEVEL.store(level, Ordering::Relaxed);
    }
}
