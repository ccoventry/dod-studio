//! A spectated grenade, animated the way the thrower's own recording shows it.
//!
//! ## What a POV demo plays
//!
//! Measured over 145 POV match demos, 4730 throws, and one recorded for the
//! purpose with every kind of throw in it
//! (`analysis/examples/grenade_pov_timeline_probe.rs`). All times are from
//! the `throw` animation, which lands with `weapons/grenthrow.wav`. Before
//! it there is `pinpull`, a median 0.6s ahead.
//!
//! | what the player did | hand grenade | stick grenade |
//! | --- | --- | --- |
//! | plain throw, a grenade left | `draw` at once | `draw` at +0.5s |
//! | plain throw of the last one | next weapon at once | next weapon at +0.5s |
//! | primed it: rolled it out and caught it again with USE (nine throws in ten in a match) | `draw` at once, then as the stick | `exploding_idle` at the catch; `exploding_pinpull` when fire is pressed again; the next weapon when the live grenade is thrown |
//!
//! The two grenades really do differ: the hand grenade's code works in
//! weapon-time, the stick's in absolute time, and the stick waits its idle
//! out before it draws or retires. The Mills bomb is the hand grenade's
//! class and is treated as one; no recording of it was to hand.
//!
//! ## What a spectator can see of that
//!
//! Not the viewmodel, and not the buttons. But everything above has a trace
//! in what every recording carries
//! (`analysis/examples/grenade_prime_probe.rs`, one HLTV half, 329 throws):
//!
//! - **The release.** The thrower's body enters its grenade attack 0.5s
//!   before the throw (`m_flStartThrow = time + 0.5`). The pin pull itself is
//!   never networked, so `pinpull` plays here, about 0.1s later than the
//!   thrower saw it.
//! - **The throw.** 0.5s after that. A world grenade (`w_stick`, `w_grenade`,
//!   `w_mills`) appears beside the thrower in the same update as the sound.
//! - **The catch.** A caught grenade leaves the world again a median 0.13s
//!   after it appeared (300 of 322 throws; the other 22 lived 1.5s or more,
//!   to their fuse). The thrower still holds the grenade model.
//!   A hand grenade that was the last one has already given way to the next
//!   weapon by then (it retires at the throw), so there the catch shows as
//!   the held model changing *back* to the grenade within that time.
//! - **The primed throw.** A new world grenade beside the thrower, with no
//!   sound and usually no body animation, and the held model changing in the
//!   same update (299 of 300).
//!
//! The next weapon coming up needs nothing from here: the held model is
//! replicated and changes in the same update as the thrower's viewmodel, so
//! `apply()`'s own weapon-change draw is already on time.
//!
//! The one thing with no trace is when the thrower pressed fire to wind the
//! primed grenade up. `exploding_pinpull` plays at the median instead: 0.7s
//! after the catch on the stick, 1.1s on the hand grenade.
//!
//! ## Called from `apply()`
//!
//! [`wind_up`] at the body change, [`each_frame`] once per frame while the
//! spectated player holds a grenade, [`weapon_changed`] when the weapon in
//! hand changes, [`forget`] when the camera does.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use super::classify::{ATTACK_SEQUENCES, DeployState, model_stem};
use super::play_viewmodel_animation;
use super::sequences::{animation_lookup_any, animation_lookup_sequence};
use crate::engine::{self, ClEngineFuncsPartial, ClEntityS, ModelSPartial};

/// How long the server holds a released grenade before it leaves the hand:
/// `m_flStartThrow = gpGlobals->time + 0.5` in `dod.dll`'s `WeaponIdle`
/// (hand `0x9c66`, stick `0x15296`, the same double constant at `0xcf5c8`).
/// A POV demo's `throw` lands 0.49-0.53s after the body change.
pub(super) const WINDUP_SECONDS: f64 = 0.5;

/// A booked throw first seen this long after it was due is dropped instead of
/// played. `apply()` only reaches it on frames that are in-eye on this
/// grenade; if none came for this long the camera was elsewhere, and the
/// grenade is long gone. A hitch hands `HUD_Frame` at most 1.0s at a time
/// (`engine::tramp_hud_frame`), so an honest frame can never be later.
const STALE_THROW_SECONDS: f64 = 1.0;

