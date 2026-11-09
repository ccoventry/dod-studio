//! `ds_sound_headshot_far`: the headshot sound (`player/helmet_hit.wav`)
//! falls off half as fast with distance, so a headshot is heard twice as far
//! away. No `.wav` file or demo is touched.
//!
//! ## Where the sound comes from
//!
//! Not from the server: DoD's client plays it from its
//! `events/effects/bodydamage.sc` handler (`client.dll+0xb790`, registered in
//! `HUD_Init` beside `roundrestart.sc`), which the server fires on a head hit.
//! When the event's `bparam1` is 0 the handler calls
//!
//! ```text
//!     EV_PlaySound(args->entindex, args->origin, CHAN_STATIC,
//!                  "player/helmet_hit.wav", RandomFloat(0.92, 1.0),
//!                  0.8 /* ATTN_NORM */, 0, 100)
//! ```
//!
//! so it passes `fire_sounds`' `EV_PlaySound` hook, which asks
//! [`attenuation`] what to play it with. `client.dll` is byte-identical across
//! installs and the hook is on the event API table, so this needs no per-build
//! table. (`events/effects/helmet.sc`, the helmet flying off, plays no sound.)
//!
//! ## What "further" means
//!
//! GoldSrc fades a sound linearly with distance: the gain is
//! `1 - distance * attenuation / 1000`, so at `ATTN_NORM` (0.8) the headshot
//! is silent from 1250 units on, and many are further than that: half the
//! 26 headshots the kill feed shows in `ktps8w9-stealth_gskill_rr2_h1` are
//! over 1000 units from the killer. Halving the attenuation stretches the
//! silence to 2500 units: at 570 units the gain goes from 0.54 to 0.77
//! (+3 dB), at 1500 from nothing to 0.4. Volume cannot be raised past 1.0 in
//! the engine, so distance is the only thing to give; `ATTN_NONE` (0) would
//! play every headshot across the map at full volume, which is more than
//! "carries further".
//!
//! ## Whose headshot
//!
//! The event's entity is the player hit. The engine does not place a sound
//! from the view entity in space (measured: the POV recorder's own head hit
//! plays at full level in both ears, setting on or off), so in a POV demo the
//! recorder's own headshot deaths are unchanged; every head hit he sees or
//! hears someone else take, his own kills included, carries further.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::{self, ClEntityS};
use crate::names::console_name;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("sound_headshot_far");
pub static CVAR: Cvar = Cvar::new();

/// The sample DoD plays for a head hit.
const SAMPLE: &[u8] = b"player/helmet_hit.wav";
/// How much of its attenuation a headshot keeps with the setting on.
const FAR_SCALE: f32 = 0.5;

// Counters, not per-call logging: read by `ds_debug_status`.
static PLAYED: AtomicU32 = AtomicU32::new(0);
static STRETCHED: AtomicU32 = AtomicU32::new(0);
/// The last headshot's distance from the local player, in whole units.
static LAST_DISTANCE: AtomicU32 = AtomicU32::new(u32::MAX);

/// The attenuation to play `sample` with: the headshot's halved while the
/// setting is on, every other sound's as the game asked.
fn scaled(sample: &[u8], attenuation: f32, far: bool) -> f32 {
    if far && sample == SAMPLE {
        attenuation * FAR_SCALE
    } else {
        attenuation
    }
}

/// Called by `fire_sounds` for every `EV_PlaySound`, on the engine thread.
///
/// # Safety
/// `origin` is null or three floats, as the engine passes it.
pub unsafe fn attenuation(sample: &CStr, origin: *const f32, attenuation: f32) -> f32 {
    let sample = sample.to_bytes();
    if sample != SAMPLE {
        return attenuation;
    }
    PLAYED.fetch_add(1, Ordering::Relaxed);
    if !origin.is_null()
        && let Some(local) = local_player()
    {
        // Safety: the engine passes the event's origin, three floats.
        let at = unsafe { std::slice::from_raw_parts(origin, 3) };
        let o = &local.origin;
        let d = ((at[0] - o.x).powi(2) + (at[1] - o.y).powi(2) + (at[2] - o.z).powi(2)).sqrt();
        if d.is_finite() {
            LAST_DISTANCE.store(d as u32, Ordering::Relaxed);
        }
    }
    let far = CVAR.on();
    if far {
        STRETCHED.fetch_add(1, Ordering::Relaxed);
    }
    scaled(sample, attenuation, far)
}

fn local_player() -> Option<&'static ClEntityS> {
    let engfuncs = engine::engfuncs()?;
    // Safety: the engine's own accessor; a `cl_entity_t` that lives as long
    // as the level, read on the engine thread.
    let local = unsafe { (engfuncs.get_local_player)() } as *const ClEntityS;
    (!local.is_null()).then(|| unsafe { &*local })
}

/// What the hook has seen this session, for `ds_debug_status`.
pub fn status() -> String {
    let last = match LAST_DISTANCE.load(Ordering::Relaxed) {
        u32::MAX => "none yet".to_string(),
        d => format!("{d} units from the local player"),
    };
    format!(
        "{} headshot sounds, {} played with half the falloff; the last {last}",
        PLAYED.load(Ordering::Relaxed),
        STRETCHED.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_headshot_sound_carries_further() {
        assert_eq!(scaled(b"player/helmet_hit.wav", 0.8, true), 0.4);
        assert_eq!(scaled(b"player/helmet_hit.wav", 0.8, false), 0.8);
        assert_eq!(scaled(b"weapons/garand_shoot.wav", 0.8, true), 0.8);
        assert_eq!(scaled(b"player/helmet_hit.wav2", 0.8, true), 0.8);
        assert_eq!(scaled(b"", 0.8, true), 0.8);
    }
}
