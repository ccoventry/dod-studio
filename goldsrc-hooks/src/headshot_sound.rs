//! `ds_sound_headshot_far`: the headshot sound (`player/helmet_hit.wav`)
//! falls off half as fast with distance, so a headshot is heard twice as far
//! away. No `.wav` file or demo is touched. `ds_sound_headshot_viewed_far`
//! keeps that to the headshots the viewed player makes (#1353).
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
//!
//! ## The viewed player's headshots (#1353)
//!
//! Most far headshots in a POV demo are other players' kills: inside the
//! WSOD25 project's Keep clips, 8 of the recorder's own against 91 of someone
//! else's at 1250-2500 units, the band only this setting makes heard. With
//! `ds_sound_headshot_viewed_far` on, only a head hit the viewed player made
//! carries further: the POV recorder, or the player an HLTV demo watches in
//! first person (`deathmsg::is_watched_player`).
//!
//! The event names the victim, not the shooter. The kill's `DeathMsg` names
//! both, and the engine dispatches a packet's user messages while it parses
//! it, before `CL_FireEvents` plays that frame's events, so [`note_kill`]
//! has usually seen the kill by the time the sound asks: over 40 WSOD25 POV
//! demos the `DeathMsg` came in the same frame as 188 of the recorder's 197
//! headshot kills, and a frame later for the other 9, which play at the
//! game's distance. A head hit that doesn't kill has no `DeathMsg` and counts
//! as someone else's.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::engine::{self, ClEntityS};
use crate::names::console_name;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("sound_headshot_far");
pub static CVAR: Cvar = Cvar::new();
pub const VIEWED_FAR_NAME: &str = console_name!("sound_headshot_viewed_far");
pub static VIEWED_FAR: Cvar = Cvar::new();

/// The sample DoD plays for a head hit.
const SAMPLE: &[u8] = b"player/helmet_hit.wav";
/// How much of its attenuation a headshot keeps with the setting on.
const FAR_SCALE: f32 = 0.5;

/// How far apart, in client seconds, a kill's `DeathMsg` and its head hit may
/// be to pair. They share a frame when they pair at all; the slack covers a
/// frame time and no more, so an earlier kill of the same player isn't taken
/// for this one.
const KILL_WINDOW: f32 = 0.1;
/// How many recent kills are kept: more than can die in one frame.
const KILLS_KEPT: usize = 8;

/// Recent kills, `pack`ed, written by the `DeathMsg` hook and read by the
/// sound hook, both on the engine thread: atomics, not a lock.
static KILLS: [AtomicU64; KILLS_KEPT] = [const { AtomicU64::new(0) }; KILLS_KEPT];
static NEXT_KILL: AtomicUsize = AtomicUsize::new(0);

// Counters, not per-call logging: read by `ds_debug_status`.
static PLAYED: AtomicU32 = AtomicU32::new(0);
static STRETCHED: AtomicU32 = AtomicU32::new(0);
/// Headshots left at the game's distance because the viewed player didn't make them.
static NOT_VIEWED: AtomicU32 = AtomicU32::new(0);
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

/// A kill as [`KILLS`] keeps it: a set bit, killer, victim, the client time.
fn pack(killer: i32, victim: i32, time: f32) -> u64 {
    1 << 48
        | u64::from(killer as u8) << 40
        | u64::from(victim as u8) << 32
        | u64::from(time.to_bits())
}

fn unpack(kill: u64) -> Option<(i32, i32, f32)> {
    (kill >> 48 & 1 == 1).then(|| {
        (
            i32::from((kill >> 40) as u8),
            i32::from((kill >> 32) as u8),
            f32::from_bits(kill as u32),
        )
    })
}

/// Who killed `victim` within [`KILL_WINDOW`] of `now`, the newest kill first.
fn killer_of(kills: impl Iterator<Item = (i32, i32, f32)>, victim: i32, now: f32) -> Option<i32> {
    kills
        .filter(|&(_, v, t)| v == victim && (now - t).abs() <= KILL_WINDOW)
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(k, _, _)| k)
}