/// How long after the throw to keep looking for the grenade it put in the
/// world. It appears in the same update as the sound, but the throw here is
/// booked off the body change and can be a few frames early.
const FIND_WITHIN_SECONDS: f64 = 0.35;
/// A thrown grenade never seen in the world by this long after the throw was
/// caught before the next update could carry it. A thrown one flies for
/// seconds and appears in the same update as the throw's sound; the booked
/// throw lands within a frame or two of that. Seen on Gorilla, who primes
/// nearly every stick and catches it 0.0-0.2s after the throw in his own POV
/// demo (`wsod25_ply2_m1_h1_gorilla`): in the HLTV demo of that half
/// (`monday-wsod25_r07_m1_h1_hltv`, 873.7s) the grenade never appears, and
/// a fresh stick was drawn where his own view held the lit one.
const UNSEEN_MEANS_CAUGHT_SECONDS: f64 = 0.2;
/// A thrown grenade that leaves the world this soon was caught. The shortest
/// one left to its fuse lived 1.53s; the longest catch took 1.5s.
const CAUGHT_WITHIN_SECONDS: f64 = 1.5;
/// With a stick grenade left, the thrower's own view draws it this long
/// after the throw (+0.500s on every one of 12 such throws). The hand grenade
/// draws at once instead; see [`draws_at_once`].
const NEXT_GRENADE_SECONDS: f64 = 0.5;
/// How long after the catch the wind-up of the primed grenade plays: the POV
/// median, since the press itself cannot be seen. Sticks 0.7s (`+0.8s` from
/// the throw less the catch at `+0.1s`), hand grenades 1.1s.
const WIND_UP_AFTER_CATCH_STICK: f64 = 0.7;
const WIND_UP_AFTER_CATCH_HAND: f64 = 1.1;
/// Nothing about a throw matters this long after it.
const FORGET_AFTER_SECONDS: f64 = 12.0;
/// How far from the thrower a new grenade can be and still be theirs.
const ARM_REACH: f32 = 96.0;
/// Entity indices to look through for a world grenade. Players are 1 to 32.
const FIRST_OTHER_ENTITY: i32 = 33;
const LAST_ENTITY: i32 = 2048;

// Demo times as f64 bits, zero for "none".
/// When the booked throw is due.
static THROW_AFTER: AtomicU64 = AtomicU64::new(0);
/// When the throw played.
static THROWN_AT: AtomicU64 = AtomicU64::new(0);
/// When the thrown grenade was seen to have been caught.
static CAUGHT_AT: AtomicU64 = AtomicU64::new(0);
/// When the primed grenade was seen thrown.
static PRIMED_THROWN_AT: AtomicU64 = AtomicU64::new(0);
/// The world grenade being watched, or -1.
static WATCHED: AtomicI32 = AtomicI32::new(-1);
static DREW_NEXT: AtomicBool = AtomicBool::new(false);
static WOUND_UP: AtomicBool = AtomicBool::new(false);
/// When this player last threw a grenade and then changed weapon, so the
/// grenade coming back into hand can be read as a catch.
static THREW_THEN_SWITCHED_AT: AtomicU64 = AtomicU64::new(0);
/// Which entities were world grenades on the last frame looked at, one bit
/// each, so a grenade that has just appeared can be told from one that was
/// thrown at the player from somewhere else.
static WERE_GRENADES: [AtomicU64; (LAST_ENTITY as usize + 64) / 64] =
    [const { AtomicU64::new(0) }; (LAST_ENTITY as usize + 64) / 64];

fn time(cell: &AtomicU64) -> Option<f64> {
    let at = f64::from_bits(cell.load(Ordering::Relaxed));
    (at != 0.0).then_some(at)
}

fn report(line: &str) {
    unsafe { crate::debug::report(&format!("anim_fix: {line}")) };
}

/// Drops everything known about a throw, saying why if there was one.
pub(super) fn forget(why: &str) {
    THREW_THEN_SWITCHED_AT.store(0, Ordering::Relaxed);
    forget_the_throw(why);
}

