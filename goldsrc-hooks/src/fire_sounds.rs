//! Watches DoD's weapon-fire sounds go past, and tells the two modules that
//! need to know a shot was fired and by whom.
//!
//! Every DoD 1.3 weapon's firing sample is named `<weapon>_shoot.wav`
//! (checked against the game's own sound files: garand_shoot, kar_shoot,
//! bar_shoot, mp44_shoot, luger_shoot, mg42_shoot, every one of them), and a
//! fire event's handler plays it through `EV_PlaySound` with the shooter's
//! entity index. So one hook on that function sees every shot that has a fire
//! event:
//!
//! - `anim_fix` animates the in-eye viewmodel from it, for the rounds of a
//!   held trigger that the body animation cannot show;
//! - `missing_shots` uses it to tell a round that came with its fire event
//!   from one the recording lost.
//!
//! The sound itself is passed through untouched.
//!
//! ## What this used to be
//!
//! `sound_fix`, which lowered the attenuation of gunshots so they carried
//! across the map, on the belief that an HLTV demo had every shot and only
//! distance was silencing them. It does not: well over half the rounds have no
//! fire event at all (`docs/goldsrc_hltv_missing_gunshots.md`). With those
//! restored, gunfire is heard as far as it is in a player's own recording,
//! which is what was wanted, and a setting that made it carry further than
//! that was dropped along with its two cvars.

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::engine::{self, EventApiPartial};

// Counters, not per-call logging: this runs for every sound the engine plays.
static CALLS: AtomicU32 = AtomicU32::new(0);
static SHOTS: AtomicU32 = AtomicU32::new(0);

/// What the hook has seen this session, for `dodstudio_debug_status`.
pub fn status() -> String {
    format!(
        "{} sounds through the hook, {} of them gunshots",
        CALLS.load(Ordering::Relaxed),
        SHOTS.load(Ordering::Relaxed),
    )
}

/// Substring test without allocating: this runs for every sound played.
fn is_weapon_fire(sample: &[u8]) -> bool {
    sample.windows(6).any(|w| w == b"_shoot")
}

type EvPlaySoundFn = unsafe extern "C" fn(i32, *mut f32, i32, *const c_char, f32, f32, i32, i32);
static REAL_EV_PLAY_SOUND: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe extern "C" fn hook_ev_play_sound(
    ent: i32,
    origin: *mut f32,
    channel: i32,
    sample: *const c_char,
    volume: f32,
    attenuation: f32,
    f_flags: i32,
    pitch: i32,
) {
    if CALLS.fetch_add(1, Ordering::Relaxed) == 0 {
        unsafe {
            crate::debug::report(
                "fire_sounds: EV_PlaySound hook is live (first sound played through it)",
            )
        };
    }
    if !sample.is_null() && is_weapon_fire(unsafe { CStr::from_ptr(sample) }.to_bytes()) {
        SHOTS.fetch_add(1, Ordering::Relaxed);
        crate::anim_fix::on_weapon_fired(ent);
        crate::missing_shots::on_shot_sound(ent);
    }
    let real = REAL_EV_PLAY_SOUND.load(Ordering::Acquire);
    // Safety: the engine's own function, captured at install.
    let real: EvPlaySoundFn = unsafe { std::mem::transmute(real) };
    unsafe {
        real(
            ent,
            origin,
            channel,
            sample,
            volume,
            attenuation,
            f_flags,
            pitch,
        )
    };
}

type EvWeaponAnimationFn = unsafe extern "C" fn(i32, i32);
static REAL_EV_WEAPON_ANIMATION: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// A fire handler asking for the first-person gun's fire animation. Passed
/// on unless `anim_fix` says the gun in hand can't be the one that fired.
unsafe extern "C" fn hook_ev_weapon_animation(sequence: i32, body: i32) {
    if !crate::anim_fix::allow_event_weapon_animation(sequence) {
        return;
    }
    let real = REAL_EV_WEAPON_ANIMATION.load(Ordering::Acquire);
    // Safety: the engine's own function, captured at install.
    let real: EvWeaponAnimationFn = unsafe { std::mem::transmute(real) };
    unsafe { real(sequence, body) };
}

/// Installs the `EV_PlaySound` hook. Must be called after `engine::engfuncs()`
/// returns `Some` (i.e. after `client.dll` has finished loading), since it
/// needs a valid `p_event_api` pointer to patch.
pub fn install() {
    let Some(engfuncs) = engine::engfuncs() else {
        unsafe {
            crate::debug::report(
                "fire_sounds::install called before engfuncs were captured -- this is a bug in install ordering",
            )
        };
        return;
    };

    let event_api: *mut EventApiPartial = engfuncs.p_event_api;
    if event_api.is_null() {
        unsafe { crate::debug::report("fire_sounds: pEventAPI is null, cannot install") };
        return;
    }

    unsafe {
        let real = (*event_api).ev_play_sound;
        REAL_EV_PLAY_SOUND.store(real as *mut c_void, Ordering::Release);
        (*event_api).ev_play_sound = hook_ev_play_sound;
        let real = (*event_api).ev_weapon_animation;
        REAL_EV_WEAPON_ANIMATION.store(real as *mut c_void, Ordering::Release);
        (*event_api).ev_weapon_animation = hook_ev_weapon_animation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_shoot_sample_is_a_gunshot() {
        assert!(is_weapon_fire(b"weapons/mp40_shoot.wav"));
        assert!(is_weapon_fire(b"weapons/garand_shoot.wav"));
        assert!(!is_weapon_fire(b"weapons/grenthrow.wav"));
        assert!(!is_weapon_fire(b"weapons/garand_reload_clipding.wav"));
        assert!(!is_weapon_fire(b"player/pl_step1.wav"));
        assert!(!is_weapon_fire(b""));
    }
}