/// Whether a headshot carries further: the setting is on, and either it's for
/// every headshot or the viewed player made this one.
fn carries_far(far: bool, viewed_only: bool, by_viewed: impl FnOnce() -> bool) -> bool {
    far && (!viewed_only || by_viewed())
}

/// Whether the `DeathMsg` hook must stay installed for [`note_kill`].
pub fn wants_kills() -> bool {
    CVAR.on() && VIEWED_FAR.on()
}

/// Called by the `DeathMsg` hook for every kill as it was recorded, before
/// the game handles it.
pub fn note_kill(killer: i32, victim: i32) {
    let Some(now) = engine::engine_client_time() else {
        return;
    };
    let slot = NEXT_KILL.fetch_add(1, Ordering::Relaxed) % KILLS_KEPT;
    KILLS[slot].store(pack(killer, victim, now), Ordering::Relaxed);
}

/// Whether the viewed player killed `victim` this frame.
fn by_viewed_player(victim: i32) -> bool {
    let Some(now) = engine::engine_client_time() else {
        return false;
    };
    let kills = KILLS
        .iter()
        .filter_map(|k| unpack(k.load(Ordering::Relaxed)));
    killer_of(kills, victim, now).is_some_and(crate::deathmsg::is_watched_player)
}

/// Called by `fire_sounds` for every `EV_PlaySound`, on the engine thread.
/// `ent` is the event's entity: for the headshot, the player hit.
///
/// # Safety
/// `origin` is null or three floats, as the engine passes it.
pub unsafe fn attenuation(sample: &CStr, ent: i32, origin: *const f32, attenuation: f32) -> f32 {
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
    let viewed_only = VIEWED_FAR.on();
    let far = carries_far(CVAR.on(), viewed_only, || by_viewed_player(ent));
    if far {
        STRETCHED.fetch_add(1, Ordering::Relaxed);
    } else if CVAR.on() && viewed_only {
        NOT_VIEWED.fetch_add(1, Ordering::Relaxed);
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
        "{} headshot sounds, {} played with half the falloff, {} at the game's because the viewed player didn't make them; the last {last}",
        PLAYED.load(Ordering::Relaxed),
        STRETCHED.load(Ordering::Relaxed),
        NOT_VIEWED.load(Ordering::Relaxed),
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

    #[test]
    fn a_kill_packs_and_unpacks() {
        assert_eq!(unpack(pack(13, 3, 789.65)), Some((13, 3, 789.65)));
        assert_eq!(unpack(pack(0, 32, 0.0)), Some((0, 32, 0.0)));
        assert_eq!(unpack(0), None);
    }

    #[test]
    fn the_hit_pairs_with_its_own_kill_only() {
        let kills = [(13, 3, 789.65), (4, 6, 789.70), (13, 6, 700.0)];
        assert_eq!(killer_of(kills.into_iter(), 3, 789.65), Some(13));
        assert_eq!(killer_of(kills.into_iter(), 6, 789.70), Some(4));
        // An earlier kill of the same player is not this one.
        assert_eq!(killer_of(kills.into_iter(), 6, 701.0), None);
        // A head hit nobody died of has no killer.
        assert_eq!(killer_of(kills.into_iter(), 9, 789.65), None);
        // A seek back puts the clock before the kill: still too far apart.
        assert_eq!(killer_of(kills.into_iter(), 3, 780.0), None);
    }

    #[test]
    fn viewed_only_keeps_far_to_the_viewed_players_headshots() {
        assert!(carries_far(true, false, || false));
        assert!(carries_far(true, true, || true));
        assert!(!carries_far(true, true, || false));
        assert!(!carries_far(false, true, || true));
        assert!(!carries_far(false, false, || true));
    }
}