fn forget_the_throw(why: &str) {
    let booked = THROW_AFTER.swap(0, Ordering::Relaxed) != 0;
    let thrown = THROWN_AT.swap(0, Ordering::Relaxed) != 0;
    CAUGHT_AT.store(0, Ordering::Relaxed);
    PRIMED_THROWN_AT.store(0, Ordering::Relaxed);
    WATCHED.store(-1, Ordering::Relaxed);
    DREW_NEXT.store(false, Ordering::Relaxed);
    WOUND_UP.store(false, Ordering::Relaxed);
    if booked {
        report(&format!("pending throw cancelled -- {why} before it fired"));
    } else if thrown {
        report(&format!("grenade forgotten -- {why}"));
    }
}

/// How close to its due time a booked throw is taken to have happened when
/// the weapon changes first: the booking is off the body change and the
/// server's own half second runs 0.46s to 0.57s.
const THROW_SLACK_SECONDS: f64 = 0.15;

/// Whether a grenade that comes back into hand `since_throw` seconds after
/// one was thrown is that grenade, caught.
fn back_in_hand_is_a_catch(since_throw: f64) -> bool {
    (0.0..=CAUGHT_WITHIN_SECONDS).contains(&since_throw)
}

/// The weapon in hand changed. Returns whether this played the animation for
/// it, in which case the caller's ordinary `draw` must not.
///
/// A last hand grenade retires as it is thrown, so the next weapon is
/// already up when the thrower catches the grenade again, and the catch
/// arrives as the grenade coming back into hand. The thrower's own view plays
/// `exploding_idle` there, not `draw`.
pub(super) fn weapon_changed(
    now: f64,
    state: Option<DeployState>,
    viewmodel: *mut ModelSPartial,
    is_grenade: bool,
) -> bool {
    let thrown_at = time(&THROWN_AT).or_else(|| {
        // Booked and all but due: the grenade left as the weapon changed.
        time(&THROW_AFTER).filter(|due| (due - now).abs() <= THROW_SLACK_SECONDS)
    });
    let earlier = time(&THREW_THEN_SWITCHED_AT);
    let already_caught = time(&CAUGHT_AT).is_some();
    forget_the_throw("weapon changed");

    if !is_grenade {
        // Away from the grenade: remember the throw, if there was one, in
        // case the grenade comes back. Not one already caught -- this change
        // is then the live grenade being thrown, and the story is over.
        let remembered = thrown_at.filter(|_| !already_caught);
        THREW_THEN_SWITCHED_AT.store(remembered.map_or(0, f64::to_bits), Ordering::Relaxed);
        return false;
    }
    THREW_THEN_SWITCHED_AT.store(0, Ordering::Relaxed);
    let Some(thrown_at) = earlier.filter(|at| back_in_hand_is_a_catch(now - at)) else {
        return false;
    };
    THROWN_AT.store(thrown_at.to_bits(), Ordering::Relaxed);
    CAUGHT_AT.store(now.to_bits(), Ordering::Relaxed);
    DREW_NEXT.store(true, Ordering::Relaxed);
    play_viewmodel_animation(
        animation_lookup_sequence("exploding_idle", state, viewmodel),
        &format!(
            "the grenade is back in hand {:.3}s after it was thrown -- caught, so it is primed",
            now - thrown_at
        ),
        state,
        viewmodel,
    );
    true
}

/// The body entered its grenade attack: `pinpull` now, `throw` booked for
/// when the grenade actually leaves the hand.
///
/// `pinpull` is longer than the wind-up and is left to run; the throw cuts it
/// short, which is what a POV recording of a quick throw looks like too.
pub(super) fn wind_up(now: f64, state: Option<DeployState>, viewmodel: *mut ModelSPartial) {
    forget("another wind-up");
    THROW_AFTER.store((now + WINDUP_SECONDS).to_bits(), Ordering::Relaxed);
    play_viewmodel_animation(
        animation_lookup_sequence("pinpull", state, viewmodel),
        &format!("grenade wind-up, pin pull now and the throw in {WINDUP_SECONDS:.3}s"),
        state,
        viewmodel,
    );
}

/// Consumes the booked throw if its time has come. `false` when nothing is
/// booked, it is not due yet, or it went stale.
fn take_due_throw(now: f64) -> bool {
    let Some(due) = time(&THROW_AFTER) else {
        return false;
    };
    if now < due {
        return false;
    }
    THROW_AFTER.store(0, Ordering::Relaxed);
    let overdue = now - due;
    if overdue > STALE_THROW_SECONDS {
        report(&format!(
            "booked throw dropped -- {overdue:.3}s overdue, the frames it waited for were not in-eye on this grenade"
        ));
        return false;
    }
    true
}

/// What to do next about a grenade that has been thrown.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Step {
    Nothing,
    /// The grenade left the world again: the thrower caught it.
    Catch,
    /// Time for the primed grenade's wind-up.
    WindUp,
    /// The primed grenade is in the world.
    PrimedThrow,
    /// A grenade is still in hand and nothing was caught: draw it.
    DrawNext,
    Forget,
}

/// Everything [`next_step`] decides from.
#[derive(Clone, Copy)]
struct Throw {
    /// Seconds since the throw played.
    since_throw: f64,
    /// Whether the thrown grenade was ever found in the world.
    found: bool,
    /// Whether it is there now.
    present: bool,
    /// Seconds since it was caught.
    since_catch: Option<f64>,
    /// Seconds since the primed grenade was thrown.
    since_primed_throw: Option<f64>,
    /// Whether a new world grenade is beside the thrower now.
    new_grenade: bool,
    drew_next: bool,
    wound_up: bool,
    wind_up_after: f64,
}

fn next_step(t: Throw) -> Step {
    if t.since_throw > FORGET_AFTER_SECONDS {
        return Step::Forget;
    }
    if let Some(since) = t.since_primed_throw {
        // The live grenade has gone. With another grenade still held, the
        // view draws it as after any throw; without, the weapon change that
        // comes in the same update has already ended all this.
        return if !t.drew_next && since >= NEXT_GRENADE_SECONDS {
            Step::DrawNext
        } else {
            Step::Nothing
        };
    }
    match t.since_catch {
        Some(since) => {
            if t.new_grenade {
                Step::PrimedThrow
            } else if !t.wound_up && since >= t.wind_up_after {
                Step::WindUp
            } else {
                Step::Nothing
            }
        }
        None => {
            // Caught: seen leaving the world soon after the throw, or never in
            // it at all.
            let left_soon = t.found && !t.present && t.since_throw <= CAUGHT_WITHIN_SECONDS;
            let never_seen = !t.found && t.since_throw >= UNSEEN_MEANS_CAUGHT_SECONDS;
            if left_soon || never_seen {
                Step::Catch
            } else if !t.drew_next && t.since_throw >= NEXT_GRENADE_SECONDS {
                Step::DrawNext
            } else {
                Step::Nothing
            }
        }
    }
}

/// Whether this grenade's own view draws again the moment it is thrown: the
/// hand grenade and the Mills bomb do, the stick waits half a second.
fn draws_at_once(viewmodel_name: &str) -> bool {
    model_stem(viewmodel_name) != "stick"
}

/// How long after the catch this grenade's primed wind-up plays.
fn wind_up_after_catch(viewmodel_name: &str) -> f64 {
    if draws_at_once(viewmodel_name) {
        WIND_UP_AFTER_CATCH_HAND
    } else {
        WIND_UP_AFTER_CATCH_STICK
    }
}

/// Whether a model is a grenade lying or flying in the world.
fn is_world_grenade(model_name: &str) -> bool {
    matches!(
        model_stem(model_name),
        "grenade" | "stick" | "mills" | "mills2"
    ) && model_name
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|file| file.starts_with("w_"))
}

/// Whether entity `index` is a world grenade in the same update as `player`.
fn world_grenade_at(
    engfuncs: &ClEngineFuncsPartial,
    index: i32,
    player: &ClEntityS,
) -> Option<&'static ClEntityS> {
    // Safety: the engine's own accessor, null past the last entity; a
    // non-null `cl_entity_t` lives as long as the frame.
    let entity = unsafe { (engfuncs.get_entity_by_index)(index).as_ref() }?;
    // An entity the latest update did not carry keeps its old state.
    if entity.curstate.messagenum != player.curstate.messagenum || entity.model.is_null() {
        return None;
    }
    // Safety: a non-null model the engine handed out.
    is_world_grenade(&unsafe { (*entity.model).name_str() }).then_some(entity)
}

/// The world grenades beside `player` this frame: the nearest one within
/// reach, and whether any within reach was not a world grenade last frame.
/// Remembers this frame's set for the next call.
fn grenades_beside(engfuncs: &ClEngineFuncsPartial, player: &ClEntityS) -> (Option<i32>, bool) {
    let here = player.curstate.origin;
    let mut nearest: Option<(i32, f32)> = None;
    let mut appeared = false;
    let mut now_grenades = [0u64; (LAST_ENTITY as usize + 64) / 64];
    for index in FIRST_OTHER_ENTITY..=LAST_ENTITY {
        // Safety: as `world_grenade_at`.
        if unsafe { (engfuncs.get_entity_by_index)(index) }.is_null() {
            break;
        }
        let Some(grenade) = world_grenade_at(engfuncs, index, player) else {
            continue;
        };
        let (word, bit) = (index as usize / 64, 1u64 << (index as usize % 64));
        now_grenades[word] |= bit;
        let there = grenade.curstate.origin;
        let distance =
            ((there.x - here.x).powi(2) + (there.y - here.y).powi(2) + (there.z - here.z).powi(2))
                .sqrt();
        if distance > ARM_REACH {
            continue;
        }
        if WERE_GRENADES[word].load(Ordering::Relaxed) & bit == 0 {
            appeared = true;
        }
        if nearest.is_none_or(|(_, d)| distance < d) {
            nearest = Some((index, distance));
        }
    }
    for (was, now) in WERE_GRENADES.iter().zip(now_grenades) {
        was.store(now, Ordering::Relaxed);
    }
    (nearest.map(|(index, _)| index), appeared)
}

/// Runs once per frame while the spectated player holds a grenade: plays the
/// booked throw when it is due, then follows the thrown grenade.
pub(super) fn each_frame(
    now: f64,
    state: Option<DeployState>,
    viewmodel: *mut ModelSPartial,
    viewmodel_name: &str,
    player: &ClEntityS,
) {
    if take_due_throw(now) {
        THROWN_AT.store(now.to_bits(), Ordering::Relaxed);
        play_viewmodel_animation(
            animation_lookup_any(ATTACK_SEQUENCES, state, viewmodel),
            &format!("scheduled throw fired, {WINDUP_SECONDS:.3}s after the wind-up"),
            state,
            viewmodel,
        );
        if draws_at_once(viewmodel_name) {
            DREW_NEXT.store(true, Ordering::Relaxed);
            play_viewmodel_animation(
                animation_lookup_sequence("draw", state, viewmodel),
                "a hand grenade draws again as it is thrown",
                state,
                viewmodel,
            );
        }
    }
    let Some(thrown_at) = time(&THROWN_AT) else {
        return;
    };
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let since_throw = now - thrown_at;
    let caught_at = time(&CAUGHT_AT);
    let primed_at = time(&PRIMED_THROWN_AT);

    // Find the grenade the throw put in the world, then watch it.
    let (nearest, appeared) = grenades_beside(engfuncs, player);
    let mut watched = WATCHED.load(Ordering::Relaxed);
    if watched < 0
        && caught_at.is_none()
        && since_throw <= FIND_WITHIN_SECONDS
        && let Some(found) = nearest
    {
        watched = found;
        WATCHED.store(found, Ordering::Relaxed);
        report(&format!(
            "the thrown grenade is entity {found}, {since_throw:.3}s after the throw"
        ));
    }
    let present = watched >= 0 && world_grenade_at(engfuncs, watched, player).is_some();
    // Once caught, the next grenade to appear beside them is the live one.
    let new_grenade = caught_at.is_some() && primed_at.is_none() && appeared;

    let step = next_step(Throw {
        since_throw,
        found: watched >= 0,
        present,
        since_catch: caught_at.map(|at| now - at),
        since_primed_throw: primed_at.map(|at| now - at),
        new_grenade,
        drew_next: DREW_NEXT.load(Ordering::Relaxed),
        wound_up: WOUND_UP.load(Ordering::Relaxed),
        wind_up_after: wind_up_after_catch(viewmodel_name),
    });
    match step {
        Step::Nothing => {}
        Step::Forget => forget("long after the throw"),
        Step::Catch => {
            CAUGHT_AT.store(now.to_bits(), Ordering::Relaxed);
            play_viewmodel_animation(
                animation_lookup_sequence("exploding_idle", state, viewmodel),
                &if WATCHED.load(Ordering::Relaxed) >= 0 {
                    format!(
                        "the thrown grenade left the world {since_throw:.3}s after the throw -- caught, so it is primed"
                    )
                } else {
                    format!(
                        "no thrown grenade appeared in the world by {since_throw:.3}s after the throw -- caught before the next update, so it is primed"
                    )
                },
                state,
                viewmodel,
            );
        }
        Step::WindUp => {
            WOUND_UP.store(true, Ordering::Relaxed);
            play_viewmodel_animation(
                animation_lookup_sequence("exploding_pinpull", state, viewmodel),
                "winding the primed grenade up",
                state,
                viewmodel,
            );
        }
        Step::PrimedThrow => {
            PRIMED_THROWN_AT.store(now.to_bits(), Ordering::Relaxed);
            DREW_NEXT.store(false, Ordering::Relaxed);
            play_viewmodel_animation(
                animation_lookup_sequence("exploding_throw", state, viewmodel),
                "the primed grenade is in the world -- thrown",
                state,
                viewmodel,
            );
        }
        Step::DrawNext => {
            DREW_NEXT.store(true, Ordering::Relaxed);
            play_viewmodel_animation(
                animation_lookup_sequence("draw", state, viewmodel),
                "a grenade is still in hand after the throw, drawing it",
                state,
                viewmodel,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_fix::tests::lock_statics;

    fn book(body_change: f64) {
        THROW_AFTER.store((body_change + WINDUP_SECONDS).to_bits(), Ordering::Relaxed);
    }

    /// The throw is booked for the server's wind-up after the body change --
    /// the grenade is still in the hand until then -- and consumed exactly
    /// once when its time comes, not on every frame afterwards.
    #[test]
    fn a_booked_throw_fires_at_the_windup_and_not_before() {
        let _statics = lock_statics();
        forget("a test starting");
        assert!(!take_due_throw(0.0), "nothing booked at the start");

        book(10.0);
        assert!(!take_due_throw(10.0), "not on the frame of the body change");
        assert!(!take_due_throw(10.25));
        assert!(!take_due_throw(10.0 + WINDUP_SECONDS - 0.001));
        assert!(time(&THROW_AFTER).is_some(), "still booked");

        assert!(take_due_throw(10.0 + WINDUP_SECONDS), "due now");
        assert!(time(&THROW_AFTER).is_none(), "consumed");
        assert!(!take_due_throw(10.51), "not twice");
        assert!(!take_due_throw(9999.0));

        // A frame that lands a little late still fires it -- a hitch can hand
        // the clock up to 1.0s in one step -- but one that lands long after
        // means the camera was elsewhere, and the throw is dropped, not
        // played on whatever grenade is in hand by then.
        book(20.0);
        assert!(take_due_throw(20.0 + WINDUP_SECONDS + STALE_THROW_SECONDS));
        book(30.0);
        assert!(!take_due_throw(
            30.0 + WINDUP_SECONDS + STALE_THROW_SECONDS + 0.001
        ));
        assert!(time(&THROW_AFTER).is_none(), "dropped, not kept");
    }

    /// A booked throw belongs to the grenade that was wound up. If the
    /// viewmodel is a different weapon by the time it would fire -- a death,
    /// a switch, or the camera moving to another player -- it must not play,
    /// and nothing of the throw before it may linger either.
    #[test]
    fn forgetting_cancels_the_booked_throw_and_what_followed_the_last_one() {
        let _statics = lock_statics();
        forget("a test starting");

        book(10.0);
        THROWN_AT.store(9.0f64.to_bits(), Ordering::Relaxed);
        CAUGHT_AT.store(9.1f64.to_bits(), Ordering::Relaxed);
        WATCHED.store(77, Ordering::Relaxed);
        WOUND_UP.store(true, Ordering::Relaxed);
        forget("weapon changed");
        assert!(!take_due_throw(10.0 + WINDUP_SECONDS), "cancelled");
        assert!(time(&THROWN_AT).is_none() && time(&CAUGHT_AT).is_none());
        assert_eq!(WATCHED.load(Ordering::Relaxed), -1);
        assert!(!WOUND_UP.load(Ordering::Relaxed));

        // And a wind-up after it books a fresh throw of its own.
        book(30.0);
        assert!(!take_due_throw(30.1));
        assert!(take_due_throw(30.0 + WINDUP_SECONDS));
    }

    /// A throw `since_throw` seconds old, the grenade found and still out.
    fn out(since_throw: f64) -> Throw {
        Throw {
            since_throw,
            found: true,
            present: true,
            since_catch: None,
            since_primed_throw: None,
            new_grenade: false,
            drew_next: false,
            wound_up: false,
            wind_up_after: WIND_UP_AFTER_CATCH_STICK,
        }
    }

    #[test]
    fn a_grenade_that_leaves_the_world_at_once_was_caught() {
        // The median catch: gone 0.13s after it appeared.
        assert_eq!(
            next_step(Throw {
                present: false,
                ..out(0.13)
            }),
            Step::Catch
        );
        // The slowest catch measured.
        assert_eq!(
            next_step(Throw {
                present: false,
                ..out(1.5)
            }),
            Step::Catch
        );
        // One that lasted to its fuse exploded; nobody caught it.
        assert_ne!(
            next_step(Throw {
                present: false,
                drew_next: true,
                ..out(3.1)
            }),
            Step::Catch
        );
    }

    #[test]
    fn a_grenade_never_seen_in_the_world_was_caught_at_once() {
        let unseen = |since: f64| Throw {
            found: false,
            present: false,
            ..out(since)
        };
        // Not yet: it can still turn up an update or two after the throw.
        assert_eq!(next_step(unseen(0.1)), Step::Nothing);
        // Caught before the next update -- the lit grenade is held, not a
        // fresh one drawn.
        assert_eq!(next_step(unseen(0.2)), Step::Catch);
        assert_ne!(next_step(unseen(0.5)), Step::DrawNext);
    }

    #[test]
    fn with_the_grenade_still_out_the_next_one_is_drawn_at_half_a_second() {
        assert_eq!(next_step(out(0.3)), Step::Nothing);
        assert_eq!(next_step(out(0.5)), Step::DrawNext);
        assert_eq!(
            next_step(Throw {
                drew_next: true,
                ..out(0.6)
            }),
            Step::Nothing
        );
        // A late catch still counts after the draw.
        assert_eq!(
            next_step(Throw {
                drew_next: true,
                present: false,
                ..out(0.9)
            }),
            Step::Catch
        );
    }

    #[test]
    fn a_caught_grenade_is_wound_up_at_the_median_and_only_once() {
        let caught = |since: f64| Throw {
            present: false,
            since_catch: Some(since),
            ..out(0.13 + since)
        };
        assert_eq!(next_step(caught(0.3)), Step::Nothing);
        assert_eq!(next_step(caught(0.7)), Step::WindUp);
        assert_eq!(
            next_step(Throw {
                wound_up: true,
                ..caught(0.9)
            }),
            Step::Nothing
        );
        // A hand grenade waits longer.
        let hand = Throw {
            wind_up_after: WIND_UP_AFTER_CATCH_HAND,
            ..caught(0.7)
        };
        assert_eq!(next_step(hand), Step::Nothing);
        // No draw while a caught grenade is held.
        assert_ne!(next_step(caught(0.6)), Step::DrawNext);
    }

    #[test]
    fn a_new_grenade_beside_a_player_holding_a_caught_one_is_the_primed_throw() {
        let t = Throw {
            present: false,
            since_catch: Some(2.4),
            new_grenade: true,
            wound_up: true,
            ..out(2.5)
        };
        assert_eq!(next_step(t), Step::PrimedThrow);
        // Before any catch, a second grenade nearby is somebody else's.
        assert_ne!(
            next_step(Throw {
                new_grenade: true,
                ..out(0.2)
            }),
            Step::PrimedThrow
        );
    }

    #[test]
    fn after_the_primed_throw_a_grenade_left_in_hand_is_drawn() {
        let thrown = |since: f64, drew_next: bool| Throw {
            present: false,
            since_catch: Some(2.4 + since),
            since_primed_throw: Some(since),
            wound_up: true,
            drew_next,
            ..out(2.5 + since)
        };
        assert_eq!(next_step(thrown(0.2, false)), Step::Nothing);
        assert_eq!(next_step(thrown(0.5, false)), Step::DrawNext);
        assert_eq!(next_step(thrown(0.6, true)), Step::Nothing);
    }

    #[test]
    fn a_throw_is_forgotten_in_the_end() {
        assert_eq!(
            next_step(Throw {
                drew_next: true,
                ..out(12.1)
            }),
            Step::Forget
        );
    }

    #[test]
    fn only_world_grenade_models_count() {
        assert!(is_world_grenade("models/w_stick.mdl"));
        assert!(is_world_grenade("models/w_grenade.mdl"));
        assert!(is_world_grenade("models/w_mills.mdl"));
        assert!(is_world_grenade("models/w_mills2.mdl"));
        assert!(!is_world_grenade("models/p_stick.mdl"));
        assert!(!is_world_grenade("models/v_grenade.mdl"));
        assert!(!is_world_grenade("models/w_garand.mdl"));
        assert!(!is_world_grenade("models/player/us-inf/us-inf.mdl"));
    }

    /// The Allied case: the last hand grenade retires at the throw, the rifle
    /// comes up, and the catch is the grenade coming back.
    #[test]
    fn a_grenade_back_in_hand_right_after_a_throw_is_a_catch() {
        assert!(back_in_hand_is_a_catch(0.65));
        assert!(back_in_hand_is_a_catch(0.0));
        assert!(back_in_hand_is_a_catch(CAUGHT_WITHIN_SECONDS));
        // Later than any catch: the player picked a grenade up, or drew a
        // second one.
        assert!(!back_in_hand_is_a_catch(3.0));
        assert!(!back_in_hand_is_a_catch(-0.1));
    }

    #[test]
    fn a_throw_is_remembered_across_the_weapon_it_gave_way_to() {
        let _statics = lock_statics();
        forget("a test starting");
        let nothing = std::ptr::null_mut();

        // Thrown at 10.0, rifle up at 10.0: remembered.
        THROWN_AT.store(10.0f64.to_bits(), Ordering::Relaxed);
        assert!(!weapon_changed(10.0, None, nothing, false));
        assert_eq!(time(&THREW_THEN_SWITCHED_AT), Some(10.0));
        assert!(time(&THROWN_AT).is_none());

        // A booked throw the weapon change beat by a frame counts as thrown.
        forget("between cases");
        book(20.0);
        assert!(!weapon_changed(20.47, None, nothing, false));
        assert_eq!(time(&THREW_THEN_SWITCHED_AT), Some(20.5));

        // One still a long way off does not: the player changed their mind.
        forget("between cases");
        book(30.0);
        assert!(!weapon_changed(30.1, None, nothing, false));
        assert!(time(&THREW_THEN_SWITCHED_AT).is_none());

        // A grenade already caught is not remembered: the change away from
        // it is the primed throw.
        forget("between cases");
        THROWN_AT.store(50.0f64.to_bits(), Ordering::Relaxed);
        CAUGHT_AT.store(50.1f64.to_bits(), Ordering::Relaxed);
        assert!(!weapon_changed(50.6, None, nothing, false));
        assert!(time(&THREW_THEN_SWITCHED_AT).is_none());

        // And the camera moving to another player forgets it.
        THREW_THEN_SWITCHED_AT.store(40.0f64.to_bits(), Ordering::Relaxed);
        forget("spectated player changed");
        assert!(time(&THREW_THEN_SWITCHED_AT).is_none());
    }

    #[test]
    fn the_hand_grenades_draw_at_once_and_the_stick_does_not() {
        assert!(draws_at_once("models/v_grenade.mdl"));
        assert!(draws_at_once("models/v_mills.mdl"));
        assert!(!draws_at_once("models/v_stick.mdl"));
    }

    #[test]
    fn the_stick_winds_up_sooner_than_the_hand_grenades() {
        assert_eq!(
            wind_up_after_catch("models/v_stick.mdl"),
            WIND_UP_AFTER_CATCH_STICK
        );
        assert_eq!(
            wind_up_after_catch("models/v_grenade.mdl"),
            WIND_UP_AFTER_CATCH_HAND
        );
        assert_eq!(
            wind_up_after_catch("models/v_mills.mdl"),
            WIND_UP_AFTER_CATCH_HAND
        );
    }
}
